"""Константы NATS — синхронизированы с backend/src/bus/subjects.rs."""

AI_DETECT_LANGUAGE = "ai.rpc.detect_language"
AI_SEARCH_QUERIES = "ai.rpc.search_queries"
AI_RANK_LYRICS = "ai.rpc.rank_lyrics"
AI_TRANSCRIBE = "ai.rpc.transcribe"
AI_ENCODE_TEXT_MULAN = "ai.rpc.encode_text_mulan"
AI_LTR_SCORE = "ai.rpc.ltr_score"
AI_RESOLVE_ARTIST = "ai.rpc.resolve_artist"
AI_VERIFY_EXISTENCE = "ai.rpc.verify_existence"
AI_MATCH_TRACK = "ai.rpc.match_track"
AI_TWO_TOWER_SCORE = "ai.rpc.two_tower_score"
AI_SEQUENTIAL_PREDICT = "ai.rpc.sequential_predict"
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

STREAM_TRAIN_LTR = "TRAIN_LTR"
SUBJECT_TRAIN_LTR_NEW = "train.ltr.new"
DURABLE_TRAIN_LTR = "ltr-workers"

STREAM_TRAIN_TWO_TOWER = "TRAIN_TWO_TOWER"
SUBJECT_TRAIN_TWO_TOWER_NEW = "train.two_tower.new"
DURABLE_TRAIN_TWO_TOWER = "two-tower-workers"

STREAM_TRAIN_SEQUENTIAL = "TRAIN_SEQUENTIAL"
SUBJECT_TRAIN_SEQUENTIAL_NEW = "train.sequential.new"
DURABLE_TRAIN_SEQUENTIAL = "sequential-workers"

STREAM_TRAIN_QUALITY = "TRAIN_QUALITY"
SUBJECT_TRAIN_QUALITY_NEW = "train.quality.new"
DURABLE_TRAIN_QUALITY = "quality-workers"

SUBJECT_DONE_INDEX_AUDIO = "done.index_audio"
SUBJECT_DONE_EMBED_LYRICS = "done.embed_lyrics"
SUBJECT_DONE_TRAIN_COLLAB = "done.train_collab"
SUBJECT_DONE_TRAIN_LTR = "done.train_ltr"
SUBJECT_DONE_TRAIN_TWO_TOWER = "done.train_two_tower"
SUBJECT_DONE_TRAIN_SEQUENTIAL = "done.train_sequential"
SUBJECT_DONE_TRAIN_QUALITY = "done.train_quality"
