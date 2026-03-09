import { Check, Link } from 'lucide-react';
import { useCallback, useState } from 'react';

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
      className={`inline-flex items-center gap-1.5 font-medium transition-all duration-300 ease-[var(--ease-apple)] cursor-pointer rounded-xl border ${
        copied
          ? 'bg-emerald-500/10 border-emerald-500/20 text-emerald-400'
          : 'bg-white/[0.04] border-white/[0.06] text-white/50 hover:bg-white/[0.08] hover:text-white/80 hover:border-white/[0.1]'
      } ${size === 'sm' ? 'px-3 py-1.5 text-[11px]' : 'px-4 py-2.5 text-[12px]'}`}
    >
      {copied ? <Check size={iconSize} className="text-emerald-400" /> : <Link size={iconSize} />}
      {copied ? 'Copied!' : 'Copy Link'}
    </button>
  );
}
