import { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, LinkIcon as Link } from '../../lib/icons';

function cleanPermalink(url: string): string {
  try {
    const u = new URL(url);
    u.searchParams.delete('utm_medium');
    u.searchParams.delete('utm_campaign');
    u.searchParams.delete('utm_source');
    const clean = u.toString();
    return clean.endsWith('?') ? clean.slice(0, -1) : clean;
  } catch {
    return url;
  }
}

export function CopyLinkButton({
  url,
  size = 'md',
}: {
  url: string | undefined;
  size?: 'sm' | 'md';
}) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);

  const handleCopy = useCallback(() => {
    if (!url) return;
    navigator.clipboard.writeText(cleanPermalink(url));
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }, [url]);

  if (!url) return null;

  const iconSize = size === 'sm' ? 13 : 15;

  return (
    <button
      type="button"
      onClick={handleCopy}
      className={`inline-flex items-center gap-1.5 font-medium transition-colors cursor-pointer rounded-md ${
        copied ? 'text-emerald-400' : 'text-white/60 hover:text-white hover:bg-white/[0.06]'
      } ${size === 'sm' ? 'h-9 px-2.5 text-[11px]' : 'h-10 px-3 text-[12px]'}`}
    >
      {copied ? <Check size={iconSize} className="text-emerald-400" /> : <Link size={iconSize} />}
      {copied ? t('auth.copied') : t('auth.copyLink')}
    </button>
  );
}
