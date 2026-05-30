"""Константы NATS — синхронизированы с backend/src/bus/subjects.rs."""

AI_DETECT_LANGUAGE = "ai.rpc.detect_language"
AI_SEARCH_QUERIES = "ai.rpc.search_queries"
AI_RANK_LYRICS = "ai.rpc.rank_lyrics"
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

# Self-gen лирика (whisper) — own work-queue стрим, НЕ ai.rpc (тяжёлая фоновая
# GPU-задача, длительность не ограничена). Backend публикует transcribe.audio.new,
# воркер отвечает событием done.transcribe.
STREAM_TRANSCRIBE = "TRANSCRIBE"
SUBJECT_TRANSCRIBE_NEW = "transcribe.audio.new"
DURABLE_TRANSCRIBE = "transcribe-workers"

# Энкод текста запроса (vibe MuLan / lyrics bge-m3) — own work-queue, НЕ ai.rpc.
# Воркеров мало и под хайлоадом отвечают долго → backend публикует
# encode.text.new, воркер отвечает событием done.encode (как transcribe).
STREAM_ENCODE = "ENCODE"
SUBJECT_ENCODE_NEW = "encode.text.new"
DURABLE_ENCODE = "encode-workers"

STREAM_TRAIN_COLLAB = "TRAIN_COLLAB"
SUBJECT_TRAIN_COLLAB_NEW = "train.collab.new"
DURABLE_TRAIN_COLLAB = "collab-workers"
OBJECT_STORE_COLLAB = "COLLAB_DATA"  # bulk-сессии едут блобом, не в сообщении

STREAM_TRAIN_QUALITY = "TRAIN_QUALITY"
SUBJECT_TRAIN_QUALITY_NEW = "train.quality.new"
DURABLE_TRAIN_QUALITY = "quality-workers"

SUBJECT_DONE_INDEX_AUDIO = "done.index_audio"
SUBJECT_DONE_EMBED_LYRICS = "done.embed_lyrics"
SUBJECT_DONE_TRANSCRIBE = "done.transcribe"
SUBJECT_DONE_TRAIN_COLLAB = "done.train_collab"
SUBJECT_DONE_TRAIN_QUALITY = "done.train_quality"
SUBJECT_DONE_ENCODE = "done.encode"
