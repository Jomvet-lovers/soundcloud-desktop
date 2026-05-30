import {useQuery} from '@tanstack/react-query';
import {useMemo} from 'react';
import type {Track, TrackScdMeta} from '../stores/player';
import {api} from './api';

/**
 * Клиентский добор `_scd_meta` (статус-бейдж трека) для шелфов из SC-источников.
 *
 * `/tracks/:urn/related` (и public-search) приходят SC-shaped без нашей меты, а
 * она нужна для бейджа A/C/F. Единственный эндпоинт, который её отдаёт —
 * одиночный `/tracks/:urn` (бэк кеширует ~10м). Добираем её точечно и кешируем
 * по урну в памяти, чтобы один и тот же трек в разных шелфах/жанрах не дёргал
 * бэк повторно. `null` = трека нет в нашем каталоге → мета не положена, бейдж
 * остаётся скрытым (компонент null-safe).
 */
const metaCache = new Map<string, TrackScdMeta | null>();

/** `pending` storage/index ещё перевернётся (pending→ok, pending→indexed) —
 *  такую мету НЕ пиним, иначе бейдж замёрзнет на первом увиденном статусе на
 *  всю сессию. Терминальную (готово/ошибка/too_long/`null`=не в каталоге)
 *  кешируем — она уже не меняется. */
function isTerminal(meta: TrackScdMeta | null): boolean {
    if (!meta) return true;
    return meta.storage_state !== 'pending' && meta.index_state !== 'pending';
}

async function fetchMeta(urn: string): Promise<TrackScdMeta | null> {
    if (metaCache.has(urn)) return metaCache.get(urn) ?? null;
    const full = await api<Track>(`/tracks/${encodeURIComponent(urn)}`).catch(() => null);
    const meta = full?._scd_meta ?? null;
    if (isTerminal(meta)) metaCache.set(urn, meta); // pending → re-fetch на следующем заходе
    return meta;
}

async function enrich(tracks: Track[]): Promise<Track[]> {
    const urns = [...new Set(tracks.filter((t) => t.urn && !t._scd_meta).map((t) => t.urn))];
    const resolved = new Map<string, TrackScdMeta | null>();
    await Promise.all(urns.map(async (urn) => resolved.set(urn, await fetchMeta(urn))));
    let changed = false;
    const out = tracks.map((t) => {
        if (t._scd_meta || !t.urn) return t;
        const meta = resolved.get(t.urn);
        if (!meta) return t;
        changed = true;
        return {...t, _scd_meta: meta};
    });
    return changed ? out : tracks;
}

/**
 * Возвращает те же треки, но с добранной `_scd_meta` для бейджей. Бандлит ровно
 * переданный (отрендеренный) список — не весь пул. До загрузки отдаёт исходные
 * треки, потом — обогащённые.
 */
export function useScdMeta(tracks: Track[]): Track[] {
    const key = useMemo(() => tracks.map((t) => t.urn).join(','), [tracks]);
    const needsEnrich = useMemo(() => tracks.some((t) => t.urn && !t._scd_meta), [tracks]);
    const {data} = useQuery({
        queryKey: ['scd-meta-enrich', key],
        queryFn: () => enrich(tracks),
        enabled: needsEnrich,
        staleTime: 10 * 60 * 1000,
        gcTime: 10 * 60 * 1000,
    });
    return data ?? tracks;
}
