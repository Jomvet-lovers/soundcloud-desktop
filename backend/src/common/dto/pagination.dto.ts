import { ApiPropertyOptional } from '@nestjs/swagger';
import { Transform } from 'class-transformer';
import { IsBoolean, IsInt, IsOptional, Max, Min } from 'class-validator';

export class PaginationQuery {
  @ApiPropertyOptional({ minimum: 1, maximum: 200, default: 50 })
  @IsOptional()
  @IsInt()
  @Min(1)
  @Max(200)
  @Transform(({ value }) => Number.parseInt(value, 10))
  limit?: number = 50;

  @ApiPropertyOptional({ default: 0, deprecated: true })
  @IsOptional()
  @IsInt()
  @Min(0)
  @Transform(({ value }) => Number.parseInt(value, 10))
  offset?: number = 0;

  @ApiPropertyOptional({ default: true })
  @IsOptional()
  @IsBoolean()
  @Transform(({ value }) => value === 'true' || value === true)
  linked_partitioning?: boolean = true;
}
