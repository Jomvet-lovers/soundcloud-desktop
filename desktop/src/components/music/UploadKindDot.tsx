import React from 'react';
import { useTranslation } from 'react-i18next';

interface UploadKindDotProps {
  kind: string | null;
  className?: string;
}

const COLORS: Record<string, string> = {
  original: 'bg-emerald-400',
  demo: 'bg-sky-400',
  reupload: 'bg-amber-400',
};

export const UploadKindDot = React.memo(function UploadKindDot({
  kind,
  className,
}: UploadKindDotProps) {
  const { t } = useTranslation();
  if (!kind) return null;
  const color = COLORS[kind];
  if (!color) return null;
  return (
    <span
      title={t(`track.uploadKind.${kind}`, kind)}
      className={`inline-block w-1.5 h-1.5 rounded-full ${color} ${className ?? ''}`}
    />
  );
});
