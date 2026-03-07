import { ApiPropertyOptional } from '@nestjs/swagger';
import { Transform } from 'class-transformer';
import { IsBoolean, IsInt, IsOptional, IsString, Max, Min } from 'class-validator';

export class PaginationQuery {
  @ApiPropertyOptional({ minimum: 1, maximum: 200, default: 50 })
  @IsOptional()
  @IsInt()
  @Min(1)
  @Max(200)
  @Transform(({ value }) => Number.parseInt(value, 10))
  limit?: number = 50;

  @ApiPropertyOptional({ description: 'Cursor for cursor-based pagination (from next_href)' })
  @IsOptional()
  @IsString()
  cursor?: string;

  @ApiPropertyOptional({ deprecated: true })
  @IsOptional()
  @IsInt()
  @Min(0)
  @Transform(({ value }) => Number.parseInt(value, 10))
  offset?: number;

  @ApiPropertyOptional({ default: true })
  @IsOptional()
  @IsBoolean()
  @Transform(({ value }) => value === 'true' || value === true)
  linked_partitioning?: boolean = true;
}
