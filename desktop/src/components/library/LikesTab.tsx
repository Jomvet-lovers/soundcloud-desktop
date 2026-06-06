import React, {useEffect, useMemo} from 'react';
import {useTranslation} from 'react-i18next';
import {fetchAllLikedTracks, useInfiniteScroll, useLikedTracks} from '../../lib/hooks';
import {Loader2} from '../../lib/icons';
import {usePlayerStore} from '../../stores/player';
import {VirtualList} from '../ui/VirtualList';
import {LibraryTrackRow} from './LibraryTrackRow';

export const LikesTab = React.memo(function LikesTab({filter}: { filter: string }) {
    const {t} = useTranslation();
    const likesQuery = useLikedTracks();
    const {tracks: likedTracks, isLoading} = likesQuery;
    const sentinelRef = useInfiniteScroll(
        !!likesQuery.hasNextPage,
        !!likesQuery.isFetchingNextPage,
        likesQuery.fetchNextPage,
    );

    // Auto-fetch remaining pages when filtering
    useEffect(() => {
        if (filter && likesQuery.hasNextPage && !likesQuery.isFetchingNextPage) {
            likesQuery.fetchNextPage();
        }
    }, [filter, likesQuery.hasNextPage, likesQuery.isFetchingNextPage]);

    const filtered = useMemo(() => {
        if (!filter) return likedTracks;
        const q = filter.toLowerCase();
        return likedTracks.filter(
            (tr) => tr.title.toLowerCase().includes(q) || tr.user.username.toLowerCase().includes(q),
        );
    }, [likedTracks, filter]);

    const expandQueue = React.useCallback(() => {
        const seen = new Set<string>(usePlayerStore.getState().queue.map((tr) => tr.urn));
        fetchAllLikedTracks(200, (page) => {
            const fresh = page.filter((tr) => !seen.has(tr.urn));
            for (const tr of fresh) seen.add(tr.urn);
            if (fresh.length > 0) usePlayerStore.getState().addToQueue(fresh);
        }).catch(() => {
        });
    }, []);

    return (
        <div className="min-h-[400px]">
            <div className="flex flex-col gap-1">
                {isLoading ? (
                    <div className="flex justify-center py-20">
                        <Loader2 size={32} className="animate-spin text-white/20"/>
                    </div>
                ) : filtered.length > 0 ? (
                    <VirtualList
                        items={filtered}
                        rowHeight={68}
                        overscan={8}
                        className="flex flex-col gap-1"
                        disabled={filtered.length < 40}
                        getItemKey={(track) => track.urn}
                        renderItem={(track, i) => (
                            <LibraryTrackRow track={track} index={i} queue={filtered} onPlay={expandQueue}/>
                        )}
                    />
                ) : (
                    <div className="py-20 text-center text-white/20">
                        {filter && likesQuery.hasNextPage
                            ? t('common.loading')
                            : filter
                                ? t('library.noMatches')
                                : t('library.noLikedTracks')}
                    </div>
                )}
            </div>
            {!filter ? (
                <div ref={sentinelRef} className="h-12 flex items-center justify-center mt-4">
                    {likesQuery.isFetchingNextPage && (
                        <Loader2 size={20} className="text-white/15 animate-spin"/>
                    )}
                </div>
            ) : likesQuery.isFetchingNextPage ? (
                <div className="h-12 flex items-center justify-center mt-4">
                    <Loader2 size={20} className="text-white/15 animate-spin"/>
                </div>
            ) : null}
        </div>
    );
});
