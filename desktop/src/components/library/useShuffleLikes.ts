import {useCallback, useState} from 'react';
import {fetchAllLikedTracks, useLikedTracks} from '../../lib/hooks';
import {armLikesContinuation} from '../../lib/queue-continuation';
import {usePlayerStore} from '../../stores/player';

function pickRandom<T>(arr: T[]): T {
    return arr[Math.floor(Math.random() * arr.length)];
}

/** Shuffle-play the whole liked collection. Start instantly off the loaded page,
 *  then let the queue-continuation source stream the rest of the likes in —
 *  shuffled across the ENTIRE collection, not just the loaded page (см.
 *  lib/queue-continuation.ts → createShuffledLikesContinuationSource). Shared so
 *  the masthead and any other "play everything" entry point behave identically. */
export function useShuffleLikes() {
    const {tracks: likedTracks} = useLikedTracks();
    const [loading, setLoading] = useState(false);

    const shuffle = useCallback(async () => {
        if (loading) return;
        usePlayerStore.setState({shuffle: true});
        const {play} = usePlayerStore.getState();

        // Есть подгруженная страница → стартуем мгновенно со случайного из неё;
        // play() под shuffle перемешает её, а прослойка дотянет ВЕСЬ остаток
        // лайков перемешанным по мере опустошения очереди.
        if (likedTracks.length > 0) {
            play(pickRandom(likedTracks), likedTracks);
            armLikesContinuation();
            return;
        }

        // Ничего не подгружено (редкий случай — masthead до загрузки лайков) →
        // тянем список (заодно греем общий кеш для прослойки), стартуем с одного
        // случайного трека, остальное дотянет перемешанная прослойка.
        setLoading(true);
        try {
            const all = await fetchAllLikedTracks();
            if (all.length === 0) return;
            const start = pickRandom(all);
            play(start, [start]);
            armLikesContinuation();
        } finally {
            setLoading(false);
        }
    }, [likedTracks, loading]);

    return {shuffle, loading};
}
