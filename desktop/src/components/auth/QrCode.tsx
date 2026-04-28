import QRCodeStyling, { type Options } from 'qr-code-styling';
import React, { useEffect, useMemo, useRef } from 'react';
import { useSettingsStore } from '../../stores/settings';

interface QrCodeProps {
  payload: string;
  size?: number;
  /** Optional logo URL or data URI to render in the centre. */
  logoUrl?: string;
}

/** Lighten a hex colour by mixing toward white. amount in [0,1]. */
function lighten(hex: string, amount: number): string {
  const m = hex.replace('#', '').match(/.{2}/g);
  if (!m) return hex;
  const [r, g, b] = m.map((h) => Number.parseInt(h, 16));
  const mix = (c: number) => Math.round(c + (255 - c) * amount);
  return `#${[mix(r), mix(g), mix(b)].map((v) => v.toString(16).padStart(2, '0')).join('')}`;
}

export const QrCode = React.memo(({ payload, size = 280, logoUrl }: QrCodeProps) => {
  const accent = useSettingsStore((s) => s.accentColor);
  const ref = useRef<HTMLDivElement>(null);
  const qrRef = useRef<QRCodeStyling | null>(null);

  const options = useMemo<Options>(() => {
    const accentLight = lighten(accent, 0.3);
    return {
      width: size,
      height: size,
      type: 'svg',
      data: payload,
      margin: 0,
      qrOptions: { errorCorrectionLevel: 'H' },
      image: logoUrl,
      imageOptions: {
        crossOrigin: 'anonymous',
        margin: 6,
        imageSize: 0.32,
        hideBackgroundDots: true,
      },
      backgroundOptions: { color: 'transparent' },
      dotsOptions: {
        type: 'rounded',
        gradient: {
          type: 'linear',
          rotation: Math.PI / 4,
          colorStops: [
            { offset: 0, color: accent },
            { offset: 1, color: accentLight },
          ],
        },
      },
      cornersSquareOptions: {
        type: 'extra-rounded',
        gradient: {
          type: 'linear',
          rotation: 0,
          colorStops: [
            { offset: 0, color: accent },
            { offset: 1, color: accentLight },
          ],
        },
      },
      cornersDotOptions: {
        type: 'dot',
        color: accent,
      },
    };
  }, [accent, payload, size, logoUrl]);

  useEffect(() => {
    if (!ref.current) return;
    if (!qrRef.current) {
      qrRef.current = new QRCodeStyling(options);
      qrRef.current.append(ref.current);
    } else {
      qrRef.current.update(options);
    }
  }, [options]);

  return (
    <div className="relative inline-flex items-center justify-center">
      {/* Glow */}
      <div
        aria-hidden
        className="absolute inset-0 rounded-3xl pointer-events-none"
        style={{
          background: `radial-gradient(circle at center, ${accent}33 0%, transparent 65%)`,
          filter: 'blur(24px)',
          transform: 'translateZ(0)',
        }}
      />
      {/* Frame */}
      <div
        className="relative rounded-2xl p-5"
        style={{
          background:
            'linear-gradient(165deg, rgba(255,255,255,0.92), rgba(255,255,255,0.96), rgba(255,255,255,1))',
          boxShadow:
            '0 18px 50px rgba(0,0,0,0.45), inset 0 1px 0 rgba(255,255,255,0.8), 0 0 0 1px rgba(0,0,0,0.04)',
        }}
      >
        <div ref={ref} style={{ width: size, height: size }} />
      </div>
    </div>
  );
});
QrCode.displayName = 'QrCode';
