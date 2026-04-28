import { createHash, randomBytes } from 'node:crypto';
import {
  Injectable,
  Logger,
  NotFoundException,
  type OnModuleInit,
  UnauthorizedException,
} from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { InjectRepository } from '@nestjs/typeorm';
import { LessThan, Repository } from 'typeorm';
import { OAuthAppsService } from '../oauth-apps/oauth-apps.service.js';
import { isValidUuid } from '../common/uuid.js';
import { type OAuthCredentials, SoundcloudService } from '../soundcloud/soundcloud.service.js';
import { ScMe } from '../soundcloud/soundcloud.types.js';
import { REFRESH_BUFFER_MS } from './auth.constants.js';
import { LoginRequest } from './entities/login-request.entity.js';
import { Session } from './entities/session.entity.js';

const LOGIN_REQUEST_TTL_MS = 15 * 60_000;

@Injectable()
export class AuthService implements OnModuleInit {
  private readonly logger = new Logger(AuthService.name);
  private readonly refreshInFlight = new Map<string, Promise<Session>>();
  private cleanupTimer?: ReturnType<typeof setInterval>;

  onModuleInit() {
    this.cleanupTimer = setInterval(() => {
      void this.cleanupExpiredLoginRequests();
    }, 60_000);
  }

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

    this.logger.log(
      `LoginRequest created id=${loginRequest.id} state=${state.slice(0, 8)}… target=${targetSessionId ?? 'new'} expiresAt=${loginRequest.expiresAt.toISOString()}`,
    );

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

  /**
   * Принимает callback от SoundCloud и **сразу** возвращает информацию для рендера
   * страницы. Реальный обмен кода + /me + сохранение происходит в фоне — страница
   * polling'ует /auth/login/status и видит прогресс. Это убирает зависание HTML
   * на 2-5 секунд и даёт защиту от дубликат-вызовов callback'а (preview, retry).
   */
  async handleCallback(
    code: string,
    state: string,
  ): Promise<{
    /** Если null — state не найден, рендерим error-страницу. */
    loginRequestId: string | null;
    /** Известный финальный исход (если уже completed/failed). Для свежезахваченных — pending. */
    initialStatus: 'pending' | 'completed' | 'failed';
    error?: string;
    sessionId?: string;
    username?: string;
  }> {
    this.logger.log(
      `Callback received: state=${state?.slice(0, 8)}… code=${code?.slice(0, 8)}…`,
    );

    // Атомарно захватываем pending → processing. Только один параллельный callback пройдёт.
    const claim = await this.loginRequestRepo
      .createQueryBuilder()
      .update(LoginRequest)
      .set({ status: 'processing' })
      .where('state = :state AND status = :status', { state, status: 'pending' })
      .execute();

    if (claim.affected) {
      // Захвачено нами — запускаем фоновую обработку, страница увидит результат через polling.
      const captured = await this.loginRequestRepo.findOne({ where: { state } });
      if (!captured) {
        this.logger.error(`Login request disappeared after claim, state=${state?.slice(0, 8)}…`);
        return {
          loginRequestId: null,
          initialStatus: 'failed',
          error: 'Internal error. Please try again.',
        };
      }
      void this.runCallbackBackground(captured.id, code);
      return { loginRequestId: captured.id, initialStatus: 'pending' };
    }

    // Не захватили — либо state не существует, либо уже processed/processing.
    const existing = await this.loginRequestRepo.findOne({ where: { state } });
    if (!existing) {
      this.logger.warn(`Callback state not found: ${state?.slice(0, 8)}…`);
      return {
        loginRequestId: null,
        initialStatus: 'failed',
        error: 'This login link is invalid or already used. Please try logging in again.',
      };
    }
    if (existing.status === 'completed') {
      return {
        loginRequestId: existing.id,
        initialStatus: 'completed',
        sessionId: existing.resultSessionId ?? undefined,
      };
    }
    if (existing.status === 'processing') {
      // Двойной callback на ещё не завершённый запрос: пусть тоже polling'ует.
      return { loginRequestId: existing.id, initialStatus: 'pending' };
    }
    return {
      loginRequestId: existing.id,
      initialStatus: 'failed',
      error: existing.error || 'This login link was already used.',
    };
  }

