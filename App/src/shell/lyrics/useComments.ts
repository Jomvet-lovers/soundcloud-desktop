import { useEffect, useState } from 'react';
import { type Comment, useSc, type Urn } from '@sc/data';

const PAGE_SIZE = 50;
const MAX_PAGES = 12; // потолок ~600 комментов (донор тянет все страницы infinite-query)

export interface CommentsLoad {
  loading: boolean;
  items: Comment[];
}

/** Комментарии трека со ВСЕМИ страницами (донор `useTrackComments` + авто-fetchNextPage):
 *  грузим страницу за страницей (offset += limit, пока `has_more`), прогрессивно
 *  наполняя ленту. `enabled=false` (панель закрыта) не гонит запрос. */
export function useComments(urn: Urn | null, enabled: boolean): CommentsLoad {
  const sc = useSc();
  const [state, setState] = useState<CommentsLoad>({ loading: false, items: [] });

  useEffect(() => {
    if (!urn || !enabled) {
      setState({ loading: false, items: [] });
      return;
    }
    let cancelled = false;
    setState({ loading: true, items: [] });
    (async () => {
      const all: Comment[] = [];
      for (let page = 0; page < MAX_PAGES; page++) {
        const res = await sc.tracks.comments(urn, PAGE_SIZE, page * PAGE_SIZE);
        if (cancelled) return;
        all.push(...res.items);
        setState({ loading: false, items: [...all] }); // прогрессивно
        if (!res.has_more) break;
      }
    })().catch(() => {
      if (!cancelled) setState({ loading: false, items: [] });
    });
    return () => {
      cancelled = true;
    };
  }, [sc, urn, enabled]);

  return state;
}
