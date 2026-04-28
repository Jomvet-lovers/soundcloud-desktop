import { ApiProperty, ApiPropertyOptional } from '@nestjs/swagger';
import { IsIn, IsString } from 'class-validator';

export class CreateLinkRequestDto {
  @ApiProperty({
    enum: ['pull', 'push'],
    description:
      'pull = caller has no session, will receive one after another device claims; push = caller has session and gives it to the device that claims',
  })
  @IsString()
  @IsIn(['pull', 'push'])
  mode: 'pull' | 'push';
}

export class CreateLinkResponseDto {
  @ApiProperty({ format: 'uuid' }) linkRequestId: string;
  @ApiProperty({ description: 'Token to render in the QR code' }) claimToken: string;
  @ApiProperty({ type: String, format: 'date-time' }) expiresAt: Date;
}

export class ClaimLinkRequestDto {
  @ApiProperty()
  @IsString()
  claimToken: string;
}

export class ClaimLinkResponseDto {
  @ApiProperty({ format: 'uuid', description: 'New session id (only relevant for push mode)' })
  sessionId: string;
  @ApiProperty({ enum: ['pull', 'push'] }) mode: 'pull' | 'push';
}

export class LinkStatusResponseDto {
  @ApiProperty({ enum: ['pending', 'claimed', 'failed', 'expired'] })
  status: 'pending' | 'claimed' | 'failed' | 'expired';
  @ApiProperty({ enum: ['pull', 'push'] }) mode: 'pull' | 'push';
  @ApiPropertyOptional({ format: 'uuid' }) sessionId?: string;
  @ApiPropertyOptional() error?: string;
}