  /**
   * Фоновая обработка захваченного LoginRequest. НЕ throw'ит — все ошибки
   * сохраняются в БД (status=failed + error). Страница увидит их через polling.
   */
  private async runCallbackBackground(loginRequestId: string, code: string): Promise<void> {
    try {
      const lr = await this.loginRequestRepo.findOne({ where: { id: loginRequestId } });
      if (!lr) {
        this.logger.error(`Background: loginRequest ${loginRequestId} disappeared`);
        return;
      }

      if (lr.expiresAt.getTime() < Date.now()) {
        await this.markRequestFailed(loginRequestId, 'Login request expired');
        return;
      }

      const creds = await this.getCredentialsForApp(lr.oauthAppId);

      let tokenResponse: Awaited<ReturnType<typeof this.soundcloudService.exchangeCodeForToken>>;
      try {
        tokenResponse = await this.soundcloudService.exchangeCodeForToken(
          code,
          lr.codeVerifier,
          creds,
        );
      } catch (err: any) {
        const msg =
          err?.response?.data?.error_description || err?.message || 'Token exchange failed';
        this.logger.error(
          `Token exchange failed for ${loginRequestId}: status=${err?.response?.status} ${msg}`,
        );
        await this.markRequestFailed(loginRequestId, msg);
        return;
      }

      const me = await this.fetchScMeWithRetries(tokenResponse.access_token);
      if (!me?.urn) {
        await this.markRequestFailed(loginRequestId, 'Failed to fetch SoundCloud user info');
        return;
      }

      let session: Session | null = null;
      if (lr.targetSessionId) {
        session = await this.sessionRepo.findOne({ where: { id: lr.targetSessionId } });
      }

      if (!session) {
        session = this.sessionRepo.create({
          accessToken: tokenResponse.access_token,
          refreshToken: tokenResponse.refresh_token,
          expiresAt: new Date(Date.now() + tokenResponse.expires_in * 1000),
          scope: tokenResponse.scope || '',
          soundcloudUserId: me.urn,
          username: me.username,
          oauthAppId: lr.oauthAppId,
        });
      } else {
        session.accessToken = tokenResponse.access_token;
        session.refreshToken = tokenResponse.refresh_token;
        session.expiresAt = new Date(Date.now() + tokenResponse.expires_in * 1000);
        session.scope = tokenResponse.scope || '';
        session.soundcloudUserId = me.urn;
        session.username = me.username;
        if (lr.oauthAppId) session.oauthAppId = lr.oauthAppId;
      }
      await this.sessionRepo.save(session);

      lr.status = 'completed';
      lr.resultSessionId = session.id;
      await this.loginRequestRepo.save(lr);

      this.logger.log(
        `Login completed: request=${loginRequestId} session=${session.id} user=${me.username}`,
      );
    } catch (err: any) {
      this.logger.error(
        `Unexpected error in callback background for ${loginRequestId}: ${err?.message}`,
        err?.stack,
      );
      await this.markRequestFailed(loginRequestId, 'Internal error during authentication');
    }
  }

  private async markRequestFailed(id: string, error: string): Promise<void> {
    try {
      await this.loginRequestRepo.update({ id }, { status: 'failed', error });
    } catch (err: any) {
      this.logger.error(`Failed to mark login request ${id} as failed: ${err?.message}`);
    }
  }

  async getLoginRequestStatus(loginRequestId: string): Promise<{
    status: 'pending' | 'completed' | 'failed' | 'expired';
    sessionId?: string;
    error?: string;
  }> {
    const lr = await this.loginRequestRepo.findOne({ where: { id: loginRequestId } });
    if (!lr) return { status: 'expired', error: 'Unknown login request' };
    if (
      (lr.status === 'pending' || lr.status === 'processing') &&
      lr.expiresAt.getTime() < Date.now()
    ) {
      return { status: 'expired', error: 'Login request expired' };
    }
    // 'processing' для клиента эквивалентен 'pending' — пусть продолжает polling.
    const status = lr.status === 'processing' ? 'pending' : lr.status;
    return {
      status,
      sessionId: lr.resultSessionId ?? undefined,
      error: lr.error ?? undefined,
    };
  }

  async refreshSession(sessionId: string): Promise<Session> {
    if (!isValidUuid(sessionId)) {
      throw new UnauthorizedException('Malformed session id');
    }
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
    if (!isValidUuid(sessionId)) return;
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
    if (!isValidUuid(sessionId)) return null;
    return this.sessionRepo.findOne({ where: { id: sessionId } });
  }

  async getValidAccessToken(sessionId: string): Promise<string> {
    if (!isValidUuid(sessionId)) {
      throw new UnauthorizedException('Malformed session id');
    }
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
