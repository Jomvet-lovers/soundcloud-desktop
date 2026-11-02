import type { Track } from '../stores/player';
import { ApiError, api } from './api';
import type { PagedResponse } from './hooks';

type RelatedTracks = PagedResponse<Track>;

const emptyRelated = (page: number, limit: number): RelatedTracks => ({
  collection: [],
  page,
  page_size: limit,
  has_more: false,
});

/**
 * Related tracks for a seed track. A 404 from the backend means "no neighbors yet",
 * not an error: return an empty page — no toast, no throw. Other errors propagate.
 */
export async function fetchRelatedTracks(
  urn: string,
  limit = 10,
  page = 0,
): Promise<RelatedTracks> {
  try {
    return await api<RelatedTracks>(
      `/tracks/${encodeURIComponent(urn)}/related?limit=${limit}&page=${page}`,
      { silentStatuses: [404] },
    );
  } catch (e) {
    if (e instanceof ApiError && (e.status === 404 || e.status === 403 || e.status === 502))
      return emptyRelated(page, limit);
    throw e;
  }
}
