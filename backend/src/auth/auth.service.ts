import { createHash, randomBytes } from 'node:crypto';
import { Injectable, Logger, NotFoundException, UnauthorizedException } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { InjectRepository } from '@nestjs/typeorm';
import { LessThan, Repository } from 'typeorm';
import { OAuthAppsService } from '../oauth-apps/oauth-apps.service.js';
import { type OAuthCredentials, SoundcloudService } from '../soundcloud/soundcloud.service.js';
import { ScMe } from '../soundcloud/soundcloud.types.js';
import { REFRESH_BUFFER_MS } from './auth.constants.js';
import { LoginRequest } from './entities/login-request.entity.js';
import { Session } from './entities/session.entity.js';

const LOGIN_REQUEST_TTL_MS = 15 * 60_000;

@Injectable()
export class AuthService {
  private readonly logger = new Logger(AuthService.name);
  private readonly refreshInFlight = new Map<string, Promise<Session>>();

  constructor(
    @InjectRepository(Session)
    private readonly sessionRepo: Repository<Session>,
    @InjectRepository(LoginRequest)
    private readonly loginRequestRepo: Repository<LoginRequest>,
    private readonly soundcloudService: SoundcloudService,
    private readonly oauthAppsService: OAuthAppsService,
    private readonly configService: ConfigService,
  ) {}

  async initiateLogin(
    existingSessionId?: string,
  ): Promise<{ url: string; loginRequestId: string }> {
    void this.cleanupExpiredLoginRequests();

    const codeVerifier = randomBytes(32).toString('base64url');
    const codeChallenge = createHash('sha256').update(codeVerifier).digest('base64url');
    const state = randomBytes(32).toString('hex');

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

    let targetSessionId: string | null = null;
    if (existingSessionId) {
      const existing = await this.sessionRepo.findOne({ where: { id: existingSessionId } });
      if (existing) {
        targetSessionId = existing.id;
        this.logger.log(`Re-auth flow for existing session ${existing.id}`);
      } else {
        this.logger.warn(
          `Re-auth requested for unknown session ${existingSessionId}, will create new`,
        );
      }
    }

    const loginRequest = this.loginRequestRepo.create({
      state,
      codeVerifier,
      oauthAppId,
      targetSessionId,
      status: 'pending',
      expiresAt: new Date(Date.now() + LOGIN_REQUEST_TTL_MS),
    });
    await this.loginRequestRepo.save(loginRequest);

    const params = new URLSearchParams({
      client_id: creds.clientId,
      redirect_uri: creds.redirectUri,
      response_type: 'code',
      code_challenge: codeChallenge,
      code_challenge_method: 'S256',
      state,
    });

    return {
      url: `${this.soundcloudService.scAuthBaseUrl}/authorize?${params.toString()}`,
      loginRequestId: loginRequest.id,
    };
  }

  async handleCallback(
    code: string,
    state: string,
  ): Promise<{ success: boolean; sessionId?: string; username?: string; error?: string }> {
    const loginRequest = await this.loginRequestRepo.findOne({ where: { state } });

    if (!loginRequest) {
      this.logger.warn(`Callback received with unknown state: ${state.slice(0, 12)}…`);
      return {
        success: false,
        error: 'This login link is invalid or already used. Please try logging in again.',
      };
    }

    if (loginRequest.status !== 'pending') {
      this.logger.warn(
        `Callback for login request ${loginRequest.id} in non-pending status: ${loginRequest.status}`,
      );
      return {
        success: loginRequest.status === 'completed',
        error:
          loginRequest.status === 'completed'
            ? undefined
            : loginRequest.error || 'This login link was already used.',
      };
    }

    if (loginRequest.expiresAt.getTime() < Date.now()) {
      loginRequest.status = 'failed';
      loginRequest.error = 'Login request expired';
      await this.loginRequestRepo.save(loginRequest);
      return {
        success: false,
        error: 'Login link expired. Please try logging in again.',
      };
    }

    const creds = await this.getCredentialsForApp(loginRequest.oauthAppId);

    let tokenResponse: Awaited<ReturnType<typeof this.soundcloudService.exchangeCodeForToken>>;
    try {
      tokenResponse = await this.soundcloudService.exchangeCodeForToken(
        code,
        loginRequest.codeVerifier,
        creds,
      );
    } catch (err: any) {
      this.logger.error(
        `Token exchange failed for login request ${loginRequest.id}: status=${err?.response?.status} message=${err?.message}`,
      );
      loginRequest.status = 'failed';
      loginRequest.error =
        err?.response?.data?.error_description || err?.message || 'Token exchange failed';
      await this.loginRequestRepo.save(loginRequest);
      return { success: false, error: loginRequest.error || 'Token exchange failed' };
    }

    const me = await this.fetchScMeWithRetries(tokenResponse.access_token);
    if (!me?.urn) {
      loginRequest.status = 'failed';
      loginRequest.error = 'Failed to fetch SoundCloud user info';
      await this.loginRequestRepo.save(loginRequest);
      return {
        success: false,
        error: 'Failed to fetch SoundCloud user info. Please try again.',
      };
    }

    let session: Session | null = null;
    if (loginRequest.targetSessionId) {
      session = await this.sessionRepo.findOne({ where: { id: loginRequest.targetSessionId } });
    }

    if (!session) {
      session = this.sessionRepo.create({
        accessToken: tokenResponse.access_token,
        refreshToken: tokenResponse.refresh_token,
        expiresAt: new Date(Date.now() + tokenResponse.expires_in * 1000),
        scope: tokenResponse.scope || '',
        soundcloudUserId: me.urn,
        username: me.username,
        oauthAppId: loginRequest.oauthAppId,
      });
    } else {
      session.accessToken = tokenResponse.access_token;
      session.refreshToken = tokenResponse.refresh_token;
      session.expiresAt = new Date(Date.now() + tokenResponse.expires_in * 1000);
      session.scope = tokenResponse.scope || '';
      session.soundcloudUserId = me.urn;
      session.username = me.username;
      if (loginRequest.oauthAppId) session.oauthAppId = loginRequest.oauthAppId;
    }
    await this.sessionRepo.save(session);

    loginRequest.status = 'completed';
    loginRequest.resultSessionId = session.id;
    await this.loginRequestRepo.save(loginRequest);

    this.logger.log(
      `Login completed: request=${loginRequest.id} session=${session.id} user=${me.username}`,
    );

    return { success: true, sessionId: session.id, username: me.username };
  }

