import { createHash, randomBytes } from 'node:crypto';
import {
  Injectable,
  Logger,
  NotFoundException,
  UnauthorizedException,
} from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { InjectRepository } from '@nestjs/typeorm';
import { Repository } from 'typeorm';
import { OAuthAppsService } from '../oauth-apps/oauth-apps.service.js';
import { type OAuthCredentials, SoundcloudService } from '../soundcloud/soundcloud.service.js';
import { ScMe } from '../soundcloud/soundcloud.types.js';
import { REFRESH_BUFFER_MS } from './auth.constants.js';
import { Session } from './entities/session.entity.js';

@Injectable()
export class AuthService {
  private readonly logger = new Logger(AuthService.name);
  /** Дедупликация параллельных refresh'ей для одной сессии */
  private readonly refreshInFlight = new Map<string, Promise<Session>>();

  constructor(
    @InjectRepository(Session)
    private readonly sessionRepo: Repository<Session>,
    private readonly soundcloudService: SoundcloudService,
    private readonly oauthAppsService: OAuthAppsService,
    private readonly configService: ConfigService,
  ) {}

  async initiateLogin(existingSessionId?: string): Promise<{ url: string; sessionId: string }> {
    const codeVerifier = randomBytes(32).toString('base64url');
    const codeChallenge = createHash('sha256').update(codeVerifier).digest('base64url');
    const state = randomBytes(16).toString('hex');

    let oauthAppId: string | undefined;
    let creds: OAuthCredentials;
    try {
      const app = this.oauthAppsService.pickRandomApp();
      oauthAppId = app.id;
      creds = {
        clientId: app.clientId,
        clientSecret: app.clientSecret,
        redirectUri: app.redirectUri,
      };
      this.logger.log(`Login initiated with app "${app.name}" (${app.id})`);
    } catch {
      creds = this.getEnvCredentials();
      if (!creds.clientId || !creds.clientSecret) {
        throw new NotFoundException(
          'No active OAuth apps available and env fallback is not configured',
        );
      }
      this.logger.warn('No active OAuth apps available, using env OAuth fallback');
    }

    let session: Session | null = null;
    if (existingSessionId) {
      session = await this.sessionRepo.findOne({ where: { id: existingSessionId } });
    }

    if (session) {
      session.codeVerifier = codeVerifier;
      session.state = state;
      session.oauthAppId = oauthAppId;
      this.logger.log(`Reusing existing session ${session.id} for re-auth`);
    } else {
      session = this.sessionRepo.create({
        codeVerifier,
        state,
        accessToken: '',
        refreshToken: '',
        expiresAt: new Date(),
        scope: '',
        oauthAppId,
      });
    }
    await this.sessionRepo.save(session);

    const authBaseUrl = this.soundcloudService.scAuthBaseUrl;

    const params = new URLSearchParams({
      client_id: creds.clientId,
      redirect_uri: creds.redirectUri,
      response_type: 'code',
      code_challenge: codeChallenge,
      code_challenge_method: 'S256',
      state,
    });

    return {
      url: `${authBaseUrl}/authorize?${params.toString()}`,
      sessionId: session.id,
    };
  }

  async handleCallback(
    code: string,
    state: string,
  ): Promise<{ session: Session | null; success: boolean; error?: string }> {
    const session = await this.sessionRepo.findOne({ where: { state } });
    if (!session) {
      this.logger.warn(`Callback received with unknown state: ${state}`);
      return {
        session: null,
        success: false,
        error: 'Session expired or not found. Please try logging in again.',
      };
    }

    if (!session.codeVerifier) {
      this.logger.warn(`Callback for session ${session.id} has no code verifier`);
      return {
        session,
        success: false,
        error: 'Login session is invalid. Please try logging in again.',
      };
    }

    const creds = await this.getSessionCredentials(session);

    try {
      const tokenResponse = await this.soundcloudService.exchangeCodeForToken(
        code,
        session.codeVerifier,
        creds,
      );

      session.accessToken = tokenResponse.access_token;
      session.refreshToken = tokenResponse.refresh_token;
      session.expiresAt = new Date(Date.now() + tokenResponse.expires_in * 1000);
      session.scope = tokenResponse.scope || '';
      session.codeVerifier = '';
      session.state = '';

      let me: ScMe | null = null;
      let lastErr: any;
      for (let attempt = 0; attempt < 3; attempt++) {
        try {
          me = await this.soundcloudService.apiGet<ScMe>('/me', session.accessToken);
          break;
        } catch (err) {
          lastErr = err;
          if (attempt < 2) {
            await new Promise((r) => setTimeout(r, 500 * (attempt + 1)));
          }
        }
      }

      if (!me?.urn) {
        this.logger.error(
          `Failed to fetch /me after retries for session ${session.id}: status=${lastErr?.response?.status} body=${JSON.stringify(lastErr?.response?.data)} message=${lastErr?.message}`,
        );
        await this.sessionRepo.remove(session);
        return {
          session,
          success: false,
          error: 'Failed to fetch SoundCloud user info. Please try again.',
        };
      }

      session.soundcloudUserId = me.urn;
      session.username = me.username;

      await this.sessionRepo.save(session);
      return { session, success: true };
    } catch (error: any) {
      return {
        session,
        success: false,
        error:
          error?.response?.data?.error_description || error?.message || 'Token exchange failed',
      };
    }
  }

