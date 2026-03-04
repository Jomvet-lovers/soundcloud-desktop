import { Controller, Get, Query, UseGuards } from '@nestjs/common';
import { ApiHeader, ApiOperation, ApiQuery, ApiTags } from '@nestjs/swagger';
import { AccessToken } from '../common/decorators/access-token.decorator.js';
import { AuthGuard } from '../common/guards/auth.guard.js';
import { ResolveService } from './resolve.service.js';

@ApiTags('resolve')
@ApiHeader({ name: 'x-session-id', required: true })
@UseGuards(AuthGuard)
@Controller('resolve')
export class ResolveController {
  constructor(private readonly resolveService: ResolveService) {}

  @Get()
  @ApiOperation({ summary: 'Resolve a SoundCloud URL to a resource' })
  @ApiQuery({ name: 'url', required: true, description: 'SoundCloud URL to resolve' })
  resolve(@AccessToken() token: string, @Query('url') url: string) {
    return this.resolveService.resolve(token, url);
  }
}
