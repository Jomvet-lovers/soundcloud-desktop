import { webcrypto, X509Certificate } from 'node:crypto';
import { promises as fs } from 'node:fs';
import * as http from 'node:http';
import * as path from 'node:path';
import * as tls from 'node:tls';
import * as x509 from '@peculiar/x509';
import * as acme from 'acme-client';

x509.cryptoProvider.set(webcrypto as unknown as Crypto);

const ACME_TLS_ALPN = 'acme-tls/1';
// RFC 8737: id-pe-acmeIdentifier
const ACME_IDENTIFIER_OID = '1.3.6.1.5.5.7.1.31';
const RENEW_BEFORE_DAYS = 30;
const RENEWAL_INTERVAL_MS = 24 * 60 * 60 * 1000;

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
    throw new Error('TLS_ENABLED=true but DOMAINS is empty (expected comma-separated domain list)');
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

export class AcmeManager {
  private context: tls.SecureContext | null = null;
  private readonly challengeContexts = new Map<string, tls.SecureContext>();
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

  // SNI: при активном challenge для запрошенного servername — отдаём
  // self-signed cert с acmeIdentifier extension (поверх него LE будет valid'ить
  // по TLS-ALPN-01). Иначе — обычный production cert.
  readonly sniCallback = (
    servername: string,
    cb: (err: Error | null, ctx?: tls.SecureContext) => void,
  ): void => {
    const challenge = this.challengeContexts.get(servername);
    if (challenge) {
      cb(null, challenge);
      return;
    }
    if (this.context) {
      cb(null, this.context);
      return;
    }
    cb(new Error('TLS context not ready'));
  };

  // Без ALPNCallback во время challenge клиент с обычным http/1.1 ALPN получил
  // бы challenge cert (т.к. SNICallback его уже отдал) и сломался по cert-mismatch.
  // С ALPNCallback таких клиентов мы reject'им на уровне ALPN — они увидят
  // чистый ALPN-error и сделают retry; через ~10–30s challenge закроется.
  readonly alpnCallback = (
    info: { servername: string; protocols: string[] },
  ): string | undefined => {
    if (this.challengeContexts.has(info.servername)) {
      return info.protocols.includes(ACME_TLS_ALPN) ? ACME_TLS_ALPN : undefined;
    }
    if (info.protocols.includes('http/1.1')) return 'http/1.1';
    if (info.protocols.length === 0) return 'http/1.1';
    return undefined;
  };

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
      `[tls] requesting new cert for ${this.cfg.domains.join(',')} via tls-alpn-01 (staging=${this.cfg.staging})`,
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
      challengePriority: ['tls-alpn-01'],
      challengeCreateFn: async (authz, challenge, keyAuthorization) => {
        if ((challenge.type as string) !== 'tls-alpn-01') return;
        const domain = authz.identifier.value;
        const ctx = await buildAlpnChallengeContext(domain, keyAuthorization);
        this.challengeContexts.set(domain, ctx);
        console.log(`[tls] challenge SET ${domain}`);
      },
      challengeRemoveFn: async (authz, challenge) => {
        if ((challenge.type as string) !== 'tls-alpn-01') return;
        const domain = authz.identifier.value;
        this.challengeContexts.delete(domain);
        console.log(`[tls] challenge CLR ${domain}`);
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

async function buildAlpnChallengeContext(
  domain: string,
  keyAuthorization: string,
): Promise<tls.SecureContext> {
  const keys = await webcrypto.subtle.generateKey(
    { name: 'ECDSA', namedCurve: 'P-256' },
    true,
    ['sign', 'verify'],
  );

  const digest = new Uint8Array(
    await webcrypto.subtle.digest('SHA-256', new TextEncoder().encode(keyAuthorization)),
  );

  // RFC 8737: extension value — DER-encoded OCTET STRING (32 bytes).
  const extValue = new Uint8Array(34);
  extValue[0] = 0x04;
  extValue[1] = 0x20;
  extValue.set(digest, 2);

  const cert = await x509.X509CertificateGenerator.createSelfSigned({
    serialNumber: '01',
    name: `CN=${domain}`,
    notBefore: new Date(Date.now() - 60_000),
    notAfter: new Date(Date.now() + 24 * 60 * 60 * 1000),
    signingAlgorithm: { name: 'ECDSA', hash: 'SHA-256' },
    keys: keys as unknown as CryptoKeyPair,
    extensions: [
      new x509.SubjectAlternativeNameExtension([{ type: 'dns', value: domain }]),
      new x509.Extension(ACME_IDENTIFIER_OID, true, extValue),
    ],
  });

  const pkcs8 = await webcrypto.subtle.exportKey('pkcs8', keys.privateKey);
  return tls.createSecureContext({
    cert: cert.toString('pem'),
    key: pkcs8ToPem(new Uint8Array(pkcs8)),
  });
}

export function buildHttpRedirectHandler(httpsPort: number): http.RequestListener {
  return (req, res) => {
    const rawHost = req.headers.host ?? '';
    const host = rawHost.split(':')[0];
    if (!host) {
      res.writeHead(400);
      res.end();
      return;
    }
    const authority = httpsPort === 443 ? host : `${host}:${httpsPort}`;
    res.writeHead(301, { Location: `https://${authority}${req.url ?? '/'}` });
    res.end();
  };
}

function pkcs8ToPem(der: Uint8Array): string {
  const b64 = Buffer.from(der).toString('base64');
  const lines = b64.match(/.{1,64}/g)?.join('\n') ?? b64;
  return `-----BEGIN PRIVATE KEY-----\n${lines}\n-----END PRIVATE KEY-----\n`;
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
    const x = new X509Certificate(cert);
    const validTo = new Date(x.validTo).getTime();
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