  async refreshSession(sessionId: string): Promise<Session> {
    const existing = this.refreshInFlight.get(sessionId);
    if (existing) return existing;

    const promise = this.doRefresh(sessionId).finally(() => {
      this.refreshInFlight.delete(sessionId);
    });
    this.refreshInFlight.set(sessionId, promise);
    return promise;
  }

  private async doRefresh(sessionId: string): Promise<Session> {
    const session = await this.sessionRepo.findOne({ where: { id: sessionId } });
    if (!session) {
      throw new UnauthorizedException('Session not found');
    }

    if (!session.refreshToken) {
      throw new UnauthorizedException('No refresh token available');
    }

    const creds = await this.getSessionCredentials(session);

    try {
      const tokenResponse = await this.soundcloudService.refreshAccessToken(
        session.refreshToken,
        creds,
      );

      session.accessToken = tokenResponse.access_token;
      if (tokenResponse.refresh_token) {
        session.refreshToken = tokenResponse.refresh_token;
      }
      session.expiresAt = new Date(Date.now() + tokenResponse.expires_in * 1000);
      if (tokenResponse.scope) session.scope = tokenResponse.scope;

      await this.sessionRepo.save(session);
      this.logger.log(
        `Session ${sessionId} refreshed, expires at ${session.expiresAt.toISOString()}`,
      );
      return session;
    } catch (error: any) {
      this.logger.warn(
        `Refresh failed for session ${sessionId}: ${error?.response?.data?.error_description || error?.message}`,
      );
      await this.sessionRepo.remove(session);
      throw new UnauthorizedException('Refresh token expired or invalid. Please re-authenticate.');
    }
  }

  async logout(sessionId: string): Promise<void> {
    const session = await this.sessionRepo.findOne({ where: { id: sessionId } });
    if (!session) return;

    if (session.accessToken) {
      await this.soundcloudService.signOut(session.accessToken);
    }

    await this.sessionRepo.remove(session);
  }

  async getSession(sessionId: string): Promise<Session | null> {
    return this.sessionRepo.findOne({ where: { id: sessionId } });
  }

  async getValidAccessToken(sessionId: string): Promise<string> {
    let session = await this.sessionRepo.findOne({ where: { id: sessionId } });
    if (!session) {
      throw new UnauthorizedException('Session not found');
    }

    const expiresAtMs = new Date(session.expiresAt).getTime();
    if (expiresAtMs - Date.now() <= REFRESH_BUFFER_MS) {
      session = await this.refreshSession(sessionId);
    }

    return session.accessToken;
  }

  /** Получить OAuth credentials для сессии (из привязанной аппки или fallback из env) */
  private async getSessionCredentials(session: Session): Promise<OAuthCredentials> {
    if (session.oauthAppId) {
      const app = await this.oauthAppsService.getById(session.oauthAppId);
      if (app) {
        return {
          clientId: app.clientId,
          clientSecret: app.clientSecret,
          redirectUri: app.redirectUri,
        };
      }
    }

    return this.getEnvCredentials();
  }

  private getEnvCredentials(): OAuthCredentials {
    return {
      clientId: this.configService.get<string>('soundcloud.clientId') || '',
      clientSecret: this.configService.get<string>('soundcloud.clientSecret') || '',
      redirectUri: this.configService.get<string>('soundcloud.redirectUri') || '',
    };
  }
}
