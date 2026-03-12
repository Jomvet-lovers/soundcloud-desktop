import React from 'react';
import { useTranslation } from 'react-i18next';
import { NavLink } from 'react-router-dom';
import { Globe, Home, Library, Search, Settings } from '../../lib/icons';
import { useAuthStore } from '../../stores/auth';
import { Avatar } from '../ui/Avatar';

const languages = [
  { code: 'en', label: 'English' },
  { code: 'ru', label: 'Русский' },
] as const;

const navItems = [
  { to: '/', icon: Home, label: 'nav.home' },
  { to: '/search', icon: Search, label: 'nav.search' },
  { to: '/library', icon: Library, label: 'nav.library' },
];

export const Sidebar = React.memo(() => {
  const { t, i18n } = useTranslation();
  const user = useAuthStore((s) => s.user);

  const toggleLanguage = () => {
    const next = i18n.language === 'ru' ? 'en' : 'ru';
    i18n.changeLanguage(next);
  };

  const currentLang = languages.find((l) => l.code === i18n.language) ?? languages[0];

  return (
    <aside className="w-[200px] shrink-0 flex flex-col h-full border-r border-white/[0.04]">
      <nav className="flex flex-col gap-0.5 px-3 pt-2">
        {navItems.map((item) => (
          <NavLink
            key={item.to}
            to={item.to}
            className={({ isActive }) =>
              `flex items-center gap-3 px-3 py-2.5 rounded-xl text-[13px] font-medium transition-all duration-200 ease-[var(--ease-apple)] ${
                isActive
                  ? 'text-white bg-white/[0.07] shadow-[inset_0_0.5px_0_rgba(255,255,255,0.1)]'
                  : 'text-white/40 hover:text-white/70 hover:bg-white/[0.04]'
              }`
            }
          >
            <item.icon size={18} strokeWidth={1.8} />
            {t(item.label)}
          </NavLink>
        ))}
      </nav>

      <div className="flex-1" />

      <div className="px-3 pb-1 flex flex-col gap-0.5">
        <button
          type="button"
          onClick={toggleLanguage}
          className="flex items-center gap-2.5 w-full px-3 py-2 rounded-xl text-[12px] font-medium text-white/40 hover:text-white/70 hover:bg-white/[0.04] transition-all duration-200 cursor-pointer"
        >
          <Globe size={16} strokeWidth={1.8} />
          <span className="truncate">{currentLang.label}</span>
        </button>
        <NavLink
          to="/settings"
          className={({ isActive }) =>
            `flex items-center gap-2.5 w-full px-3 py-2 rounded-xl text-[12px] font-medium transition-all duration-200 ${
              isActive
                ? 'text-white/70 bg-white/[0.07]'
                : 'text-white/40 hover:text-white/70 hover:bg-white/[0.04]'
            }`
          }
        >
          <Settings size={16} strokeWidth={1.8} />
          <span className="truncate">{t('nav.settings')}</span>
        </NavLink>
      </div>

      {user && (
        <div className="px-3 pb-3">
          <NavLink
            to={`/user/${encodeURIComponent(user.urn)}`}
            className={({ isActive }) =>
              `flex items-center gap-2.5 px-3 py-2.5 rounded-xl transition-all duration-200 cursor-pointer ${
                isActive
                  ? 'bg-white/[0.07] shadow-[inset_0_0.5px_0_rgba(255,255,255,0.1)]'
                  : 'hover:bg-white/[0.04]'
              }`
            }
          >
            <Avatar src={user.avatar_url} alt={user.username} size={26} />
            <span className="text-[12px] text-white/40 truncate font-medium">{user.username}</span>
          </NavLink>
        </div>
      )}
    </aside>
  );
});
