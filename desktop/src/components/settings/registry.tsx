import type { ReactNode } from 'react';
import { Database, Eye, Globe, Headphones, Link, User } from '../../lib/icons';
import { AccountCard } from './cards/AccountCard';
import { AudioDeviceCard } from './cards/AudioDeviceCard';
import { CacheCard } from './cards/CacheCard';
import { DiscordCard } from './cards/DiscordCard';
import { PlaybackCard } from './cards/PlaybackCard';
import { StartupCard } from './cards/StartupCard';
import { ThemeCard } from './cards/ThemeCard';
import { WallpaperCard } from './cards/WallpaperCard';

export type SettingsCategoryId =
  | 'general'
  | 'appearance'
  | 'audio'
  | 'integrations'
  | 'storage'
  | 'account';

export interface SettingsCategory {
  id: SettingsCategoryId;
  label: string;
  icon: ReactNode;
  Body: () => ReactNode;
}

/** The settings map — one entry per left-rail category, each composing small cards. */
export const SETTINGS_CATEGORIES: SettingsCategory[] = [
  {
    id: 'general',
    label: 'General',
    icon: <Globe size={17} />,
    Body: () => (
      <>
        <StartupCard />
      </>
    ),
  },
  {
    id: 'appearance',
    label: 'Appearance',
    icon: <Eye size={17} />,
    Body: () => (
      <>
        <ThemeCard />
        <WallpaperCard />
      </>
    ),
  },
  {
    id: 'audio',
    label: 'Audio',
    icon: <Headphones size={17} />,
    Body: () => (
      <>
        <PlaybackCard />
        <AudioDeviceCard />
      </>
    ),
  },
  {
    id: 'integrations',
    label: 'Integrations',
    icon: <Link size={17} />,
    Body: () => (
      <>
        <DiscordCard />
      </>
    ),
  },
  {
    id: 'storage',
    label: 'Storage',
    icon: <Database size={17} />,
    Body: () => (
      <>
        <CacheCard />
      </>
    ),
  },
  {
    id: 'account',
    label: 'Account',
    icon: <User size={17} />,
    Body: () => <AccountCard />,
  },
];
