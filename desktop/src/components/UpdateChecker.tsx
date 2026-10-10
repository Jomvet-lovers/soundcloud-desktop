import { openUrl } from '@tauri-apps/plugin-opener';
import { useMemo } from 'react';
import { proxiedAssetUrl } from '../lib/asset-url';
import { APP_VERSION } from '../lib/constants';
import { ExternalLink, X } from '../lib/icons';
import type { GithubRelease } from '../lib/update-check';

function stripLeadingV(version: string) {
  return version.replace(/^v/, '');
}

function renderInlineMarkdown(text: string, keyPrefix: string) {
  const parts: React.ReactNode[] = [];
  const pattern = /!\[([^\]]*)\]\(([^)]+)\)|\[([^\]]+)\]\(([^)]+)\)/g;
  let lastIndex = 0;
  let match: RegExpExecArray | null = null;
  let matchIndex = 0;

  while ((match = pattern.exec(text)) !== null) {
    if (match.index > lastIndex) {
      parts.push(text.slice(lastIndex, match.index));
    }

    const [, imageAlt, imageUrl, linkLabel, linkUrl] = match;
    if (imageUrl) {
      parts.push(
        <img
          key={`${keyPrefix}-img-${matchIndex}`}
          src={proxiedAssetUrl(imageUrl) ?? imageUrl}
          alt={imageAlt || ''}
          loading="lazy"
          decoding="async"
          className="mt-2 rounded-lg border border-white/[0.08] max-w-full"
        />,
      );
    } else if (linkUrl) {
      parts.push(
        <button
          key={`${keyPrefix}-link-${matchIndex}`}
          type="button"
          onClick={() => openUrl(linkUrl)}
          className="inline text-accent hover:underline cursor-pointer"
        >
          {linkLabel}
        </button>,
      );
    }

    lastIndex = pattern.lastIndex;
    matchIndex += 1;
  }

  if (lastIndex < text.length) {
    parts.push(text.slice(lastIndex));
  }

  return parts;
}

function renderReleaseBody(body: string) {
  const lines = body.split(/\r?\n/);
  const nodes: React.ReactNode[] = [];

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    const trimmed = line.trim();
    if (!trimmed) {
      nodes.push(<div key={index} className="h-3" />);
      continue;
    }
    if (trimmed.startsWith('### ')) {
      nodes.push(
        <h4 key={index} className="text-[13px] font-semibold text-white/80 mt-3 first:mt-0">
          {renderInlineMarkdown(trimmed.slice(4), `h4-${index}`)}
        </h4>,
      );
      continue;
    }
    if (trimmed.startsWith('## ') || trimmed.startsWith('# ')) {
      nodes.push(
        <h3 key={index} className="text-[14px] font-semibold text-white/85 mt-3 first:mt-0">
          {renderInlineMarkdown(trimmed.replace(/^#+\s*/, ''), `h3-${index}`)}
        </h3>,
      );
      continue;
    }
    if (trimmed.startsWith('- ') || trimmed.startsWith('* ')) {
      nodes.push(
        <p key={index} className="text-[12px] leading-relaxed text-white/60 pl-3">
          {'\u2022'} {renderInlineMarkdown(trimmed.slice(2), `li-${index}`)}
        </p>,
      );
      continue;
    }

    const calloutMatch = trimmed.match(/^>\s*\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\]\s*(.*)$/);
    if (calloutMatch) {
      const [, kind, firstLine] = calloutMatch;
      const bodyLines: string[] = [];
      if (firstLine) bodyLines.push(firstLine);

      while (index + 1 < lines.length) {
        const nextLine = lines[index + 1];
        if (!nextLine.trim().startsWith('>')) break;
        index += 1;
        bodyLines.push(nextLine.trim().replace(/^>\s?/, ''));
      }

      nodes.push(
        <div
          key={index}
          className="border-t border-white/[0.08] mt-2 pt-2 first:mt-0 first:border-t-0 first:pt-0"
        >
          <p className="text-[11px] font-semibold text-white/40">{kind}</p>
          <div className="mt-1 space-y-1 text-[12px] leading-relaxed text-white/60">
            {bodyLines.map((calloutLine, calloutIndex) => (
              <p key={`${index}-${calloutIndex}`}>
                {renderInlineMarkdown(calloutLine, `callout-${index}-${calloutIndex}`)}
              </p>
            ))}
          </div>
        </div>,
      );
      continue;
    }

    nodes.push(
      <p key={index} className="text-[12px] leading-relaxed text-white/60 whitespace-pre-wrap">
        {renderInlineMarkdown(trimmed, `p-${index}`)}
      </p>,
    );
  }

  return nodes;
}

export function UpdateChecker({
  release,
  onDismiss,
}: {
  release: GithubRelease;
  onDismiss: () => void;
}) {
  const renderedNotes = useMemo(() => renderReleaseBody(release.body), [release.body]);

  if (!release) return null;

  return (
    <div className="fixed inset-0 z-[9999] flex items-center justify-center bg-black">
      <div className="relative w-full max-w-md mx-4 rounded-2xl bg-[#1a1a1e] border border-white/[0.12] overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between px-5 pt-5 pb-4">
          <div>
            <h2 className="text-sm font-semibold">{'Update available'}</h2>
            <p className="text-[11px] text-white/30 mt-0.5">
              {stripLeadingV(APP_VERSION)} → {stripLeadingV(release.tag_name)}
            </p>
          </div>
          <button
            type="button"
            onClick={onDismiss}
            className="w-7 h-7 rounded-lg bg-white/[0.05] hover:bg-white/[0.08] flex items-center justify-center transition-colors cursor-pointer"
          >
            <X size={14} className="text-white/40" />
          </button>
        </div>

        {/* Release title */}
        {release.name && (
          <div className="px-5 pb-3 border-t border-white/[0.08] pt-3">
            <p className="text-[13px] font-medium text-white/80">{release.name}</p>
          </div>
        )}

        {/* Release notes */}
        {release.body && (
          <div className="selectable mx-5 mb-4 max-h-60 overflow-y-auto border-t border-white/[0.08] pt-3 space-y-1">
            {renderedNotes}
          </div>
        )}

        {/* Actions */}
        <div className="px-5 py-4 border-t border-white/[0.08]">
          <div className="flex gap-2">
            <button
              type="button"
              onClick={onDismiss}
              className="flex-1 py-2.5 rounded-xl bg-white/[0.05] hover:bg-white/[0.08] text-[13px] text-white/50 font-medium transition-colors cursor-pointer"
            >
              {'Later'}
            </button>
            <button
              type="button"
              onClick={() => openUrl(release.html_url)}
              className="flex-1 py-2.5 rounded-xl bg-accent hover:bg-accent-hover text-[13px] text-accent-contrast font-semibold transition-colors cursor-pointer flex items-center justify-center gap-1.5"
            >
              {'Release page'}
              <ExternalLink size={13} />
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
