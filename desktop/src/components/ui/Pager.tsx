import { ChevronLeft, ChevronRight, Loader2 } from '../../lib/icons';

/** Numbered-pagination footer (Prev / Page N / Next) for paged result lists. */
export function Pager({
  page,
  hasMore,
  isFetching,
  onPage,
}: {
  page: number;
  hasMore: boolean;
  isFetching: boolean;
  onPage: (page: number) => void;
}) {
  if (page === 0 && !hasMore) return null;
  const buttonClass =
    'flex items-center gap-1 rounded-lg px-2.5 py-1.5 text-[12px] font-medium text-white/55 transition-colors hover:bg-white/[0.06] hover:text-white/85 disabled:pointer-events-none disabled:opacity-30';
  return (
    <div className="mt-5 flex items-center justify-center gap-2">
      <button
        type="button"
        disabled={page === 0 || isFetching}
        onClick={() => onPage(page - 1)}
        className={buttonClass}
      >
        <ChevronLeft size={14} />
        {'Prev'}
      </button>
      <span className="min-w-[76px] text-center text-[12px] text-white/45">
        {`Page ${page + 1}`}
      </span>
      <button
        type="button"
        disabled={!hasMore || isFetching}
        onClick={() => onPage(page + 1)}
        className={buttonClass}
      >
        {'Next'}
        <ChevronRight size={14} />
      </button>
      <span className="flex size-4 items-center justify-center">
        {isFetching && <Loader2 size={14} className="animate-spin text-white/25" />}
      </span>
    </div>
  );
}
