import { HttpService } from '@nestjs/axios';
import { Injectable, Logger, OnModuleInit } from '@nestjs/common';
import type { AxiosRequestConfig, AxiosResponse } from 'axios';
import { SoundcloudService } from '../soundcloud/soundcloud.service.js';
import type { ScTrack } from '../soundcloud/soundcloud.types.js';
import { IndexingService } from './indexing.service.js';

const URN_RE = /soundcloud:tracks:(\d+)/g;
const TTL_MS = 5 * 60 * 1000;
const MAX_SEEN = 20_000;
const MAX_BODY_SCAN_BYTES = 512 * 1024;

type DiscoveryConfig = AxiosRequestConfig & { skipTrackDiscovery?: boolean };

@Injectable()
export class TrackDiscoveryService implements OnModuleInit {
  private readonly logger = new Logger(TrackDiscoveryService.name);
  private readonly recentlySeen = new Map<string, number>();
  private readonly inflight = new Map<string, Promise<void>>();

  constructor(
    private readonly httpService: HttpService,
    private readonly indexing: IndexingService,
    private readonly sc: SoundcloudService,
  ) {}

  onModuleInit() {
    this.httpService.axiosRef.interceptors.response.use(
      (response) => {
        try {
          this.onResponse(response);
        } catch (e) {
          this.logger.debug(`interceptor error: ${(e as Error).message}`);
        }
        return response;
      },
      (error) => Promise.reject(error),
    );
  }

  private onResponse(response: AxiosResponse) {
    const cfg = response.config as DiscoveryConfig;
    if (cfg.skipTrackDiscovery) return;
    if (!response.data) return;

    const contentType = String(response.headers?.['content-type'] ?? '');
    const isJson = contentType.includes('json') || typeof response.data === 'object';
    if (!isJson) return;

    let serialized: string;
    if (typeof response.data === 'string') {
      serialized = response.data;
    } else {
      try {
        serialized = JSON.stringify(response.data);
      } catch {
        return;
      }
    }
    if (serialized.length > MAX_BODY_SCAN_BYTES) {
      serialized = serialized.slice(0, MAX_BODY_SCAN_BYTES);
    }

    const ids = new Set<string>();
    for (const match of serialized.matchAll(URN_RE)) {
      ids.add(match[1]);
    }
    if (ids.size === 0) return;

    const authHeader =
      (cfg.headers?.Authorization as string | undefined) ??
      (cfg.headers?.authorization as string | undefined);
    const accessToken = authHeader?.startsWith('OAuth ') ? authHeader.slice(6) : undefined;
    if (!accessToken) return;

    this.processIds([...ids], accessToken).catch((e) => {
      this.logger.debug(`processIds error: ${(e as Error).message}`);
    });
  }

  private async processIds(ids: string[], accessToken: string): Promise<void> {
    const now = Date.now();

    if (this.recentlySeen.size > MAX_SEEN) {
      for (const [k, ts] of this.recentlySeen) {
        if (now - ts > TTL_MS) this.recentlySeen.delete(k);
      }
    }

    const fresh = ids.filter((id) => {
      const ts = this.recentlySeen.get(id);
      return !ts || now - ts > TTL_MS;
    });
    if (fresh.length === 0) return;

    for (const id of fresh) this.recentlySeen.set(id, now);

    await Promise.all(fresh.map((id) => this.processOne(id, accessToken)));
  }

  private processOne(scTrackId: string, accessToken: string): Promise<void> {
    const existing = this.inflight.get(scTrackId);
    if (existing) return existing;
    const promise = this.runOne(scTrackId, accessToken).finally(() => {
      this.inflight.delete(scTrackId);
    });
    this.inflight.set(scTrackId, promise);
    return promise;
  }

  private async runOne(scTrackId: string, accessToken: string): Promise<void> {
    try {
      const track = await this.sc.apiGet<ScTrack>(
        `/tracks/soundcloud:tracks:${scTrackId}`,
        accessToken,
        undefined,
        { skipTrackDiscovery: true },
      );
      await this.indexing.ensureTrackIndexed(track);
    } catch (e) {
      this.logger.debug(`runOne ${scTrackId}: ${(e as Error).message}`);
    }
  }
}
