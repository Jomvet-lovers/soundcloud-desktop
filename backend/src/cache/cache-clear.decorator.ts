import { SetMetadata } from '@nestjs/common';

export interface CacheClearOptions {
  keys: string[];
}

export const CACHE_CLEAR_OPTIONS_KEY = 'cache:clear:options';

/**
 * Сбрасывает кэш по перечисленным логическим ключам после успешного выполнения хендлера.
 *
 * Каждый ключ поддерживает плейсхолдеры `{name}` — резолвятся из `request.params`
 * (route-параметров). Должны совпадать с тем что было в `@Cached({ key })` —
 * иначе bucket в Redis не найдётся.
 *
 * @example
 * // PUT /playlists/:playlistUrn — чистит только этот плейлист, остальные в кэше остаются
 * @CacheClear('me-playlists', 'playlist-detail:{playlistUrn}', 'playlist-tracks:{playlistUrn}')
 */
export const CacheClear = (...keys: string[]) => SetMetadata(CACHE_CLEAR_OPTIONS_KEY, { keys });
