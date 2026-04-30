import { SetMetadata } from '@nestjs/common';

export interface CachedOptions {
  /** TTL в секундах */
  ttl: number;
  /** 'shared' — один кэш для всех юзеров, 'user' — per-session */
  scope?: 'shared' | 'user';
  /**
   * Логический ключ для точечной инвалидции через @CacheClear.
   *
   * Поддерживает плейсхолдеры `{name}` — резолвятся из `request.params` (route-параметров).
   * Так одно и то же декларативное имя ключа разводится в Redis по разным bucket'ам
   * на разные ресурсы.
   *
   * @example
   * // GET /playlists/:playlistUrn → ключ 'playlist-detail:soundcloud:playlists:42'
   * @Cached({ ttl: 3600, key: 'playlist-detail:{playlistUrn}' })
   */
  key?: string;
}

export const CACHE_OPTIONS_KEY = 'cache:options';

/**
 * Декоратор для кэширования ответа эндпоинта в Redis.
 *
 * @example
 * // Кэшировать related tracks на сутки для всех юзеров
 * @Cached({ ttl: 86400 })
 *
 * // Кэшировать профиль /me на 60 секунд per-user
 * @Cached({ ttl: 60, scope: 'user' })
 *
 * // Кэшировать лайки юзера с ключом для инвалидции
 * @Cached({ ttl: 30, scope: 'user', key: 'me-liked-tracks' })
 */
export const Cached = (options: CachedOptions) =>
  SetMetadata(CACHE_OPTIONS_KEY, { scope: 'shared', ...options });
