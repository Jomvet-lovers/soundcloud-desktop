import {
  type CanActivate,
  type ExecutionContext,
  Injectable,
  UnauthorizedException,
} from '@nestjs/common';
import { AuthService } from '../../auth/auth.service.js';
import { isValidUuid } from '../uuid.js';

@Injectable()
export class AuthGuard implements CanActivate {
  constructor(private readonly authService: AuthService) {}

  async canActivate(context: ExecutionContext): Promise<boolean> {
    const request = context.switchToHttp().getRequest();
    const sessionId = request.headers['x-session-id'] ?? request.query?.session_id;

    if (!sessionId || !isValidUuid(sessionId)) {
      throw new UnauthorizedException('Missing or malformed x-session-id header');
    }

    request.accessToken = await this.authService.getValidAccessToken(sessionId);
    request.sessionId = sessionId;

    const session = await this.authService.getSession(sessionId);
    if (!session?.soundcloudUserId) {
      throw new UnauthorizedException(
        'Session missing SoundCloud user info, please re-authenticate',
      );
    }

    return true;
  }
}
