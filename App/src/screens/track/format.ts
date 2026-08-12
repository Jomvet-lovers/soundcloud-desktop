import type { Track, TrackParticipant } from '@sc/data';

/** Кликабельная персона в строке участников. */
export interface CreditPerson {
  id: string | null;
  name: string;
}

/** Группа участников: префикс («feat. »), люди-ссылки, суффикс («— ремикс»). */
export interface CreditGroup {
  prefix: string;
  people: CreditPerson[];
  suffix: string;
}

// remix/prod по роли, остальное (вкл. primary/guest/null) → feat
export function participantGroups(track: Track): CreditGroup[] {
  const bucket: Record<'feat' | 'remix' | 'prod', CreditPerson[]> = { feat: [], remix: [], prod: [] };
  const seen = new Set<string>();
  for (const p of track.participants) {
    if (!p.name || seen.has(p.name)) continue;
    seen.add(p.name);
    const r = (p.role ?? '').toLowerCase();
    const key = /remix/.test(r) ? 'remix' : /prod/.test(r) ? 'prod' : 'feat';
    bucket[key].push(personOf(p));
  }
  const groups: CreditGroup[] = [];
  if (bucket.feat.length) groups.push({ prefix: 'feat. ', people: bucket.feat, suffix: '' });
  if (bucket.remix.length) groups.push({ prefix: '', people: bucket.remix, suffix: ' — ремикс' });
  if (bucket.prod.length) groups.push({ prefix: 'prod. ', people: bucket.prod, suffix: '' });
  return groups;
}

const personOf = (p: TrackParticipant): CreditPerson => ({ id: p.id, name: p.name });

/** Цвет/лейбл пилюли типа загрузки (донор `KIND_TONE`). */
export function uploadKindPill(kind: string | null): { label: string; tint: string } | null {
  if (!kind) return null;
  const tones: Record<string, string> = {
    original: '#6ee7b7', demo: '#7dd3fc', alt: '#c4b5fd', reupload: '#fcd34d', cover: '#f0abfc',
  };
  const tint = tones[kind.toLowerCase()];
  if (!tint) return null;
  return { label: kind.toUpperCase(), tint };
}

// оптимистичная поправка: серверное число ± локальный тоггл
export function likeCountOf(track: Track, liked: boolean): number {
  const base = track.likes_count ?? 0;
  const fav = track.user_favorite ?? false;
  if (liked && !fav) return base + 1;
  if (!liked && fav) return Math.max(0, base - 1);
  return base;
}

/** Компактные числа: 1.2K / 3.4M / 12 (донор `fc`). */
export function formatCount(n: number | null | undefined): string {
  if (n == null) return '—';
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(n >= 10_000_000 ? 0 : 1).replace(/\.0$/, '')}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(n >= 10_000 ? 0 : 1).replace(/\.0$/, '')}K`;
  return String(n);
}
