/**
 * «Прослойка» дозагрузки очереди для конкретного контекста воспроизведения
 * (лайки, плейлист, лента подписок…). Пока активный источник не исчерпан, его
 * страницы доезжают в очередь раньше, чем включится волна: контекст играется
 * ДО КОНЦА. Источник кончился — отдаёт управление autopilot'у (волне), см.
 * lib/queue-autopilot.ts.
 *
 * Источник один на сессию воспроизведения: ставится при play() из контекста,
 * сбрасывается при старте нового play() (player store зовёт reset-handler) и
 * при исчерпании.
 */

import {type Track, usePlayerStore} from '../stores/player';
import {api} from './api';

export interface QueueContinuationSource {
    /** Имя для логов. */
    readonly kind: string;

    /**
     * Следующая порция треков. Пустой массив = источник исчерпан (дальше волна).
     * Дубли отсеивает вызывающий по живой очереди — здесь чистая пагинация.
     */
    next(): Promise<Track[]>;
}

let active: QueueContinuationSource | null = null;

export function setQueueContinuationSource(src: QueueContinuationSource | null): void {
    active = src;
}

export function getQueueContinuationSource(): QueueContinuationSource | null {
    return active;
}

interface PagedTracks {
    collection: Track[];
    has_more: boolean;
}

/**
 * Источник из любого page-based эндпоинта `{ collection, has_more }`.
 * Тянет по странице за вызов; `has_more=false` или пустая страница = иссяк.
 */
export function createPagedContinuationSource(
    kind: string,
    base: string,
    opts?: { pageSize?: number; startPage?: number },
): QueueContinuationSource {
    const pageSize = opts?.pageSize ?? 50;
    let page = Math.max(0, opts?.startPage ?? 0);
    let done = false;
    return {
        kind,
        async next() {
            if (done) return [];
            const sep = base.includes('?') ? '&' : '?';
            const data = await api<PagedTracks>(`${base}${sep}limit=${pageSize}&page=${page}`);
            page += 1;
            const collection = data.collection ?? [];
            if (!data.has_more || collection.length === 0) done = true;
            return collection;
        },
    };
}

const LIKES_PAGE_SIZE = 50;

/**
 * Лайки от и до. `alreadyLoaded` — сколько лайков уже в очереди при старте
 * (первая страница списка): пропускаем полностью покрытые страницы, стык
 * добивает дедуп на стороне autopilot'а.
 */
export function createLikesContinuationSource(alreadyLoaded = 0): QueueContinuationSource {
    return createPagedContinuationSource('likes', '/me/likes/tracks', {
        pageSize: LIKES_PAGE_SIZE,
        startPage: Math.floor(Math.max(0, alreadyLoaded) / LIKES_PAGE_SIZE),
    });
}

/**
 * Поставить «лайки до конца» под текущую (уже выставленную play()) очередь —
 * один вызов для всех точек входа в лайки (LikesTab, шелф на home, превью
 * в library). Зовётся в `onPlay` сразу после play(): тот сбросил прошлый
 * источник, мы ставим свой. Стартовая страница — из живой длины очереди.
 */
export function armLikesContinuation(): void {
    const loaded = usePlayerStore.getState().queue.length;
    setQueueContinuationSource(createLikesContinuationSource(loaded));
}
