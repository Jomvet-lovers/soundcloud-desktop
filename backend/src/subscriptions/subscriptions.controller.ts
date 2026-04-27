import {
  Body,
  Controller,
  Delete,
  Get,
  Headers,
  HttpCode,
  Param,
  Post,
  UnauthorizedException,
} from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { ApiHeader, ApiOperation, ApiTags } from '@nestjs/swagger';
import { SubscriptionsService } from './subscriptions.service.js';

@ApiTags('subscriptions')
@Controller('admin/subscriptions')
export class SubscriptionsController {
  private readonly adminToken: string;

  constructor(
    private readonly service: SubscriptionsService,
    configService: ConfigService,
  ) {
    this.adminToken = configService.get<string>('admin.token') ?? '';
  }

  private checkAdmin(token: string | undefined) {
    if (!this.adminToken || token !== this.adminToken) {
      throw new UnauthorizedException('Invalid admin token');
    }
  }

  @Get()
  @ApiOperation({ summary: 'List all subscriptions' })
  @ApiHeader({ name: 'x-admin-token', required: true })
  list(@Headers('x-admin-token') adminToken: string) {
    this.checkAdmin(adminToken);
    return this.service.list();
  }

  @Post()
  @HttpCode(200)
  @ApiOperation({ summary: 'Create or update subscription' })
  @ApiHeader({ name: 'x-admin-token', required: true })
  async upsert(
    @Headers('x-admin-token') adminToken: string,
    @Body() body: { user_urn: string; exp_date: number },
  ) {
    this.checkAdmin(adminToken);
    await this.service.upsert(body.user_urn, body.exp_date);
    return { message: 'ok' };
  }

  @Delete(':userUrn')
  @ApiOperation({ summary: 'Delete subscription' })
  @ApiHeader({ name: 'x-admin-token', required: true })
  async remove(@Headers('x-admin-token') adminToken: string, @Param('userUrn') userUrn: string) {
    this.checkAdmin(adminToken);
    const deleted = await this.service.remove(userUrn);
    return { deleted };
  }
}
