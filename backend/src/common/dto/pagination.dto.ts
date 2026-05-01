import { ApiPropertyOptional } from '@nestjs/swagger';
import { Transform } from 'class-transformer';
import { IsInt, IsOptional, Max, Min } from 'class-validator';

export class PaginationQuery {
  @ApiPropertyOptional({ minimum: 1, maximum: 200, default: 30 })
  @IsOptional()
  @IsInt()
  @Min(1)
  @Max(200)
  @Transform(({ value }) => Number.parseInt(value, 10))
  limit?: number = 30;

  @ApiPropertyOptional({ minimum: 0, default: 0, description: '0-based page index' })
  @IsOptional()
  @IsInt()
  @Min(0)
  @Transform(({ value }) => Number.parseInt(value, 10))
  page?: number = 0;
}
