// WebSocket client with auto-reconnect for Economind signal streaming

export type ServerEventType =
  | 'strategy_run_started'
  | 'strategy_run_completed'
  | 'signal_emitted'
  | 'ingestion_job_completed'
  | 'position_opened'
  | 'position_closed'
  | 'system_error';

export interface ServerEvent {
  type: ServerEventType;
  [key: string]: unknown;
}

type EventHandler = (event: ServerEvent) => void;

const RECONNECT_BASE_MS = 1_000;
const RECONNECT_MAX_MS = 30_000;

export class EconomindWS {
  private ws: WebSocket | null = null;
  private handlers = new Set<EventHandler>();
  private closed = false;
  private attempt = 0;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(private readonly apiKey: string) {}

  connect(): void {
    this.closed = false;
    this._open();
  }

  disconnect(): void {
    this.closed = true;
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer);
    this.ws?.close();
    this.ws = null;
  }

  on(handler: EventHandler): () => void {
    this.handlers.add(handler);
    return () => this.handlers.delete(handler);
  }

  private _open(): void {
    const proto = location.protocol === 'https:' ? 'wss' : 'ws';
    const url = `${proto}://${location.host}/api/v1/ws/signals?api_key=${encodeURIComponent(this.apiKey)}`;
    this.ws = new WebSocket(url);

    this.ws.onopen = () => {
      this.attempt = 0;
    };

    this.ws.onmessage = (e) => {
      try {
        const event = JSON.parse(e.data as string) as ServerEvent;
        this.handlers.forEach((h) => h(event));
      } catch {
        // ignore malformed messages
      }
    };

    this.ws.onclose = () => {
      if (!this.closed) this._scheduleReconnect();
    };

    this.ws.onerror = () => {
      this.ws?.close();
    };
  }

  private _scheduleReconnect(): void {
    const delay = Math.min(RECONNECT_BASE_MS * 2 ** this.attempt, RECONNECT_MAX_MS);
    this.attempt++;
    this.reconnectTimer = setTimeout(() => this._open(), delay);
  }
}

// Singleton connected to the current API key
let instance: EconomindWS | null = null;

export function getWsClient(): EconomindWS {
  if (!instance) {
    const key = localStorage.getItem('api_key') ?? '';
    instance = new EconomindWS(key);
  }
  return instance;
}

export function resetWsClient(): void {
  instance?.disconnect();
  instance = null;
}
