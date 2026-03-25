import { createHash, randomBytes } from 'node:crypto';
import { BadRequestException, Injectable, Logger, UnauthorizedException } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { InjectRepository } from '@nestjs/typeorm';
import { AxiosError } from 'axios';
import { Repository } from 'typeorm';
import { OAuthAppsService } from '../oauth-apps/oauth-apps.service.js';
import { type OAuthCredentials, SoundcloudService } from '../soundcloud/soundcloud.service.js';
import { ScMe } from '../soundcloud/soundcloud.types.js';
import { Session } from './entities/session.entity.js';

@Injectable()
export class AuthService {
  private readonly logger = new Logger(AuthService.name);

  constructor(
    @InjectRepository(Session)
    private readonly sessionRepo: Repository<Session>,
    private readonly soundcloudService: SoundcloudService,
    private readonly oauthAppsService: OAuthAppsService,
    private readonly configService: ConfigService,
  ) {}

  async initiateLogin(): Promise<{ url: string; sessionId: string }> {
    const codeVerifier = randomBytes(32).toString('base64url');
    const codeChallenge = createHash('sha256').update(codeVerifier).digest('base64url');
    const state = randomBytes(16).toString('hex');

    // Рандомно выбираем OAuth-аппку
    const app = this.oauthAppsService.pickRandomApp();
    this.logger.log(`Login initiated with app "${app.name}" (${app.id})`);

    const session = this.sessionRepo.create({
      codeVerifier,
      state,
      accessToken: '',
      refreshToken: '',
      expiresAt: new Date(),
      scope: '',
      oauthAppId: app.id,
    });
    await this.sessionRepo.save(session);

    const authBaseUrl = this.soundcloudService.scAuthBaseUrl;

    const params = new URLSearchParams({
      client_id: app.clientId,
      redirect_uri: app.redirectUri,
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
  ): Promise<{ session: Session; success: boolean; error?: string }> {
    const session = await this.sessionRepo.findOne({ where: { state } });
    if (!session) {
      throw new BadRequestException('Invalid state parameter');
    }

    if (!session.codeVerifier) {
      throw new BadRequestException('No code verifier found for this session');
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

      try {
        const me = await this.soundcloudService.apiGet<ScMe>('/me', session.accessToken);
        session.soundcloudUserId = me.urn;
        session.username = me.username;
      } catch {}

      await this.sessionRepo.save(session);
      return { session, success: true };
    } catch (error: any) {
      // Проверяем, не бан ли это
      await this.checkAndHandleBan(error, session.oauthAppId);

      return {
        session,
        success: false,
        error:
          error?.response?.data?.error_description || error?.message || 'Token exchange failed',
      };
    }
  }

  async refreshSession(sessionId: string): Promise<Session> {
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
      session.refreshToken = tokenResponse.refresh_token;
      session.expiresAt = new Date(Date.now() + tokenResponse.expires_in * 1000);

      await this.sessionRepo.save(session);
      return session;
    } catch (error: any) {
      // Проверяем, не бан ли это
      const isBan = await this.checkAndHandleBan(error, session.oauthAppId);

      if (!isBan) {
        await this.sessionRepo.remove(session);
        throw new UnauthorizedException(
          'Refresh token expired or invalid. Please re-authenticate.',
        );
      }

      // Если бан — не удаляем сессию, юзер может переавторизоваться
      throw new UnauthorizedException('SoundCloud app banned. Please re-authenticate.');
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

    if (session.expiresAt <= new Date()) {
      session = await this.refreshSession(sessionId);
    }

    return session.accessToken;
  }

  /**
   * Проверяет, является ли ошибка баном аппки.
   * Если да — помечает аппку как забаненную.
   * @returns true если это был бан
   */
  private async checkAndHandleBan(error: unknown, oauthAppId: string | null): Promise<boolean> {
    if (!oauthAppId) return false;

    if (error instanceof AxiosError && error.response) {
      const { status, data } = error.response;
      if (this.oauthAppsService.isSoundCloudAppBan(status, data)) {
        await this.oauthAppsService.markBanned(
          oauthAppId,
          `CloudFront 403 block at ${new Date().toISOString()}`,
        );
        return true;
      }
    }

    return false;
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

    // Fallback: env credentials (для старых сессий без oauthAppId)
    return {
      clientId: this.configService.get<string>('soundcloud.clientId') || '',
      clientSecret: this.configService.get<string>('soundcloud.clientSecret') || '',
      redirectUri: this.configService.get<string>('soundcloud.redirectUri') || '',
    };
  }
}
