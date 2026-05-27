import React from 'react';
import { useTranslation } from 'react-i18next';
import { Database, Search as SearchIcon, X } from '../../lib/icons';

interface UserSearchBoxProps {
  value: string;
  onChange: (v: string) => void;
  /**
   * Лейбл скоупа: подсказка "ищем по трекам/плейлистам", чтобы юзер понимал,
   * что результаты ограничены текущим табом.
   */
  scopeLabel: string;
  /**
   * Когда true — инпут disabled (текущий таб не поддерживает поиск по контенту,
   * например followers/following).
   */
  disabled?: boolean;
}

/**
 * Inline-поиск контента конкретного юзера. Всегда бьёт в нашу базу (на SC такой
 * выборки нет), поэтому рядом — бейдж "DB". Бейдж сообщает юзеру цену запроса
 * и почему результаты могут отличаться от внешнего профиля.
 */
function UserSearchBoxImpl({ value, onChange, scopeLabel, disabled }: UserSearchBoxProps) {
  const { t } = useTranslation();

  return (
    <div className="relative w-full">
      <div className="absolute inset-y-0 left-3.5 flex items-center pointer-events-none">
        <SearchIcon size={16} className={disabled ? 'text-white/15' : 'text-white/35'} />
      </div>
      <input
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        disabled={disabled}
        placeholder={
          disabled
            ? t('user.search.unavailable')
            : t('user.search.placeholder', { scope: scopeLabel })
        }
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
            title={t('user.search.clear')}
          >
            <X size={12} />
          </button>
        )}
        <div
          className="flex items-center gap-1 px-2 py-1 rounded-full text-[10px] uppercase tracking-widest font-semibold text-white/40 bg-white/[0.04] border border-white/[0.05]"
          title={t('search.source.dbHint')}
        >
          <Database size={9} />
          {t('search.source.dbBadge')}
        </div>
      </div>
    </div>
  );
}

export const UserSearchBox = React.memo(UserSearchBoxImpl);
