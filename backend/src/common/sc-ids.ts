/**
 * Нормализует sc_track_id к чистому числовому виду.
 * Клиент может прислать URN ("soundcloud:tracks:12345") или число ("12345").
 * Qdrant хранит point_id как int, `indexed_tracks.scTrackId` — числовая строка.
 * Возвращает null если вход невалиден — вызывающий должен игнорировать событие.
 */
export function normalizeScTrackId(input: string | null | undefined): string | null {
  if (!input) return null;
  const last = input.includes(':') ? input.split(':').pop() ?? '' : input;
  return /^\d+$/.test(last) ? last : null;
}