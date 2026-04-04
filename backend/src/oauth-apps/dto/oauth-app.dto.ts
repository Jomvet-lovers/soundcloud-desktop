import { IsBoolean, IsOptional, IsString } from 'class-validator';

export class CreateOAuthAppDto {
  @IsString()
  name: string;

  @IsString()
  clientId: string;

  @IsString()
  clientSecret: string;

  @IsString()
  redirectUri: string;

  @IsOptional()
  @IsBoolean()
  active?: boolean;
}

export class UpdateOAuthAppDto {
  @IsOptional()
  @IsString()
  name?: string;

  @IsOptional()
  @IsString()
  clientId?: string;

  @IsOptional()
  @IsString()
  clientSecret?: string;

  @IsOptional()
  @IsString()
  redirectUri?: string;

  @IsOptional()
  @IsBoolean()
  active?: boolean;
}

export class OAuthAppResponseDto {
  id: string;
  name: string;
  clientId: string;
  redirectUri: string;
  active: boolean;
  createdAt: Date;
}
