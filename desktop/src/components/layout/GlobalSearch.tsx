import { memo, useEffect, useRef, useState } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import { Clock, Search as SearchIcon, X } from '../../lib/icons';
import { isMac } from '../../lib/platform';
import { useSearchHistoryStore } from '../../stores/searchHistory';
import { useSearchQueryStore } from '../../stores/searchQuery';
import { isSoundCloudUrl } from '../search/utils';

/* The one global search field — lives in the titlebar, present on every page.
 * Writes the shared query store and routes to /search; the Search page reads
 * that store, so there's a single search input app-wide. Flat lens, ⌘K hint,
 * recent-search dropdown. */
export const GlobalSearch = memo(function GlobalSearch() {
  const navigate = useNavigate();
  const location = useLocation();
  const q = useSearchQueryStore((s) => s.q);
  const setQ = useSearchQueryStore((s) => s.setQ);
  const history = useSearchHistoryStore((s) => s.queries);
  const removeQuery = useSearchHistoryStore((s) => s.removeQuery);
  const clearHistory = useSearchHistoryStore((s) => s.clearHistory);

  const [focused, setFocused] = useState(false);
  const blurTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const isUrl = isSoundCloudUrl(q);
  const showHistory = focused && q.trim() === '' && history.length > 0;

  useEffect(
    () => () => {
      if (blurTimer.current) clearTimeout(blurTimer.current);
    },
    [],
  );

  const goSearch = () => {
    if (location.pathname !== '/search') navigate('/search');
  };
  const change = (v: string) => {
    setQ(v);
    if (v) goSearch();
  };
  const pick = (value: string) => {
    setQ(value);
    goSearch();
  };

  return (
    <div className="relative w-full max-w-[600px]" style={{ isolation: 'isolate' }}>
      <div
        className="relative flex items-center gap-2.5 h-11 pl-4 pr-2 rounded-full overflow-hidden"
        style={{
          background: 'rgba(28,28,34,0.9)',
          border: `0.5px solid ${focused ? 'var(--color-accent)' : 'rgba(255,255,255,0.12)'}`,
          boxShadow: focused ? '0 10px 34px rgba(0,0,0,0.4)' : '0 6px 20px rgba(0,0,0,0.28)',
          transition: 'border-color 300ms ease',
        }}
      >
        <SearchIcon
          size={17}
          className="shrink-0 transition-colors duration-300"
          style={{ color: focused ? 'var(--color-accent)' : 'rgba(255,255,255,0.45)' }}
        />
        <input
          id="global-search-input"
          value={q}
          onChange={(e) => change(e.target.value)}
          onFocus={() => {
            if (blurTimer.current) clearTimeout(blurTimer.current);
            setFocused(true);
          }}
          onBlur={() => {
            blurTimer.current = setTimeout(() => setFocused(false), 150);
          }}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              const value = q.trim();
              // `#tag` opens the tag feed instead of a text search.
              if (value.startsWith('#') && value.length > 1) {
                navigate(`/tag/${encodeURIComponent(value.slice(1).trim())}`);
              } else {
                goSearch();
              }
              (e.currentTarget as HTMLInputElement).blur();
            } else if (e.key === 'Escape') {
              (e.currentTarget as HTMLInputElement).blur();
            }
          }}
          placeholder={'What do you want to hear?'}
          spellCheck={false}
          autoComplete="off"
          className="flex-1 min-w-0 bg-transparent outline-none text-[14px] text-white/90 placeholder:text-white/35 select-text"
        />
        {isUrl && (
          <span className="shrink-0 text-[10px] text-accent/90 px-2 py-0.5 rounded-full bg-accent/10 border border-accent/20">
            {'Press Enter to open link'}
          </span>
        )}
        {q ? (
          <button
            type="button"
            onClick={() => setQ('')}
            className="shrink-0 w-7 h-7 flex items-center justify-center rounded-full text-white/40 hover:text-white/80 hover:bg-white/5 transition-colors cursor-pointer"
            aria-label={'Clear'}
          >
            <X size={15} />
          </button>
        ) : (
          !focused && (
            <kbd className="shrink-0 hidden sm:flex items-center gap-0.5 h-6 px-2 mr-1 rounded-md text-[10px] font-semibold text-white/30 bg-white/[0.05] border border-white/10">
              {isMac() ? '⌘' : 'Ctrl'} K
            </kbd>
          )
        )}
      </div>

      {showHistory && (
        <div
          className="absolute left-0 right-0 mt-2 p-1.5 rounded-2xl overflow-hidden"
          style={{
            background: '#101014',
            border: '0.5px solid rgba(255,255,255,0.1)',
            boxShadow: '0 24px 60px rgba(0,0,0,0.55)',
          }}
        >
          <div className="flex items-center justify-between px-2.5 py-1.5">
            <span className="text-[11px] text-white/35">{'Recent searches'}</span>
            <button
              type="button"
              onMouseDown={(e) => {
                e.preventDefault();
                clearHistory();
              }}
              className="text-[11px] text-white/35 hover:text-white/70 transition-colors cursor-pointer"
            >
              {'Clear all'}
            </button>
          </div>
          {history.slice(0, 8).map((item) => (
            <div
              key={item}
              className="group/h flex items-center gap-2.5 px-2.5 py-2 rounded-xl hover:bg-white/[0.06] transition-colors cursor-pointer"
              onMouseDown={(e) => {
                e.preventDefault();
                pick(item);
              }}
            >
              <Clock size={13} className="shrink-0 text-white/25" />
              <span className="flex-1 min-w-0 truncate text-[13px] text-white/70">{item}</span>
              <button
                type="button"
                onMouseDown={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  removeQuery(item);
                }}
                className="shrink-0 w-6 h-6 flex items-center justify-center rounded-full text-white/0 group-hover/h:text-white/40 hover:!text-white/80 transition-colors"
                aria-label={'Clear'}
              >
                <X size={13} />
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
});
