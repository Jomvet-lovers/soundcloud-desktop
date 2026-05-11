import { useQuery } from '@tanstack/react-query';
import { useMemo } from 'react';
import { api } from '../../lib/api';
import { type Aura, DEFAULT_AURA, resolveAura } from '../../lib/aura';
import type { Track } from '../../stores/player';
import type { ArtistAlbum, ArtistDetail, TracksSort } from './types';

const STALE_DETAIL = 60_000;
const STALE_TRACKS = 30_000;
const STALE_ALBUMS = 120_000;
const STALE_STAR = 5 * 60_000;
const GC_STAR = 10 * 60_000;

export function useArtistDetail(id: string | undefined) {
  return useQuery({
    queryKey: ['artist', id],
    queryFn: () => api<ArtistDetail>(`/artists/${encodeURIComponent(id!)}`),
    enabled: !!id,
    staleTime: STALE_DETAIL,
  });
}

export function useArtistTracks(
  id: string | undefined,
  role: 'primary' | 'featured',
  sort: TracksSort,
) {
  return useQuery({
    queryKey: ['artist', id, 'tracks', role, sort],
    queryFn: () =>
      api<{ collection: Track[] }>(
        `/artists/${encodeURIComponent(id!)}/tracks?role=${role}&sort=${sort}&limit=80`,
      ),
    enabled: !!id,
    staleTime: STALE_TRACKS,
    select: (d) => d.collection,
  });
}

export function useArtistAlbums(id: string | undefined) {
  return useQuery({
    queryKey: ['artist', id, 'albums'],
    queryFn: () => api<ArtistAlbum[]>(`/artists/${encodeURIComponent(id!)}/albums`),
    enabled: !!id,
    staleTime: STALE_ALBUMS,
  });
}

type ArtistStarResponse = {
  premium: boolean;
  aura_id?: string | null;
  custom_hex?: string | null;
  source_sc_user_id?: string | null;
};

export interface ArtistStar {
  hasStar: boolean;
  aura: Aura;
}

export function useArtistStar(id: string | undefined): ArtistStar {
  const query = useQuery({
    queryKey: ['artist', id, 'star'],
    queryFn: () => api<ArtistStarResponse>(`/artists/${encodeURIComponent(id!)}/star`),
    enabled: !!id,
    staleTime: STALE_STAR,
    gcTime: GC_STAR,
  });

  return useMemo(() => {
    const data = query.data;
    if (!data?.premium) return { hasStar: false, aura: DEFAULT_AURA };
    return {
      hasStar: true,
      aura: resolveAura(data.aura_id, data.custom_hex),
    };
  }, [query.data]);
}
