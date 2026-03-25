import { Body, Controller, Delete, Get, Headers, Param, Patch, Post, UnauthorizedException } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { ApiHeader, ApiOkResponse, ApiOperation, ApiTags } from '@nestjs/swagger';
import { CreateOAuthAppDto, OAuthAppResponseDto, UpdateOAuthAppDto } from './dto/oauth-app.dto.js';
import { OAuthAppsService } from './oauth-apps.service.js';

@ApiTags('oauth-apps')
@Controller('oauth-apps')
export class OAuthAppsController {
  private readonly adminToken: string;

  constructor(
    private readonly service: OAuthAppsService,
    private readonly configService: ConfigService,
  ) {
    this.adminToken = this.configService.get<string>('admin.token') ?? '';
  }

  private checkAdmin(token: string | undefined) {
    if (!this.adminToken || token !== this.adminToken) {
      throw new UnauthorizedException('Invalid admin token');
    }
  }

  @Get()
  @ApiOperation({ summary: 'List all OAuth apps' })
  @ApiHeader({ name: 'x-admin-token', required: true })
  @ApiOkResponse({ type: [OAuthAppResponseDto] })
  async findAll(@Headers('x-admin-token') adminToken: string) {
    this.checkAdmin(adminToken);
    const apps = await this.service.findAll();
    return apps.map((app) => ({
      id: app.id,
      name: app.name,
      clientId: app.clientId,
      redirectUri: app.redirectUri,
      active: app.active,
      bannedAt: app.bannedAt,
      banReason: app.banReason,
      createdAt: app.createdAt,
    }));
  }

  @Post()
  @ApiOperation({ summary: 'Create new OAuth app' })
  @ApiHeader({ name: 'x-admin-token', required: true })
  async create(
    @Headers('x-admin-token') adminToken: string,
    @Body() dto: CreateOAuthAppDto,
  ) {
    this.checkAdmin(adminToken);
    return this.service.create(dto);
  }

  @Patch(':id')
  @ApiOperation({ summary: 'Update OAuth app' })
  @ApiHeader({ name: 'x-admin-token', required: true })
  async update(
    @Headers('x-admin-token') adminToken: string,
    @Param('id') id: string,
    @Body() dto: UpdateOAuthAppDto,
  ) {
    this.checkAdmin(adminToken);
    return this.service.update(id, dto);
  }

  @Delete(':id')
  @ApiOperation({ summary: 'Delete OAuth app' })
  @ApiHeader({ name: 'x-admin-token', required: true })
  async remove(
    @Headers('x-admin-token') adminToken: string,
    @Param('id') id: string,
  ) {
    this.checkAdmin(adminToken);
    await this.service.remove(id);
    return { success: true };
  }
}
