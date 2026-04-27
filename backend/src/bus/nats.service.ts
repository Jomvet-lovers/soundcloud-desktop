import type { JetStreamClient, JetStreamManager, PubAck } from '@nats-io/jetstream';
import { jetstream, jetstreamManager, RetentionPolicy, StorageType } from '@nats-io/jetstream';
import type { NatsConnection } from '@nats-io/nats-core';
import { headers as createHeaders } from '@nats-io/nats-core';
import { connect } from '@nats-io/transport-node';
import { Injectable, Logger, OnModuleDestroy, OnModuleInit } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { STREAMS } from './subjects.js';

const encoder = new TextEncoder();
const decoder = new TextDecoder();

@Injectable()
export class NatsService implements OnModuleInit, OnModuleDestroy {
  private readonly logger = new Logger(NatsService.name);
  private nc!: NatsConnection;
  private js!: JetStreamClient;
  private jsm!: JetStreamManager;

  constructor(private readonly config: ConfigService) {}

  async onModuleInit(): Promise<void> {
    const rawUrl = this.config.getOrThrow<string>('nats.url');
    // user:pass в URL парсер ломает (читает как IPv6) — вытаскиваем отдельно
    const parsed = new URL(rawUrl);
    const cleanUrl = `${parsed.protocol}//${parsed.host}`;
    this.nc = await connect({
      servers: cleanUrl,
      name: 'backend',
      reconnect: true,
      maxReconnectAttempts: -1,
      waitOnFirstConnect: true,
      ...(parsed.username ? { user: decodeURIComponent(parsed.username) } : {}),
      ...(parsed.password ? { pass: decodeURIComponent(parsed.password) } : {}),
    });
    this.logger.log(`NATS connected → ${cleanUrl}`);

    this.jsm = await jetstreamManager(this.nc);
    this.js = jetstream(this.nc);

    await this.ensureStream(STREAMS.aiRpc.name, [...STREAMS.aiRpc.subjects], true, 120);
    await this.ensureStream(STREAMS.indexAudio.name, [...STREAMS.indexAudio.subjects], true);
    await this.ensureStream(STREAMS.embedLyrics.name, [...STREAMS.embedLyrics.subjects], true);
    await this.ensureStream(STREAMS.done.name, [...STREAMS.done.subjects], false);
    await this.ensureStream(STREAMS.storageEvents.name, [...STREAMS.storageEvents.subjects], false);
  }

  async onModuleDestroy(): Promise<void> {
    await this.nc?.drain().catch(() => {});
  }

  private async ensureStream(
    name: string,
    subjects: string[],
    workQueue: boolean,
    maxAgeSeconds?: number,
  ): Promise<void> {
    const defaultAgeSec = workQueue ? 24 * 60 * 60 : 60 * 60;
    const ageSec = maxAgeSeconds ?? defaultAgeSec;
    const cfg = {
      name,
      subjects,
      retention: workQueue ? RetentionPolicy.Workqueue : RetentionPolicy.Limits,
      storage: StorageType.File,
      max_age: ageSec * 1_000_000_000,
    };
    try {
      await this.jsm.streams.add(cfg);
      this.logger.log(`JetStream created ${name} subjects=${subjects.join(',')}`);
    } catch (e) {
      const msg = (e as Error).message;
      if (msg.includes('already in use') || msg.includes('stream name already in use')) {
        await this.jsm.streams.update(name, cfg);
      } else {
        throw e;
      }
    }
  }

  get connection(): NatsConnection {
    return this.nc;
  }

  get jetStream(): JetStreamClient {
    return this.js;
  }

  /**
   * AI RPC через JetStream. Сообщение пишется в стрим (worker не подхватит, пока занят).
   * Reply inbox передаём через стандартный NATS-заголовок `Nats-Reply-To` — воркер
   * читает его из headers и публикует ответ через core NATS publish.
   */
  async request<T>(
    subject: string,
    payload: unknown,
    timeoutMs: number,
    opts: { throwOnError?: boolean } = {},
  ): Promise<T | null> {
    const inbox = `_INBOX.${Math.random().toString(36).slice(2)}.${Date.now().toString(36)}`;
    const sub = this.nc.subscribe(inbox, { max: 1 });
    const hdrs = createHeaders();
    hdrs.set('X-Reply-To', inbox);
    let timer: NodeJS.Timeout | undefined;
    try {
      const timeoutPromise = new Promise<null>((resolve) => {
        timer = setTimeout(() => resolve(null), timeoutMs);
      });
      await this.js.publish(subject, encoder.encode(JSON.stringify(payload)), { headers: hdrs });
      const iter = sub[Symbol.asyncIterator]();
      const replyPromise = iter.next().then((r) => (r.done ? null : r.value));
      const msg = await Promise.race([replyPromise, timeoutPromise]);
      if (!msg) {
        this.logger.debug(`request ${subject} timeout`);
        if (opts.throwOnError) throw new Error(`${subject} timeout after ${timeoutMs}ms`);
        return null;
      }
      const raw = decoder.decode(msg.data);
      if (!raw) return null;
      const parsed = JSON.parse(raw) as { ok: boolean; data?: T; error?: string };
      if (!parsed.ok) {
        this.logger.debug(`request ${subject} error: ${parsed.error}`);
        if (opts.throwOnError) throw new Error(parsed.error ?? `${subject} failed`);
        return null;
      }
      return parsed.data ?? null;
    } catch (e) {
      if (opts.throwOnError) throw e;
      this.logger.debug(`request ${subject} failed: ${(e as Error).message}`);
      return null;
    } finally {
      if (timer) clearTimeout(timer);
      try {
        sub.unsubscribe();
      } catch {}
    }
  }

  /** JetStream publish (durable, acked by server). */
  async publish(subject: string, payload: unknown): Promise<PubAck> {
    return this.js.publish(subject, encoder.encode(JSON.stringify(payload)));
  }

  /** Subscribe to JetStream as a durable consumer (pull). */
  async consume(
    stream: string,
    durable: string,
    handler: (data: unknown, meta: { streamSeq: number; deliveries: number }) => Promise<void>,
    filterSubject?: string,
  ): Promise<void> {
    const addCfg = {
      durable_name: durable,
      ack_policy: 'explicit' as never,
      ack_wait: 30 * 1_000_000_000,
      max_deliver: 5,
      ...(filterSubject ? { filter_subject: filterSubject } : {}),
    };
    try {
      await this.jsm.consumers.info(stream, durable);
    } catch {
      await this.jsm.consumers.add(stream, addCfg);
    }

    // Outer loop: после disconnect consumer.consume() может бросить heartbeat-error —
    // пересоздаём подписку, чтобы цикл переживал перезапуск NATS-сервера.
    (async () => {
      while (true) {
        try {
          const consumer = await this.js.consumers.get(stream, durable);
          const messages = await consumer.consume();
          for await (const m of messages) {
            const info = m.info;
            try {
              const data = JSON.parse(decoder.decode(m.data));
              await handler(data, { streamSeq: info.streamSequence, deliveries: info.deliveryCount });
              m.ack();
            } catch (e) {
              this.logger.error(`consume ${stream}/${durable}: ${(e as Error).message}`);
              m.nak(5_000);
            }
          }
        } catch (e) {
          this.logger.warn(`consume loop ${stream}/${durable} broke: ${(e as Error).message} — retry in 2s`);
          await new Promise((r) => setTimeout(r, 2000));
        }
      }
    })();
  }
}
