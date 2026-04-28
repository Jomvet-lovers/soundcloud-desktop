const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/**
 * Валидирует, что строка — UUID. Защищает БД от попадания мусора (например, "undefined"
 * от старых клиентов), который Postgres отбивает с invalid input syntax.
 */
export function isValidUuid(value: unknown): value is string {
  return typeof value === 'string' && UUID_RE.test(value);
}
