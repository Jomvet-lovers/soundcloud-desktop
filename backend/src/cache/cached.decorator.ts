import { SetMetadata } from '@nestjs/common';

export interface CachedOptions {
  /** TTL в секундах */
  ttl: number;
  /** 'shared' — один кэш для всех юзеров, 'user' — per-session */
  scope?: 'shared' | 'user';
}

export const CACHE_OPTIONS_KEY = 'cache:options';

/**
 * Декоратор для кэширования ответа эндпоинта в PostgreSQL.
 *
 * @example
 * // Кэшировать related tracks на сутки для всех юзеров
 * @Cached({ ttl: 86400 })
 *
 * // Кэшировать профиль /me на 60 секунд per-user
 * @Cached({ ttl: 60, scope: 'user' })
 */
export const Cached = (options: CachedOptions) =>
  SetMetadata(CACHE_OPTIONS_KEY, { scope: 'shared', ...options });
