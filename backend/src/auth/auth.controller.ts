import {
  Controller,
  Get,
  Header,
  Headers,
  HttpCode,
  Logger,
  Post,
  Query,
} from '@nestjs/common';
import { ApiHeader, ApiOkResponse, ApiOperation, ApiQuery, ApiTags } from '@nestjs/swagger';
import { AuthService } from './auth.service.js';
import { renderCallbackPage } from './callback-page.js';
import {
  LoginResponseDto,
  LoginStatusResponseDto,
  LogoutResponseDto,
  RefreshResponseDto,
  SessionResponseDto,
} from './dto/auth-response.dto.js';

@ApiTags('auth')
@Controller('auth')
export class AuthController {
  private readonly logger = new Logger(AuthController.name);

  constructor(private readonly authService: AuthService) {}

  @Get('login')
  @ApiOperation({ summary: 'Initiate OAuth 2.1 login flow with PKCE (creates LoginRequest)' })
  @ApiHeader({
    name: 'x-session-id',
    required: false,
    description: 'If present and valid, the resulting tokens will be written to this session',
  })
  @ApiOkResponse({ type: LoginResponseDto })
  async login(@Headers('x-session-id') existingSessionId?: string) {
    return this.authService.initiateLogin(existingSessionId);
  }

  @Get('login/status')
  @ApiOperation({ summary: 'Poll status of a pending LoginRequest' })
  @ApiQuery({ name: 'id', required: true })
  @ApiOkResponse({ type: LoginStatusResponseDto })
  async loginStatus(@Query('id') id: string) {
    return this.authService.getLoginRequestStatus(id);
  }

  @Get('callback')
  @ApiOperation({ summary: 'OAuth callback from SoundCloud' })
  @ApiQuery({ name: 'code', required: true })
  @ApiQuery({ name: 'state', required: true })
  @ApiOkResponse({ description: 'HTML callback page' })
  @Header('Content-Type', 'text/html; charset=utf-8')
  async callback(@Query('code') code: string, @Query('state') state: string) {
    try {
      const result = await this.authService.handleCallback(code, state);
      return renderCallbackPage({
        success: result.success,
        username: result.username,
        error: result.error,
      });
    } catch (err: any) {
      this.logger.error(`Unhandled error in /auth/callback: ${err?.message}`, err?.stack);
      return renderCallbackPage({
        success: false,
        error: 'Authentication failed due to a server error. Please try again.',
      });
    }
  }

  @Get('session')
  @ApiOperation({ summary: 'Get current session status' })
  @ApiHeader({ name: 'x-session-id', required: true })
  @ApiOkResponse({ type: SessionResponseDto })
  async session(@Headers('x-session-id') sessionId: string) {
    const session = await this.authService.getSession(sessionId);
    if (!session?.accessToken) {
      return { authenticated: false };
    }
    return {
      authenticated: true,
      sessionId: session.id,
      username: session.username,
      soundcloudUserId: session.soundcloudUserId,
      expiresAt: session.expiresAt,
    };
  }

  @Post('refresh')
  @HttpCode(200)
  @ApiOperation({ summary: 'Refresh access token' })
  @ApiHeader({ name: 'x-session-id', required: true })
  @ApiOkResponse({ type: RefreshResponseDto })
  async refresh(@Headers('x-session-id') sessionId: string) {
    const session = await this.authService.refreshSession(sessionId);
    return {
      sessionId: session.id,
      expiresAt: session.expiresAt,
    };
  }

  @Post('logout')
  @HttpCode(200)
  @ApiOperation({ summary: 'Logout and invalidate session' })
  @ApiHeader({ name: 'x-session-id', required: true })
  @ApiOkResponse({ type: LogoutResponseDto })
  async logout(@Headers('x-session-id') sessionId: string) {
    await this.authService.logout(sessionId);
    return { success: true };
  }
}