  async getLoginRequestStatus(loginRequestId: string): Promise<{
    status: 'pending' | 'completed' | 'failed' | 'expired';
    sessionId?: string;
    error?: string;
  }> {
    const lr = await this.loginRequestRepo.findOne({ where: { id: loginRequestId } });
    if (!lr) return { status: 'expired', error: 'Unknown login request' };
    if (lr.status === 'pending' && lr.expiresAt.getTime() < Date.now()) {
      return { status: 'expired', error: 'Login request expired' };
    }
    return {
      status: lr.status,
      sessionId: lr.resultSessionId ?? undefined,
      error: lr.error ?? undefined,
    };
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
      // НЕ удаляем сессию: refresh может временно отказать (network/5xx).
      // Пусть остаётся, у юзера откроется ReAuthOverlay → re-auth попадёт в ту же запись.
      throw new UnauthorizedException('Refresh token expired or invalid. Please re-authenticate.');
    }
  }

  async logout(sessionId: string): Promise<void> {
    const session = await this.sessionRepo.findOne({ where: { id: sessionId } });
    if (!session) return;

    if (session.accessToken) {
      try {
        await this.soundcloudService.signOut(session.accessToken);
      } catch (err: any) {
        this.logger.warn(`signOut failed for session ${sessionId}: ${err?.message}`);
      }
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

  async getCredentialsForApp(oauthAppId?: string | null): Promise<OAuthCredentials> {
    if (oauthAppId) {
      const app = await this.oauthAppsService.getById(oauthAppId);
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

  private async getSessionCredentials(session: Session): Promise<OAuthCredentials> {
    return this.getCredentialsForApp(session.oauthAppId);
  }

  private getEnvCredentials(): OAuthCredentials {
    return {
      clientId: this.configService.get<string>('soundcloud.clientId') || '',
      clientSecret: this.configService.get<string>('soundcloud.clientSecret') || '',
      redirectUri: this.configService.get<string>('soundcloud.redirectUri') || '',
    };
  }

  private async fetchScMeWithRetries(accessToken: string): Promise<ScMe | null> {
    let lastErr: any;
    for (let attempt = 0; attempt < 3; attempt++) {
      try {
        return await this.soundcloudService.apiGet<ScMe>('/me', accessToken);
      } catch (err) {
        lastErr = err;
        if (attempt < 2) {
          await new Promise((r) => setTimeout(r, 500 * (attempt + 1)));
        }
      }
    }
    this.logger.error(
      `Failed to fetch /me after retries: status=${lastErr?.response?.status} body=${JSON.stringify(lastErr?.response?.data)} message=${lastErr?.message}`,
    );
    return null;
  }

  private async cleanupExpiredLoginRequests(): Promise<void> {
    try {
      await this.loginRequestRepo.delete({ expiresAt: LessThan(new Date()) });
    } catch (err: any) {
      this.logger.warn(`Failed to cleanup expired login requests: ${err?.message}`);
    }
  }
}
