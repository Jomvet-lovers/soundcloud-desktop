import { ApiProperty, ApiPropertyOptional } from '@nestjs/swagger';

export class LoginResponseDto {
  @ApiProperty({ description: 'SoundCloud OAuth authorization URL' }) url: string;
  @ApiProperty({ description: 'Login request id, used for polling status', format: 'uuid' })
  loginRequestId: string;
}

export class LoginStatusResponseDto {
  @ApiProperty({ enum: ['pending', 'completed', 'failed', 'expired'] })
  status: 'pending' | 'completed' | 'failed' | 'expired';

  @ApiPropertyOptional({ format: 'uuid', description: 'Available when status=completed' })
  sessionId?: string;

  @ApiPropertyOptional() error?: string;
}

export class SessionResponseDto {
  @ApiProperty() authenticated: boolean;
  @ApiPropertyOptional({ format: 'uuid' }) sessionId?: string;
  @ApiPropertyOptional() username?: string;
  @ApiPropertyOptional() soundcloudUserId?: string;
  @ApiPropertyOptional({ type: String, format: 'date-time' }) expiresAt?: Date;
}

export class RefreshResponseDto {
  @ApiProperty({ format: 'uuid' }) sessionId: string;
  @ApiProperty({ type: String, format: 'date-time' }) expiresAt: Date;
}

export class LogoutResponseDto {
  @ApiProperty() success: boolean;
}
