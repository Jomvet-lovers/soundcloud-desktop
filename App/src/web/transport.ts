import {
  type EventChannel,
  type EventMap,
  type Transport,
  TransportError,
  type Unsubscribe,
} from '@sc/data';

export interface LoopbackTarget {
  port: number;
  token: string;
}

/** Формат шелла/превью: `#p=PORT;t=TOKEN` (Servo-шелл передаёт так же). */
export function parseTarget(hash: string): LoopbackTarget | null {
  const m = /p=(\d+);t=([0-9a-f]+)/.exec(hash);
  return m ? { port: Number(m[1]), token: m[2] } : null;
}

type Handler = (payload: unknown) => void;

/** Транспорт до sc-rpc: POST /rpc/{method} + мультиплекс-WS /events. */
export class LoopbackTransport implements Transport {
  private ws: WebSocket | null = null;
  private wsOpen = false;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private handlers = new Map<EventChannel, Set<Handler>>();

  constructor(private readonly target: LoopbackTarget) {}

  async call<T>(method: string, args?: Record<string, unknown>): Promise<T> {
    const res = await fetch(`http://127.0.0.1:${this.target.port}/rpc/${method}`, {
      method: 'POST',
      headers: {
        authorization: `Bearer ${this.target.token}`,
        'content-type': 'application/json',
      },
      body: JSON.stringify(args ?? null),
    });
    if (!res.ok) {
      let message = res.statusText;
      try {
        const body = (await res.json()) as { error?: string };
        if (body.error) message = body.error;
      } catch {
        // не-JSON тело — оставляем statusText
      }
      throw new TransportError(method, message);
    }
    return (await res.json()) as T;
  }

  subscribe<K extends EventChannel>(
    channel: K,
    handler: (payload: EventMap[K]) => void,
  ): Unsubscribe {
    let set = this.handlers.get(channel);
    if (!set) {
      set = new Set();
      this.handlers.set(channel, set);
      this.send({ op: 'sub', channel });
    }
    set.add(handler as Handler);
    this.ensureWs();

    return () => {
      const current = this.handlers.get(channel);
      if (!current) return;
      current.delete(handler as Handler);
      if (current.size === 0) {
        this.handlers.delete(channel);
        this.send({ op: 'unsub', channel });
      }
    };
  }

  private send(msg: { op: 'sub' | 'unsub'; channel: string }) {
    if (this.ws && this.wsOpen) this.ws.send(JSON.stringify(msg));
    // не открыт — onopen переподпишет всё разом
  }

  private ensureWs() {
    if (this.ws) return;
    const ws = new WebSocket(`ws://127.0.0.1:${this.target.port}/events?t=${this.target.token}`);
    this.ws = ws;
    ws.onopen = () => {
      this.wsOpen = true;
      for (const channel of this.handlers.keys()) ws.send(JSON.stringify({ op: 'sub', channel }));
    };
    ws.onmessage = (e) => {
      let msg: { channel?: EventChannel; payload?: unknown };
      try {
        msg = JSON.parse(String(e.data));
      } catch {
        return;
      }
      if (!msg.channel) return;
      const set = this.handlers.get(msg.channel);
      if (set) for (const h of set) h(msg.payload);
    };
    ws.onclose = () => {
      this.wsOpen = false;
      this.ws = null;
      if (this.handlers.size > 0 && !this.reconnectTimer) {
        this.reconnectTimer = setTimeout(() => {
          this.reconnectTimer = null;
          this.ensureWs();
        }, 1000);
      }
    };
  }
}
