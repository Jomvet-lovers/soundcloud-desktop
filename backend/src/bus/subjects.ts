export const SUBJECTS = {
  // AI RPC (request-reply через JetStream, чтобы воркер не брал новое пока занят)
  aiDetectLanguage: 'ai.rpc.detect_language',
  aiSearchQueries: 'ai.rpc.search_queries',
  aiRankLyrics: 'ai.rpc.rank_lyrics',
  aiTranscribe: 'ai.rpc.transcribe',
  aiEncodeTextMulan: 'ai.rpc.encode_text_mulan',

  // JetStream work queues (backend → worker, durable)
  indexAudio: 'index.audio.new',
  embedLyrics: 'embed.lyrics.new',

  // JetStream fan-out (worker → backend notify о завершении)
  doneIndexAudio: 'done.index_audio',
  doneEmbedLyrics: 'done.embed_lyrics',

  // JetStream fan-out (streaming → backend notify об аплоаде в storage)
  storageTrackUploaded: 'storage.track_uploaded',
} as const;

export const STREAMS = {
  // AI RPC work queue — короткий TTL, чтобы устаревшие запросы не висели в стриме.
  aiRpc: {
    name: 'AI_RPC',
    subjects: ['ai.rpc.>'],
  },
  // Backend → worker, durable consumers на стороне воркера
  indexAudio: {
    name: 'INDEX_AUDIO',
    subjects: ['index.audio.>'],
  },
  embedLyrics: {
    name: 'EMBED_LYRICS',
    subjects: ['embed.lyrics.>'],
  },
  // Worker → backend notifications
  done: {
    name: 'PIPELINE_DONE',
    subjects: ['done.>'],
  },
  // Streaming → backend notifications (storage.track_uploaded и т.п.)
  storageEvents: {
    name: 'STORAGE_EVENTS',
    subjects: ['storage.>'],
  },
} as const;

export const QUEUE_GROUPS = {
  aiWorkers: 'ai-workers',
} as const;
