import type { IriumEvent, IriumEventType, EventFilter } from "./types.js";
// eslint-disable-next-line @typescript-eslint/no-require-imports
import NodeWebSocket from "ws";

export type EventHandler<T = unknown> = (event: IriumEvent<T>) => void;

export class IriumWsClient {
  private ws: NodeWebSocket | null = null;
  private handlers: Map<string, EventHandler[]> = new Map();
  private reconnectDelay = 2000;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private stopped = false;

  constructor(
    private readonly wsUrl: string,
    private readonly authHeader?: string,
  ) {}

  connect(): void {
    const headers: Record<string, string> = {};
    if (this.authHeader) {
      headers["Authorization"] = this.authHeader;
    }

    const ws = new NodeWebSocket(this.wsUrl, { headers });

    ws.on("open", () => {
      this.ws = ws;
      this.reconnectDelay = 2000;
      this.flushSubscriptions();
    });

    ws.on("error", () => {
      // will be followed by close event which triggers reconnect
    });

    ws.on("message", (data: NodeWebSocket.RawData) => {
      try {
        const raw = typeof data === "string" ? data : data.toString("utf8");
        const event: IriumEvent = JSON.parse(raw);
        const type = event.type;
        const handlers = [
          ...(this.handlers.get(type) ?? []),
          ...(this.handlers.get("*") ?? []),
        ];
        for (const h of handlers) h(event);
      } catch {
        // ignore malformed events
      }
    });

    ws.on("close", () => {
      this.ws = null;
      if (!this.stopped) {
        this.reconnectTimer = setTimeout(() => {
          this.connect();
        }, this.reconnectDelay);
        this.reconnectDelay = Math.min(this.reconnectDelay * 2, 30000);
      }
    });
  }

  private flushSubscriptions(): void {
    if (!this.ws || this.ws.readyState !== NodeWebSocket.OPEN) return;
    const allTypes = [...this.handlers.keys()];
    if (allTypes.length > 0) {
      this.ws.send(JSON.stringify({ action: "subscribe", events: allTypes }));
    }
  }

  subscribe(
    eventTypes: IriumEventType[],
    handler: EventHandler,
    filter?: EventFilter,
  ): void {
    for (const t of eventTypes) {
      if (!this.handlers.has(t)) this.handlers.set(t, []);
      this.handlers.get(t)!.push(handler as EventHandler);
    }
    if (this.ws && this.ws.readyState === NodeWebSocket.OPEN) {
      const allTypes = [...this.handlers.keys()];
      const msg: Record<string, unknown> = {
        action: "subscribe",
        events: allTypes,
      };
      if (filter) msg.filter = filter;
      this.ws.send(JSON.stringify(msg));
    }
  }

  unsubscribe(eventTypes: IriumEventType[], handler: EventHandler): void {
    for (const t of eventTypes) {
      const list = this.handlers.get(t) ?? [];
      const idx = list.indexOf(handler as EventHandler);
      if (idx !== -1) list.splice(idx, 1);
    }
  }

  close(): void {
    this.stopped = true;
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer);
    this.ws?.close();
  }
}
