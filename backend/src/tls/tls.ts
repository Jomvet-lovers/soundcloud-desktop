import { promises as fs } from 'node:fs';
import * as http from 'node:http';
import * as path from 'node:path';
import * as tls from 'node:tls';
import { X509Certificate } from 'node:crypto';
import * as acme from 'acme-client';

export interface TlsConfig {
  domains: string[];
  email: string;
  cacheDir: string;
  staging: boolean;
  httpsPort: number;
  httpPort: number;
  httpRedirect: boolean;
}

export function tlsConfigFromEnv(): TlsConfig | null {
  if (!envBool('TLS_ENABLED', false)) return null;

  const domains = parseCsv(process.env.DOMAINS ?? '');
  if (domains.length === 0) {
    throw new Error(
      'TLS_ENABLED=true but DOMAINS is empty (expected comma-separated domain list)',
    );
  }

  return {
    domains,
    email: process.env.ACME_EMAIL ?? `admin@${domains[0]}`,
    cacheDir: process.env.ACME_CACHE_DIR ?? '/var/cache/acme',
    staging: envBool('ACME_STAGING', false),
    httpsPort: envU16('TLS_HTTPS_PORT', 443),
    httpPort: envU16('TLS_HTTP_PORT', 80),
    httpRedirect: envBool('TLS_HTTP_REDIRECT', true),
  };
}

const RENEW_BEFORE_DAYS = 30;
const RENEWAL_INTERVAL_MS = 24 * 60 * 60 * 1000;

export class AcmeManager {
  private context: tls.SecureContext | null = null;
  private readonly challenges = new Map<string, string>();
  private renewalTimer: NodeJS.Timeout | null = null;

  constructor(private readonly cfg: TlsConfig) {}

  async start(): Promise<void> {
    await fs.mkdir(this.cfg.cacheDir, { recursive: true });
    await this.ensureCert();

    this.renewalTimer = setInterval(() => {
      this.ensureCert().catch((err) => {
        console.error('[tls] renewal error:', err);
      });
    }, RENEWAL_INTERVAL_MS);
    this.renewalTimer.unref?.();
  }

  stop(): void {
    if (this.renewalTimer) {
      clearInterval(this.renewalTimer);
      this.renewalTimer = null;
    }
  }

  readonly sniCallback = (
    _servername: string,
    cb: (err: Error | null, ctx?: tls.SecureContext) => void,
  ): void => {
    if (this.context) cb(null, this.context);
    else cb(new Error('TLS context not ready'));
  };

  handleChallenge(req: http.IncomingMessage, res: http.ServerResponse): boolean {
    const url = req.url ?? '';
    const prefix = '/.well-known/acme-challenge/';
    if (!url.startsWith(prefix)) return false;

    const token = url.slice(prefix.length);
    const keyAuth = this.challenges.get(token);
    if (keyAuth) {
      res.writeHead(200, { 'Content-Type': 'text/plain' });
      res.end(keyAuth);
    } else {
      res.writeHead(404);
      res.end();
    }
    return true;
  }

  private async ensureCert(): Promise<void> {
    const certPath = path.join(this.cfg.cacheDir, 'cert.pem');
    const keyPath = path.join(this.cfg.cacheDir, 'key.pem');

    const existing = await tryLoad(certPath, keyPath);
    if (existing && isValidFor(existing.cert, RENEW_BEFORE_DAYS)) {
      this.context = tls.createSecureContext({ cert: existing.cert, key: existing.key });
      console.log(
        `[tls] loaded cached cert for ${this.cfg.domains.join(',')} (staging=${this.cfg.staging})`,
      );
      return;
    }

    console.log(
      `[tls] requesting new cert for ${this.cfg.domains.join(',')} (staging=${this.cfg.staging})`,
    );

    const accountKey = await this.loadOrCreateAccountKey();
    const client = new acme.Client({
      directoryUrl: this.cfg.staging
        ? acme.directory.letsencrypt.staging
        : acme.directory.letsencrypt.production,
      accountKey,
    });

    const [csrKey, csr] = await acme.crypto.createCsr({
      commonName: this.cfg.domains[0],
      altNames: this.cfg.domains,
    });

    const cert = await client.auto({
      csr,
      email: this.cfg.email,
      termsOfServiceAgreed: true,
      challengePriority: ['http-01'],
      challengeCreateFn: async (_authz, challenge, keyAuthorization) => {
        if (challenge.type !== 'http-01') return;
        this.challenges.set(challenge.token, keyAuthorization);
      },
      challengeRemoveFn: async (_authz, challenge) => {
        if (challenge.type !== 'http-01') return;
        this.challenges.delete(challenge.token);
      },
    });

    await fs.writeFile(certPath, cert);
    await fs.writeFile(keyPath, csrKey);
    this.context = tls.createSecureContext({ cert, key: csrKey });
    console.log(`[tls] issued cert for ${this.cfg.domains.join(',')}`);
  }

  private async loadOrCreateAccountKey(): Promise<Buffer> {
    const keyPath = path.join(this.cfg.cacheDir, 'account.key');
    try {
      return await fs.readFile(keyPath);
    } catch {
      const key = await acme.crypto.createPrivateKey();
      await fs.writeFile(keyPath, key);
      return key;
    }
  }
}

export function buildAcmeHttpHandler(
  acmeManager: AcmeManager,
  httpRedirect: boolean,
  httpsPort: number,
): http.RequestListener {
  const redirect = redirectHandler(httpsPort);
  return (req, res) => {
    if (acmeManager.handleChallenge(req, res)) return;
    if (httpRedirect) {
      redirect(req, res);
    } else {
      res.writeHead(404);
      res.end();
    }
  };
}

function redirectHandler(httpsPort: number): http.RequestListener {
  return (req, res) => {
    const rawHost = req.headers.host ?? '';
    const host = rawHost.split(':')[0];
    if (!host) {
      res.writeHead(400);
      res.end();
      return;
    }
    const authority = httpsPort === 443 ? host : `${host}:${httpsPort}`;
    const target = `https://${authority}${req.url ?? '/'}`;
    res.writeHead(301, { Location: target });
    res.end();
  };
}

async function tryLoad(
  certPath: string,
  keyPath: string,
): Promise<{ cert: Buffer; key: Buffer } | null> {
  try {
    const [cert, key] = await Promise.all([fs.readFile(certPath), fs.readFile(keyPath)]);
    return { cert, key };
  } catch {
    return null;
  }
}

function isValidFor(cert: Buffer, daysLeft: number): boolean {
  try {
    const x509 = new X509Certificate(cert);
    const validTo = new Date(x509.validTo).getTime();
    return validTo - Date.now() > daysLeft * 24 * 60 * 60 * 1000;
  } catch {
    return false;
  }
}

function envBool(key: string, def: boolean): boolean {
  const v = process.env[key];
  if (v == null) return def;
  return ['1', 'true', 'yes', 'on'].includes(v.toLowerCase());
}

function envU16(key: string, def: number): number {
  const v = process.env[key];
  if (!v) return def;
  const n = Number.parseInt(v, 10);
  return Number.isFinite(n) && n > 0 && n < 65536 ? n : def;
}

function parseCsv(v: string): string[] {
  return v
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean);
}
