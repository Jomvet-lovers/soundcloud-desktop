import { Body, Controller, Get, Headers, HttpCode, Post, Query } from '@nestjs/common';
import { ApiHeader, ApiOkResponse, ApiOperation, ApiQuery, ApiTags } from '@nestjs/swagger';
import {
  ClaimLinkRequestDto,
  ClaimLinkResponseDto,
  CreateLinkRequestDto,
  CreateLinkResponseDto,
  LinkStatusResponseDto,
} from './dto/link.dto.js';
import { LinkService } from './link.service.js';

@ApiTags('auth')
@Controller('auth/link')
export class LinkController {
  constructor(private readonly linkService: LinkService) {}

  @Post('create')
  @HttpCode(200)
  @ApiOperation({ summary: 'Create a QR link request (cross-device session transfer)' })
  @ApiHeader({
    name: 'x-session-id',
    required: false,
    description: 'Required for mode=push (caller acts as the source of session tokens)',
  })
  @ApiOkResponse({ type: CreateLinkResponseDto })
  async create(
    @Body() body: CreateLinkRequestDto,
    @Headers('x-session-id') sourceSessionId?: string,
  ) {
    return this.linkService.create(body.mode, sourceSessionId);
  }

  @Post('claim')
  @HttpCode(200)
  @ApiOperation({
    summary:
      'Claim a QR link token. For pull mode caller must provide x-session-id; for push mode no session is needed.',
  })
  @ApiHeader({ name: 'x-session-id', required: false })
  @ApiOkResponse({ type: ClaimLinkResponseDto })
  async claim(
    @Body() body: ClaimLinkRequestDto,
    @Headers('x-session-id') sourceSessionId?: string,
  ) {
    return this.linkService.claim(body.claimToken, sourceSessionId);
  }

  @Get('status')
  @ApiOperation({ summary: 'Poll link request status' })
  @ApiQuery({ name: 'id', required: true })
  @ApiOkResponse({ type: LinkStatusResponseDto })
  async status(@Query('id') id: string) {
    return this.linkService.getStatus(id);
  }
}
