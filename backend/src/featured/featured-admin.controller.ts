import {
  Body,
  Controller,
  Delete,
  Get,
  Headers,
  HttpCode,
  Param,
  Patch,
  Post,
  UnauthorizedException,
} from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import {
  ApiHeader,
  ApiOkResponse,
  ApiOperation,
  ApiProperty,
  ApiPropertyOptional,
  ApiTags,
} from '@nestjs/swagger';
import { IsBoolean, IsIn, IsInt, IsNotEmpty, IsOptional, IsString, Min } from 'class-validator';
import { FeaturedService, type FeaturedItemType } from './featured.service.js';

class CreateFeaturedDto {
  @ApiProperty({ example: 'track', enum: ['track', 'playlist', 'user'] })
  @IsString()
  @IsIn(['track', 'playlist', 'user'])
  type: FeaturedItemType;

  @ApiProperty({ example: 'soundcloud:tracks:123456' })
  @IsString()
  @IsNotEmpty()
  scUrn: string;

  @ApiPropertyOptional({ example: 3, default: 1, description: 'Relative weight for random pick' })
  @IsInt()
  @Min(1)
  @IsOptional()
  weight?: number;

  @ApiPropertyOptional({ example: true, default: true })
  @IsBoolean()
  @IsOptional()
  active?: boolean;
}

class UpdateFeaturedDto {
  @ApiPropertyOptional({ example: 'playlist', enum: ['track', 'playlist', 'user'] })
  @IsString()
  @IsIn(['track', 'playlist', 'user'])
  @IsOptional()
  type?: FeaturedItemType;

  @ApiPropertyOptional({ example: 'soundcloud:playlists:789' })
  @IsString()
  @IsNotEmpty()
  @IsOptional()
  scUrn?: string;

  @ApiPropertyOptional({ example: 5, description: 'Relative weight for random pick' })
  @IsInt()
  @Min(1)
  @IsOptional()
  weight?: number;

  @ApiPropertyOptional({ example: false })
  @IsBoolean()
  @IsOptional()
  active?: boolean;
}

class FeaturedItemResponse {
  @ApiProperty({ example: 'a1b2c3d4-e5f6-7890-abcd-ef1234567890' })
  id: string;
  @ApiProperty({ example: 'track', enum: ['track', 'playlist', 'user'] })
  type: FeaturedItemType;
  @ApiProperty({ example: 'soundcloud:tracks:123456' })
  scUrn: string;
  @ApiProperty({ example: 3 })
  weight: number;
  @ApiProperty({ example: true })
  active: boolean;
  @ApiProperty({ example: '2026-03-28T12:00:00.000Z' })
  createdAt: Date;
}

@ApiTags('featured-admin')
@Controller('admin/featured')
export class FeaturedAdminController {
  private readonly adminToken: string;

  constructor(
    private readonly featuredService: FeaturedService,
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
  @ApiOperation({ summary: 'List all featured items' })
  @ApiHeader({ name: 'x-admin-token', required: true })
  @ApiOkResponse({ type: [FeaturedItemResponse] })
  findAll(@Headers('x-admin-token') adminToken: string) {
    this.checkAdmin(adminToken);
    return this.featuredService.findAll();
  }

  @Post()
  @ApiOperation({ summary: 'Create featured item' })
  @ApiHeader({ name: 'x-admin-token', required: true })
  @ApiOkResponse({ type: FeaturedItemResponse })
  create(@Headers('x-admin-token') adminToken: string, @Body() dto: CreateFeaturedDto) {
    this.checkAdmin(adminToken);
    return this.featuredService.create(dto);
  }

  @Patch(':id')
  @ApiOperation({ summary: 'Update featured item' })
  @ApiHeader({ name: 'x-admin-token', required: true })
  @ApiOkResponse({ type: FeaturedItemResponse })
  update(
    @Headers('x-admin-token') adminToken: string,
    @Param('id') id: string,
    @Body() dto: UpdateFeaturedDto,
  ) {
    this.checkAdmin(adminToken);
    return this.featuredService.update(id, dto);
  }

  @Delete(':id')
  @HttpCode(200)
  @ApiOperation({ summary: 'Delete featured item' })
  @ApiHeader({ name: 'x-admin-token', required: true })
  async remove(@Headers('x-admin-token') adminToken: string, @Param('id') id: string) {
    this.checkAdmin(adminToken);
    await this.featuredService.remove(id);
    return { success: true };
  }
}
