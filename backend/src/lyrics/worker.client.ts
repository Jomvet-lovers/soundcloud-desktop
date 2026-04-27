import { Injectable } from '@nestjs/common';
import { NatsService } from '../bus/nats.service.js';
import { SUBJECTS } from '../bus/subjects.js';

export interface RankCandidate {
  idx: number;
  source: string;
  snippet: string;
}

export interface RankResult {
  best_idx: number;
  score: number;
  scores?: Array<{ idx: number; score: number }>;
}

export interface TranscribeResult {
  syncedLrc: string | null;
  plainText: string | null;
  language: string;
}

@Injectable()
export class WorkerClient {
  constructor(private readonly nats: NatsService) {}

  detectLanguage(text: string): Promise<{ language: string; confidence: number } | null> {
    return this.nats.request(SUBJECTS.aiDetectLanguage, { text }, 15_000);
  }

  async generateSearchQueries(artist: string, title: string): Promise<string[]> {
    const res = await this.nats.request<{ queries: string[] }>(
      SUBJECTS.aiSearchQueries,
      { artist, title },
      40_000,
    );
    const queries = res?.queries?.filter((q) => q?.trim()) ?? [];
    return queries.length ? queries : [`${artist} ${title}`.trim()];
  }

  rankLyrics(
    artist: string,
    title: string,
    candidates: RankCandidate[],
  ): Promise<RankResult | null> {
    if (!candidates.length) return Promise.resolve(null);
    return this.nats.request(SUBJECTS.aiRankLyrics, { artist, title, candidates }, 60_000);
  }

  transcribeAudio(
    audioUrl: string,
    language?: string,
    initialPrompt?: string,
  ): Promise<TranscribeResult | null> {
    return this.nats.request(
      SUBJECTS.aiTranscribe,
      { audio_url: audioUrl, language, initial_prompt: initialPrompt },
      180_000,
      { throwOnError: true },
    );
  }

  async encodeTextMulan(text: string): Promise<number[] | null> {
    const res = await this.nats.request<{ vector: number[] }>(
      SUBJECTS.aiEncodeTextMulan,
      { text },
      15_000,
    );
    return res?.vector ?? null;
  }
}
