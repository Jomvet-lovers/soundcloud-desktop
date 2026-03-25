export interface NewsItem {
  id: string;
  /** Optional image URL (artwork, banner, etc.) */
  image?: string;
  /** i18n key for the toast title */
  titleKey: string;
  /** i18n key for the toast short description */
  descriptionKey: string;
  /** i18n key for the full modal body */
  bodyKey: string;
  /** Accent color override (tailwind class, e.g. 'violet' | 'amber' | 'sky') */
  accent?: string;
}

/**
 * All news items, newest first.
 * Add new entries at the top. Once irrelevant, remove them.
 */
export const NEWS: NewsItem[] = [
  {
    id: 'artist-pro-oauth-2025-03',
    titleKey: 'news.artistPro.title',
    descriptionKey: 'news.artistPro.description',
    bodyKey: 'news.artistPro.body',
    accent: 'violet',
  },
];
