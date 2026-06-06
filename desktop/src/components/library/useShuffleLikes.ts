import {useCallback, useState} from 'react';
import {fetchAllLikedTracks, useLikedTracks} from '../../lib/hooks';
import {usePlayerStore} from '../../stores/player';

/** Shuffle-play the whole liked collection: start instantly off the loaded page,
 *  then stream the rest in as it arrives. Shared so the masthead and any other
 *  "play everything" entry point behave identically. */
export function useShuffleLikes() {
    const {tracks: likedTracks} = useLikedTracks();
    const [loading, setLoading] = useState(false);

    const shuffle = useCallback(async () => {
        if (loading) return;
        if (!usePlayerStore.getState().shuffle) usePlayerStore.setState({shuffle: true});

        const seen = new Set<string>();
        let started = false;

        if (likedTracks.length > 0) {
            for (const t of likedTracks) seen.add(t.urn);
            const random = likedTracks[Math.floor(Math.random() * likedTracks.length)];
            usePlayerStore.getState().play(random, likedTracks);
            started = true;
        } else {
            setLoading(true);
        }

        try {
            await fetchAllLikedTracks(200, (page) => {
                const fresh = page.filter((t) => !seen.has(t.urn));
                for (const t of fresh) seen.add(t.urn);
                if (fresh.length === 0) return;
                if (!started) {
                    const random = fresh[Math.floor(Math.random() * fresh.length)];
                    usePlayerStore.getState().play(random, fresh);
                    started = true;
                    setLoading(false);
                } else {
                    usePlayerStore.getState().addToQueue(fresh);
                }
            });
        } finally {
            setLoading(false);
        }
    }, [likedTracks, loading]);

    return {shuffle, loading};
}
