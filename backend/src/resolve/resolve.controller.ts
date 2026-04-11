import { Controller, Get, Headers, Query } from '@nestjs/common';
import { ApiHeader, ApiOperation, ApiQuery, ApiTags } from '@nestjs/swagger';
import { AuthService } from '../auth/auth.service.js';
import { Cached } from '../cache/cached.decorator.js';
import { ResolveService } from './resolve.service.js';

@ApiTags('resolve')
@Controller('resolve')
export class ResolveController {
  constructor(
    private readonly resolveService: ResolveService,
    private readonly authService: AuthService,
  ) {}

  @Get()
  @Cached({ ttl: 86400 })
  @ApiOperation({ summary: 'Resolve a SoundCloud URL to a resource' })
  @ApiHeader({ name: 'x-session-id', required: false })
  @ApiQuery({ name: 'url', required: true, description: 'SoundCloud URL to resolve' })
  async resolve(@Headers('x-session-id') sessionId: string | undefined, @Query('url') url: string) {
    if (sessionId) {
      try {
        const token = await this.authService.getValidAccessToken(sessionId);
        return await this.resolveService.resolve(token, url);
      } catch {}
    }
    return this.resolveService.resolveWithRandomToken(url);
  }
}
