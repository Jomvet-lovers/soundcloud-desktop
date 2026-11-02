import React from 'react';
import { Database, Search as SearchIcon, X } from '../../lib/icons';

interface UserSearchBoxProps {
  value: string;
  onChange: (v: string) => void;
  /**
   * Scope label: a hint like "searching tracks/playlists" so the user
   * understands that results are limited to the current tab.
   */
  scopeLabel: string;
  /**
   * When true the input is disabled (the current tab doesn't support content
   * search, e.g. followers/following).
   */
  disabled?: boolean;
}

/**
 * Inline search over a specific user's content. Always hits our own database
 * (SC has no such query), hence the "DB" badge next to it. The badge tells the
 * user the cost of the request and why results may differ from the external
 * profile.
 */
function UserSearchBoxImpl({ value, onChange, scopeLabel, disabled }: UserSearchBoxProps) {
  return (
    <div className="relative w-full">
      <div className="absolute inset-y-0 left-3.5 flex items-center pointer-events-none">
        <SearchIcon size={16} className={disabled ? 'text-white/15' : 'text-white/35'} />
      </div>
      <input
        type="text"
        autoComplete="off"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        disabled={disabled}
        placeholder={disabled ? 'Search not available here' : `Search in their ${scopeLabel}…`}
        className={`w-full text-[13px] py-2.5 pl-10 pr-24 rounded-2xl outline-none border transition-all duration-300 ${
          disabled
            ? 'bg-white/[0.015] border-white/[0.03] text-white/30 placeholder:text-white/15 cursor-not-allowed'
            : 'bg-white/[0.03] hover:bg-white/[0.05] focus:bg-white/[0.06] border-white/[0.05] focus:border-accent/30 focus:ring-1 focus:ring-accent/30 text-white placeholder:text-white/30'
        }`}
      />
      <div className="absolute inset-y-0 right-2 flex items-center gap-1.5">
        {value && !disabled && (
          <button
            type="button"
            onClick={() => onChange('')}
            className="w-6 h-6 flex items-center justify-center rounded-full hover:bg-white/[0.08] text-white/30 hover:text-white/80 transition-all cursor-pointer"
            title={'Clear search'}
          >
            <X size={12} />
          </button>
        )}
        <div
          className="flex items-center gap-1 px-2 py-1 rounded-full text-[10px] font-medium text-white/40 bg-white/[0.04] border border-white/[0.05]"
          title={'Search inside the local SCD library — fast, scoped to mirrored content.'}
        >
          <Database size={9} />
          {'DB'}
        </div>
      </div>
    </div>
  );
}

export const UserSearchBox = React.memo(UserSearchBoxImpl);
