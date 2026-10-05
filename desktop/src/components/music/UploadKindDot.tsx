import React from 'react';

interface UploadKindDotProps {
  kind: string | null;
  className?: string;
}

const COLORS: Record<string, string> = {
  original: 'bg-emerald-400',
  demo: 'bg-sky-400',
  reupload: 'bg-amber-400',
  cover: 'bg-fuchsia-400',
};

const LABELS: Record<string, string> = {
  original: 'original',
  demo: 'demo',
  alt: 'alt',
  reupload: 're-upload',
  cover: 'cover',
};

export const UploadKindDot = React.memo(function UploadKindDot({
  kind,
  className,
}: UploadKindDotProps) {
  if (!kind) return null;
  const color = COLORS[kind];
  if (!color) return null;
  return (
    <span
      title={LABELS[kind] ?? kind}
      className={`inline-block w-1.5 h-1.5 rounded-full ${color} ${className ?? ''}`}
    />
  );
});
