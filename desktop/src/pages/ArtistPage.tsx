import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useParams } from 'react-router-dom';
import { ArtistAboutTab } from '../components/artist/ArtistAboutTab';
import { ArtistAlbumsTab } from '../components/artist/ArtistAlbumsTab';
import { ArtistHero } from '../components/artist/ArtistHero';
import { ArtistRelatedTab } from '../components/artist/ArtistRelatedTab';
import { ArtistTracksTab, type TracksView } from '../components/artist/ArtistTracksTab';
import type { ArtistTabId, TracksSort } from '../components/artist/types';
import { useArtistDetail, useArtistStar } from '../components/artist/useArtistData';
import { AuraField } from '../components/user/AuraField';
import { USER_PAGE_KEYFRAMES } from '../components/user/keyframes';
import { type TabDescriptor, TabDock } from '../components/user/TabDock';
import { Loader2 } from '../lib/icons';

export function ArtistPage() {
  const { id } = useParams<{ id: string }>();
  const { t } = useTranslation();

  const detail = useArtistDetail(id);
  const artist = detail.data;

  const { hasStar, aura } = useArtistStar(id);

  const [tab, setTab] = useState<ArtistTabId>('tracks');
  const [primarySort, setPrimarySort] = useState<TracksSort>('popular');
  const [featuredSort, setFeaturedSort] = useState<TracksSort>('popular');
  const [primaryView, setPrimaryView] = useState<TracksView>('list');
  const [featuredView, setFeaturedView] = useState<TracksView>('list');

  const tabs = useMemo<ReadonlyArray<TabDescriptor<ArtistTabId>>>(() => {
    if (!artist) return [];
    const out: TabDescriptor<ArtistTabId>[] = [
      { id: 'tracks', label: t('artist.tracks'), count: artist.track_count_primary },
    ];
    if (artist.track_count_featured > 0) {
      out.push({
        id: 'appears',
        label: t('artist.appearsOn'),
        count: artist.track_count_featured,
      });
    }
    out.push({ id: 'albums', label: t('artist.albums'), count: artist.album_count });
    out.push({
      id: 'related',
      label: t('artist.related'),
      count: artist.related_artists.length,
    });
    out.push({ id: 'about', label: t('artist.about'), count: undefined });
    return out;
  }, [artist, t]);

  if (detail.isLoading || (!artist && !detail.error)) {
    return (
      <div className="relative w-full min-h-screen flex items-center justify-center">
        <Loader2 size={28} className="text-white/30 animate-spin" />
      </div>
    );
  }

  if (detail.error || !artist) {
    return (
      <div className="relative w-full min-h-screen flex items-center justify-center text-white/40 text-sm">
        {t('common.error')}
      </div>
    );
  }

  return (
    <>
      <style>{USER_PAGE_KEYFRAMES}</style>
      <div className="relative w-full min-h-screen">
        <AuraField aura={aura} isStar={hasStar} />

        <div
          className="relative z-10 w-full max-w-[1480px] mx-auto px-4 md:px-8 pt-10 md:pt-16 pb-32"
          style={{ isolation: 'isolate' }}
        >
          <ArtistHero
            artist={artist}
            hasStar={hasStar}
            aura={aura}
            popularTracks={artist.popular_tracks}
          />

          <div className="mt-10 mb-8">
            <TabDock<ArtistTabId> tabs={tabs} active={tab} onChange={setTab} aura={aura} />
          </div>

          <div
            className="rounded-[2rem] p-3 md:p-5"
            style={{
              background:
                'linear-gradient(180deg, rgba(255,255,255,0.035) 0%, rgba(255,255,255,0.015) 100%)',
              backdropFilter: 'blur(28px) saturate(160%)',
              WebkitBackdropFilter: 'blur(28px) saturate(160%)',
              boxShadow:
                '0 30px 80px rgba(0,0,0,0.30), inset 0 0 0 1px rgba(255,255,255,0.06), inset 0 1px 0 rgba(255,255,255,0.05)',
            }}
          >
            {tab === 'tracks' && (
              <ArtistTracksTab
                artistId={artist.id}
                role="primary"
                aura={aura}
                sort={primarySort}
                onSortChange={setPrimarySort}
                view={primaryView}
                onViewChange={setPrimaryView}
              />
            )}
            {tab === 'appears' && (
              <ArtistTracksTab
                artistId={artist.id}
                role="featured"
                aura={aura}
                sort={featuredSort}
                onSortChange={setFeaturedSort}
                view={featuredView}
                onViewChange={setFeaturedView}
              />
            )}
            {tab === 'albums' && <ArtistAlbumsTab artistId={artist.id} aura={aura} />}
            {tab === 'related' && <ArtistRelatedTab related={artist.related_artists} aura={aura} />}
            {tab === 'about' && <ArtistAboutTab artist={artist} aura={aura} />}
          </div>
        </div>
      </div>
    </>
  );
}
