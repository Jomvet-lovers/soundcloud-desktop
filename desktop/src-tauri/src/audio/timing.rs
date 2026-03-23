use tauri::{Emitter, State};

use crate::audio::state::{AudioState, CommentsTimelineState, LyricsTimelineState};
use crate::audio::types::{FloatingCommentEvent, LyricsTimingLine};

pub fn audio_set_lyrics_timeline(lines: Vec<LyricsTimingLine>, state: State<'_, AudioState>) {
    let mut sorted = lines;
    sorted.sort_by(|a, b| a.time_secs.total_cmp(&b.time_secs));
    *state.lyrics_timeline.lock().unwrap() = Some(LyricsTimelineState {
        lines: sorted,
        active_index: None,
    });
}

pub fn audio_clear_lyrics_timeline(state: State<'_, AudioState>) {
    *state.lyrics_timeline.lock().unwrap() = None;
}

pub fn audio_set_comments_timeline(
    comments: Vec<FloatingCommentEvent>,
    state: State<'_, AudioState>,
) {
    let mut sorted = comments;
    sorted.sort_by_key(|comment| comment.timestamp_ms);
    *state.comments_timeline.lock().unwrap() = Some(CommentsTimelineState {
        comments: sorted,
        next_index: 0,
    });
}

pub fn audio_clear_comments_timeline(state: State<'_, AudioState>) {
    *state.comments_timeline.lock().unwrap() = None;
}

pub fn process_lyrics_timeline(handle: &tauri::AppHandle, state: &AudioState, pos_secs: f64) {
    let mut timeline = state.lyrics_timeline.lock().unwrap();
    let Some(timeline) = timeline.as_mut() else {
        return;
    };

    let mut next_active = None;
    for (index, line) in timeline.lines.iter().enumerate().rev() {
        if line.time_secs <= pos_secs + 0.3 {
            next_active = Some(index);
            break;
        }
    }

    if timeline.active_index != next_active {
        timeline.active_index = next_active;
        handle
            .emit("lyrics:active_line", next_active.map(|index| index as i64))
            .ok();
    }
}

pub fn process_comments_timeline(handle: &tauri::AppHandle, state: &AudioState, pos_secs: f64) {
    let mut timeline = state.comments_timeline.lock().unwrap();
    let Some(timeline) = timeline.as_mut() else {
        return;
    };

    let current_ms = (pos_secs * 1000.0).max(0.0) as u64;

    while timeline.next_index > 0
        && timeline.comments[timeline.next_index - 1].timestamp_ms > current_ms + 2_000
    {
        timeline.next_index -= 1;
    }

    while timeline.next_index < timeline.comments.len() {
        let comment = &timeline.comments[timeline.next_index];
        if comment.timestamp_ms + 2_000 < current_ms {
            timeline.next_index += 1;
            continue;
        }
        if comment.timestamp_ms > current_ms + 2_000 {
            break;
        }
        handle.emit("comments:show", comment.clone()).ok();
        timeline.next_index += 1;
    }
}
