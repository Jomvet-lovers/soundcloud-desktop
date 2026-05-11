import { memo } from 'react';
import {
  siApplemusic,
  siBandcamp,
  siDiscogs,
  siFacebook,
  siGenius,
  siInstagram,
  siLastdotfm,
  siMusicbrainz,
  siSoundcloud,
  siSpotify,
  siTiktok,
  siWikipedia,
  siX,
  siYoutube,
} from 'simple-icons';
import { ExternalLink, Globe, LinkIcon } from '../../lib/icons';

const Brand = memo(
  ({ d, size = 14, className }: { d: string; size?: number; className?: string }) => (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" className={className}>
      <path d={d} />
    </svg>
  ),
);

type Spec = { d?: string; lucide?: 'globe' | 'link' | 'external'; tone: string };

const NETWORKS: Record<string, Spec> = {
  instagram: { d: siInstagram.path, tone: '#e1306c' },
  twitter: { d: siX.path, tone: '#ffffff' },
  x: { d: siX.path, tone: '#ffffff' },
  youtube: { d: siYoutube.path, tone: '#ff0033' },
  soundcloud: { d: siSoundcloud.path, tone: '#ff5500' },
  spotify: { d: siSpotify.path, tone: '#1ed760' },
  apple_music: { d: siApplemusic.path, tone: '#fa57c1' },
  bandcamp: { d: siBandcamp.path, tone: '#629aa9' },
  tiktok: { d: siTiktok.path, tone: '#ffffff' },
  discogs: { d: siDiscogs.path, tone: '#dadada' },
  lastfm: { d: siLastdotfm.path, tone: '#d51007' },
  genius: { d: siGenius.path, tone: '#ffff64' },
  musicbrainz: { d: siMusicbrainz.path, tone: '#ba478f' },
  facebook: { d: siFacebook.path, tone: '#1877f2' },
  wikipedia: { d: siWikipedia.path, tone: '#ffffff' },
  personal: { lucide: 'globe', tone: '#ffffff' },
};

export function socialMeta(kind: string) {
  const k = kind.toLowerCase();
  return NETWORKS[k] ?? { lucide: 'link' as const, tone: '#ffffff' };
}

export function SocialIcon({
  kind,
  size = 14,
  className,
}: {
  kind: string;
  size?: number;
  className?: string;
}) {
  const meta = socialMeta(kind);
  if (meta.d) return <Brand d={meta.d} size={size} className={className} />;
  if (meta.lucide === 'globe') return <Globe size={size} className={className} />;
  if (meta.lucide === 'external') return <ExternalLink size={size} className={className} />;
  return <LinkIcon size={size} className={className} />;
}

export function socialLabel(kind: string) {
  const k = kind.toLowerCase();
  if (k === 'apple_music') return 'Apple Music';
  if (k === 'lastfm') return 'Last.fm';
  if (k === 'twitter' || k === 'x') return 'Twitter';
  if (k === 'musicbrainz') return 'MusicBrainz';
  if (!k) return 'Link';
  return k[0].toUpperCase() + k.slice(1);
}
