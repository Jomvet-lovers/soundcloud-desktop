import { memo, useEffect, useRef } from 'react';

interface InfiniteSentinelProps {
  hasMore: boolean;
  isFetching: boolean;
  onLoadMore: () => void;
  rootMargin?: string;
}

function InfiniteSentinelImpl({
  hasMore,
  isFetching,
  onLoadMore,
  rootMargin = '600px 0px',
}: InfiniteSentinelProps) {
  const ref = useRef<HTMLDivElement>(null);
  const fnRef = useRef(onLoadMore);
  fnRef.current = onLoadMore;

  useEffect(() => {
    const el = ref.current;
    if (!el || !hasMore) return;
    const obs = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          if (e.isIntersecting) {
            fnRef.current();
            break;
          }
        }
      },
      { rootMargin },
    );
    obs.observe(el);
    return () => obs.disconnect();
  }, [hasMore, rootMargin]);

  if (!hasMore && !isFetching) return null;
  return <div ref={ref} className="h-8 w-full" aria-hidden />;
}

export const InfiniteSentinel = memo(InfiniteSentinelImpl);
