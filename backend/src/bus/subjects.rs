pub const AI_DETECT_LANGUAGE: &str = "ai.rpc.detect_language";
pub const AI_SEARCH_QUERIES: &str = "ai.rpc.search_queries";
pub const AI_RANK_LYRICS: &str = "ai.rpc.rank_lyrics";
pub const AI_TRANSCRIBE: &str = "ai.rpc.transcribe";
pub const AI_ENCODE_TEXT_MULAN: &str = "ai.rpc.encode_text_mulan";
pub const AI_RESOLVE_ARTIST: &str = "ai.rpc.resolve_artist";
pub const AI_VERIFY_EXISTENCE: &str = "ai.rpc.verify_existence";
pub const AI_MATCH_TRACK: &str = "ai.rpc.match_track";
pub const AI_QUALITY_SCORE: &str = "ai.rpc.quality_score";

pub const INDEX_AUDIO: &str = "index.audio.new";
pub const EMBED_LYRICS: &str = "embed.lyrics.new";
pub const TRAIN_COLLAB: &str = "train.collab.new";
pub const TRAIN_QUALITY: &str = "train.quality.new";

pub const ENRICH_TRACK: &str = "enrich.track.new";

pub const DONE_INDEX_AUDIO: &str = "done.index_audio";
pub const DONE_EMBED_LYRICS: &str = "done.embed_lyrics";

pub const STORAGE_TRACK_UPLOADED: &str = "storage.track_uploaded";

pub struct StreamCfg {
    pub name: &'static str,
    pub subjects: &'static [&'static str],
}

pub mod streams {
    use super::StreamCfg;

    pub const AI_RPC: StreamCfg = StreamCfg {
        name: "AI_RPC",
        subjects: &["ai.rpc.>"],
    };
    pub const INDEX_AUDIO: StreamCfg = StreamCfg {
        name: "INDEX_AUDIO",
        subjects: &["index.audio.>"],
    };
    pub const EMBED_LYRICS: StreamCfg = StreamCfg {
        name: "EMBED_LYRICS",
        subjects: &["embed.lyrics.>"],
    };
    pub const TRAIN_COLLAB: StreamCfg = StreamCfg {
        name: "TRAIN_COLLAB",
        subjects: &["train.collab.>"],
    };
    pub const TRAIN_QUALITY: StreamCfg = StreamCfg {
        name: "TRAIN_QUALITY",
        subjects: &["train.quality.>"],
    };
    pub const ENRICH: StreamCfg = StreamCfg {
        name: "ENRICH",
        subjects: &["enrich.>"],
    };
    pub const DONE: StreamCfg = StreamCfg {
        name: "PIPELINE_DONE",
        subjects: &["done.>"],
    };
    pub const STORAGE_EVENTS: StreamCfg = StreamCfg {
        name: "STORAGE_EVENTS",
        subjects: &["storage.>"],
    };
}
