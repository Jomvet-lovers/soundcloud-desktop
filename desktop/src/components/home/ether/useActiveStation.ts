import {useCallback, useEffect, useRef, useState} from 'react';

/** Какая станция (секция) сейчас «в фокусе» скролла. Дискретный защёлк по
 *  IntersectionObserver-сентинелам — никакой непрерывной интерполяции scrollY
 *  (скролл живёт в <main>, координаты ломаются на ресайзах/догрузках). */
export function useActiveStation(ids: string[]) {
  const [active, setActive] = useState<string | null>(ids[0] ?? null);
  const nodes = useRef(new Map<string, HTMLElement>());
  const visible = useRef(new Set<string>());
  const observer = useRef<IntersectionObserver | null>(null);
  const order = useRef(ids);
  order.current = ids;

  const idsKey = ids.join(',');

  // biome-ignore lint/correctness/useExhaustiveDependencies: пересоздаём обсервер при смене набора станций.
  useEffect(() => {
    const first = nodes.current.values().next().value as HTMLElement | undefined;
    const root = (first?.closest('main') as HTMLElement | null) ?? null;
    const io = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const id = (entry.target as HTMLElement).dataset.station;
          if (!id) continue;
          if (entry.isIntersecting) visible.current.add(id);
          else visible.current.delete(id);
        }
        const next = order.current.find((id) => visible.current.has(id));
        if (next) setActive(next);
      },
      { root, rootMargin: '-140px 0px -45% 0px', threshold: 0 },
    );
    observer.current = io;
    for (const node of nodes.current.values()) io.observe(node);
    return () => {
      io.disconnect();
      observer.current = null;
      visible.current.clear();
    };
  }, [idsKey]);

  const register = useCallback((id: string) => {
    return (node: HTMLElement | null) => {
      const prev = nodes.current.get(id);
      if (prev && observer.current) observer.current.unobserve(prev);
      if (node) {
        node.dataset.station = id;
        nodes.current.set(id, node);
        observer.current?.observe(node);
      } else {
        nodes.current.delete(id);
      }
    };
  }, []);

  const jump = useCallback((id: string) => {
    nodes.current.get(id)?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }, []);

  return { active, register, jump };
}
