import { Controller, Get, UseGuards } from '@nestjs/common';
import { ApiHeader, ApiOkResponse, ApiOperation, ApiTags } from '@nestjs/swagger';
import { SessionId } from '../common/decorators/session-id.decorator.js';
import { AuthGuard } from '../common/guards/auth.guard.js';
import { FeaturedService } from './featured.service.js';

@ApiTags('featured')
@ApiHeader({ name: 'x-session-id', required: true })
@UseGuards(AuthGuard)
@Controller('featured')
export class FeaturedController {
  constructor(private readonly featuredService: FeaturedService) {}

  @Get()
  @ApiOperation({ summary: 'Get a featured item (weighted random)' })
  @ApiOkResponse({ description: 'Featured item or null' })
  pick(@SessionId() sessionId: string) {
    return this.featuredService.pick(sessionId);
  }
}
