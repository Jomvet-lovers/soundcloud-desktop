"""Константы NATS — синхронизированы с backend/src/bus/subjects.rs."""

AI_DETECT_LANGUAGE = "ai.rpc.detect_language"
AI_SEARCH_QUERIES = "ai.rpc.search_queries"
AI_RANK_LYRICS = "ai.rpc.rank_lyrics"
AI_TRANSCRIBE = "ai.rpc.transcribe"
AI_ENCODE_TEXT_MULAN = "ai.rpc.encode_text_mulan"
AI_RESOLVE_ARTIST = "ai.rpc.resolve_artist"
AI_VERIFY_EXISTENCE = "ai.rpc.verify_existence"
AI_MATCH_TRACK = "ai.rpc.match_track"
AI_QUALITY_SCORE = "ai.rpc.quality_score"

STREAM_AI_RPC = "AI_RPC"
SUBJECT_AI_RPC_FILTER = "ai.rpc.>"
DURABLE_AI_RPC = "ai-workers"

STREAM_INDEX_AUDIO = "INDEX_AUDIO"
SUBJECT_INDEX_AUDIO_NEW = "index.audio.new"
DURABLE_INDEX_AUDIO = "audio-workers"

STREAM_EMBED_LYRICS = "EMBED_LYRICS"
SUBJECT_EMBED_LYRICS_NEW = "embed.lyrics.new"
DURABLE_EMBED_LYRICS = "lyrics-workers"

STREAM_TRAIN_COLLAB = "TRAIN_COLLAB"
SUBJECT_TRAIN_COLLAB_NEW = "train.collab.new"
DURABLE_TRAIN_COLLAB = "collab-workers"

STREAM_TRAIN_QUALITY = "TRAIN_QUALITY"
SUBJECT_TRAIN_QUALITY_NEW = "train.quality.new"
DURABLE_TRAIN_QUALITY = "quality-workers"

SUBJECT_DONE_INDEX_AUDIO = "done.index_audio"
SUBJECT_DONE_EMBED_LYRICS = "done.embed_lyrics"
SUBJECT_DONE_TRAIN_COLLAB = "done.train_collab"
SUBJECT_DONE_TRAIN_QUALITY = "done.train_quality"
