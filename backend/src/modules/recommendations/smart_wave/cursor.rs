//! Cursor для бесконечной волны.
//!
//! Хранится в Redis с TTL 30 минут под ключом
//! `smartwave:cursor:{seed_kind}:{seed_key}`. Если Redis грохнули — клиент
//! просто получит свежий старт, всё остальное (избежание уже сыгранного
//! и т.д.) переоткроется из user_events. Cursor — opaque-токен, клиент
//! его эхает обратно в follow-up запросах.
//!
//! Что хранится:
//! - `served` — сколько треков уже отдали этому юзеру в этом сеансе волны;
//! - `seen_tracks` — ringbuffer последних 200 sc_track_id, чтобы не повторять;
//! - `seen_artists` — счётчик per-artist в скользящем окне 30 треков;
//! - `neg_window` — сколько дизов/скипов было в последних 20 треках от волны
//!   (записывает feedback endpoint), используется blender'ом для адаптации.

use std::collections::VecDeque;

use deadpool_redis::redis::AsyncCommands;
use deadpool_redis::Pool as RedisPool;
use rand::Rng;
use serde::{Deserialize, Serialize};
use tracing::debug;
use uuid::Uuid;

const TTL_SECS: u64 = 30 * 60;
const SEEN_TRACKS_CAP: usize = 200;
const ARTIST_WINDOW: usize = 30;
const NEG_WINDOW: u8 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeedKind {
    User,
    Track,
    Artist,
}

impl SeedKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Track => "track",
            Self::Artist => "artist",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaveCursor {
    /// Полу-случайный handle: даже одинаковый seed у одного юзера может вести
    /// несколько параллельных сессий волны (например в двух вкладках).
    pub handle: String,
    pub seed_kind: SeedKind,
    pub seed_key: String,
    pub served: u32,
    pub seen_tracks: VecDeque<u64>,
    pub seen_artists: VecDeque<Uuid>,
    /// Последние NEG_WINDOW исходов (0 = нейтрал/позитив, 1 = негатив).
    pub neg_flags: VecDeque<u8>,
}

impl WaveCursor {
    pub fn new(seed_kind: SeedKind, seed_key: String) -> Self {
        let handle = random_handle();
        Self {
            handle,
            seed_kind,
            seed_key,
            served: 0,
            seen_tracks: VecDeque::with_capacity(SEEN_TRACKS_CAP),
            seen_artists: VecDeque::with_capacity(ARTIST_WINDOW),
            neg_flags: VecDeque::with_capacity(NEG_WINDOW as usize),
        }
    }

    pub fn redis_key(&self, owner: &str) -> String {
        format!(
            "smartwave:cursor:{}:{}:{}:{}",
            self.seed_kind.as_str(),
            owner,
            self.seed_key,
            self.handle
        )
    }

    /// Доля дизов в скользящем окне feedback'а.
    pub fn neg_rate(&self) -> f32 {
        if self.neg_flags.is_empty() {
            return 0.0;
        }
        let neg = self.neg_flags.iter().filter(|f| **f == 1).count() as f32;
        neg / self.neg_flags.len() as f32
    }

    pub fn mark_served(&mut self, sc_track_id: u64, artist_id: Option<Uuid>) {
        self.served = self.served.saturating_add(1);
        push_capped(&mut self.seen_tracks, sc_track_id, SEEN_TRACKS_CAP);
        if let Some(a) = artist_id {
            push_capped(&mut self.seen_artists, a, ARTIST_WINDOW);
        }
    }

    pub fn record_outcomes(&mut self, negatives: usize, positives: usize) {
        for _ in 0..negatives {
            push_capped(&mut self.neg_flags, 1, NEG_WINDOW as usize);
        }
        for _ in 0..positives {
            push_capped(&mut self.neg_flags, 0, NEG_WINDOW as usize);
        }
    }

    pub fn artist_count_in_window(&self, artist_id: Uuid) -> usize {
        self.seen_artists.iter().filter(|a| **a == artist_id).count()
    }

    pub fn contains(&self, sc_track_id: u64) -> bool {
        self.seen_tracks.iter().any(|t| *t == sc_track_id)
    }
}

fn push_capped<T>(buf: &mut VecDeque<T>, v: T, cap: usize) {
    if buf.len() >= cap {
        buf.pop_front();
    }
    buf.push_back(v);
}

fn random_handle() -> String {
    let mut rng = rand::thread_rng();
    let n: u64 = rng.gen();
    format!("{:x}", n & 0xffff_ffff)
}

pub async fn load_or_new(
    redis: &RedisPool,
    owner: &str,
    token: Option<&str>,
    seed_kind: SeedKind,
    seed_key: &str,
) -> WaveCursor {
    if let Some(t) = token {
        if let Some(c) = read(redis, owner, t).await {
            if c.seed_kind == seed_kind && c.seed_key == seed_key {
                return c;
            }
        }
    }
    WaveCursor::new(seed_kind, seed_key.to_string())
}

pub async fn save(redis: &RedisPool, owner: &str, cursor: &WaveCursor) -> Option<String> {
    let Ok(payload) = serde_json::to_string(cursor) else {
        return None;
    };
    let key = cursor.redis_key(owner);
    let Ok(mut conn) = redis.get().await else {
        debug!("smartwave: redis unavailable, cursor stateless");
        return None;
    };
    let res: Result<(), _> = conn.set_ex::<_, _, ()>(&key, payload, TTL_SECS).await;
    if let Err(e) = res {
        debug!(error = %e, "smartwave: cursor save failed");
        return None;
    }
    Some(cursor.handle.clone())
}

async fn read(redis: &RedisPool, owner: &str, handle: &str) -> Option<WaveCursor> {
    let mut conn = redis.get().await.ok()?;
    // Сканировать не нужно — handle включает все ключи seed/owner; но клиент
    // присылает только handle, без seed_kind. Чтобы избежать SCAN, держим
    // отдельный backref handle→full_key (cheap).
    let backref: Option<String> = conn
        .get(handle_lookup_key(owner, handle))
        .await
        .ok()
        .flatten();
    let full_key = backref?;
    let raw: Option<String> = conn.get(&full_key).await.ok().flatten();
    let raw = raw?;
    serde_json::from_str(&raw).ok()
}

pub async fn register_handle(redis: &RedisPool, owner: &str, cursor: &WaveCursor) {
    let Ok(mut conn) = redis.get().await else {
        return;
    };
    let _: Result<(), _> = conn
        .set_ex::<_, _, ()>(
            handle_lookup_key(owner, &cursor.handle),
            cursor.redis_key(owner),
            TTL_SECS,
        )
        .await;
}

fn handle_lookup_key(owner: &str, handle: &str) -> String {
    format!("smartwave:handle:{owner}:{handle}")
}
