import { SetMetadata } from '@nestjs/common';

export interface CacheClearOptions {
  keys: string[];
}

export const CACHE_CLEAR_OPTIONS_KEY = 'cache:clear:options';

export const CacheClear = (...keys: string[]) => SetMetadata(CACHE_CLEAR_OPTIONS_KEY, { keys });
