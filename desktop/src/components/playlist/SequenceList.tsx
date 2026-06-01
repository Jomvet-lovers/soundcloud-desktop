import {
    closestCenter,
    DndContext,
    type DragEndEvent,
    KeyboardSensor,
    PointerSensor,
    useSensor,
    useSensors,
} from '@dnd-kit/core';
import {SortableContext, verticalListSortingStrategy} from '@dnd-kit/sortable';
import React, {useMemo} from 'react';
import {useTranslation} from 'react-i18next';
import {Clock, ListMusic, Loader2} from '../../lib/icons';
import type {Track} from '../../stores/player';
import {VirtualList} from '../ui/VirtualList';
import {SequenceRow, SortableSequenceRow} from './SequenceRow';

const PANEL = {
    background: 'rgba(255,255,255,0.02)',
    border: '0.5px solid rgba(255,255,255,0.06)',
    boxShadow: '0 24px 60px rgba(0,0,0,0.28), inset 0 1px 0 rgba(255,255,255,0.04)',
} as const;

function Header({count}: { count: number }) {
    const {t} = useTranslation();
    return (
        <div className="flex items-center justify-between px-3 pt-1 pb-3">
      <span className="inline-flex items-center gap-2 text-[11px] font-bold uppercase tracking-[0.22em] text-white/55">
        <ListMusic size={12}/> {t('playlist.theSequence')}
          <span className="text-white/25 ml-1 tabular-nums">{count}</span>
      </span>
            <Clock size={12} className="text-white/25"/>
        </div>
    );
}

/** The Sequence — virtualized tracklist. Owner gets drag-to-reorder + remove;
 *  everyone gets play-on-hover, now-playing highlight and the genre hue-ticks. */
export const SequenceList = React.memo(function SequenceList({
                                                                 tracks,
                                                                 isOwner,
                                                                 onDragEnd,
                                                                 onRemove,
                                                                 sentinelRef,
                                                                 hasNextPage,
                                                                 isFetchingNextPage,
                                                             }: {
    tracks: Track[];
    isOwner: boolean;
    onDragEnd: (e: DragEndEvent) => void;
    onRemove: (urn: string) => void;
    sentinelRef: React.Ref<HTMLDivElement>;
    hasNextPage: boolean;
    isFetchingNextPage: boolean;
}) {
    const {t} = useTranslation();
    const sensors = useSensors(
        useSensor(PointerSensor, {activationConstraint: {distance: 5}}),
        useSensor(KeyboardSensor),
    );
    const ids = useMemo(() => tracks.map((tr) => tr.urn), [tracks]);

    if (tracks.length === 0) {
        return (
            <div className="py-20 flex flex-col items-center gap-4">
                <div
                    className="w-16 h-16 rounded-2xl flex items-center justify-center"
                    style={{
                        background: 'rgba(255,255,255,0.03)',
                        border: '0.5px solid rgba(255,255,255,0.06)',
                    }}
                >
                    <ListMusic size={24} className="text-white/15"/>
                </div>
                <p className="text-white/30 text-sm">{t('playlist.emptyCrate')}</p>
            </div>
        );
    }

    const sentinel = hasNextPage ? (
        <div ref={sentinelRef} className="flex justify-center py-4">
            {isFetchingNextPage && <Loader2 size={20} className="animate-spin text-white/30"/>}
        </div>
    ) : null;

    return (
        <div className="rounded-[2rem] p-3 md:p-4" style={PANEL}>
            <Header count={tracks.length}/>
            {isOwner ? (
                <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={onDragEnd}>
                    <SortableContext items={ids} strategy={verticalListSortingStrategy}>
                        <VirtualList
                            items={tracks}
                            rowHeight={68}
                            overscan={10}
                            className="space-y-0.5"
                            getItemKey={(tr) => tr.urn}
                            renderItem={(track, i) => (
                                <SortableSequenceRow track={track} index={i} queue={tracks} onRemove={onRemove}/>
                            )}
                        />
                    </SortableContext>
                </DndContext>
            ) : (
                <VirtualList
                    items={tracks}
                    rowHeight={68}
                    overscan={10}
                    className="space-y-0.5"
                    getItemKey={(tr) => tr.urn}
                    renderItem={(track, i) => <SequenceRow track={track} index={i} queue={tracks}/>}
                />
            )}
            {sentinel}
        </div>
    );
});
