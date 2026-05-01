"""Константы NATS — синхронизированы с backend/src/bus/subjects.ts."""

AI_DETECT_LANGUAGE = "ai.rpc.detect_language"
AI_SEARCH_QUERIES = "ai.rpc.search_queries"
AI_RANK_LYRICS = "ai.rpc.rank_lyrics"
AI_TRANSCRIBE = "ai.rpc.transcribe"
AI_ENCODE_TEXT_MULAN = "ai.rpc.encode_text_mulan"
AI_LTR_SCORE = "ai.rpc.ltr_score"

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

SUBJECT_DONE_INDEX_AUDIO = "done.index_audio"
SUBJECT_DONE_EMBED_LYRICS = "done.embed_lyrics"
SUBJECT_DONE_TRAIN_COLLAB = "done.train_collab"
SUBJECT_DONE_TRAIN_LTR = "done.train_ltr"
