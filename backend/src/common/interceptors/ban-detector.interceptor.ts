import {
  type CallHandler,
  type ExecutionContext,
  Injectable,
  type NestInterceptor,
} from '@nestjs/common';
import { AxiosError } from 'axios';
import { type Observable, catchError, throwError } from 'rxjs';
import { AuthService } from '../../auth/auth.service.js';
import { OAuthAppsService } from '../../oauth-apps/oauth-apps.service.js';

/**
 * Interceptor для детекции бана OAuth-аппки при любых SC API вызовах.
 * Если ловим CloudFront 403 — помечаем аппку как забаненную.
 */
@Injectable()
export class BanDetectorInterceptor implements NestInterceptor {
  constructor(
    private readonly oauthAppsService: OAuthAppsService,
    private readonly authService: AuthService,
  ) {}

  intercept(context: ExecutionContext, next: CallHandler): Observable<unknown> {
    return next.handle().pipe(
      catchError(async (error) => {
        if (error instanceof AxiosError && error.response) {
          const { status, data } = error.response;
          if (this.oauthAppsService.isSoundCloudAppBan(status, data)) {
            const request = context.switchToHttp().getRequest();
            const sessionId = request.headers?.['x-session-id'] ?? request.query?.session_id;

            if (sessionId) {
              const session = await this.authService.getSession(sessionId);
              if (session?.oauthAppId) {
                await this.oauthAppsService.markBanned(
                  session.oauthAppId,
                  `CloudFront 403 block during API call at ${new Date().toISOString()}`,
                );
              }
            }
          }
        }
        throw error;
      }),
    );
  }
}
