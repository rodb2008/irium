# Irium WebSocket and SSE Streaming API

The Irium node exposes a real-time event stream over WebSocket and Server-Sent Events
(SSE) on the same port as the HTTP API. There is no separate port.

---

## Connection URL

```
WebSocket:  ws://<host>:<port>/ws
SSE:        http://<host>:<port>/events
```

The WebSocket and SSE handlers share the main HTTP server's port — there is no
separate WebSocket port. The HTTP listener binds to `IRIUM_NODE_HOST` /
`IRIUM_NODE_PORT` (default `127.0.0.1:38300`); use the same host and port for
`/ws` and `/events`.

Example with default port:

```
ws://localhost:38300/ws
http://localhost:38300/events
```

---

## Authentication

If `IRIUM_RPC_TOKEN` is set on the node, all WebSocket connections must present the
token in the initial HTTP upgrade request:

```
Authorization: Bearer <token>
```

Connections without a valid token receive `HTTP 401 Unauthorized` and are rejected
before the upgrade completes.

If `IRIUM_WS_PUBLIC=true` is also set, unauthenticated connections are allowed but
receive only public events: `block.new` and `offer.created`. All other event types
require authentication.

---

## Subscription Message

After connecting, send a JSON subscription message to select which events to receive.
The server does not stream any events until a subscription is sent.

**Subscribe to specific event types:**

```json
{
  "action": "subscribe",
  "events": ["block.new", "agreement.satisfied", "proof.gossip_received"]
}
```

**Subscribe using a wildcard:**

```json
{
  "action": "subscribe",
  "events": ["agreement.*"]
}
```

`agreement.*` matches all agreement event types emitted by iriumd:
`agreement.funded`, `agreement.proof_submitted`, `agreement.satisfied`,
`agreement.timeout`, `agreement.auto_released`.

**Subscribe with an agreement filter:**

```json
{
  "action": "subscribe",
  "events": ["agreement.*"],
  "filter": {
    "agreement_hash": "a1b2c3d4e5f6..."
  }
}
```

With a filter, only events whose `data.agreement_hash` matches are delivered. Events
without an `agreement_hash` field in their data (e.g. `block.new`) are unaffected by
agreement filters.

**Acknowledgement:**

The server responds immediately with:

```json
{
  "type": "subscribed",
  "events": ["agreement.*"]
}
```

The `events` array in the response reflects your subscription patterns exactly as sent. Wildcards such as `agreement.*` are stored as patterns and matched against incoming events — they are not expanded in the acknowledgement.

---

## Event Types

All events share this envelope:

```json
{
  "type": "<event_type>",
  "ts": 1746144000,
  "data": { ... }
}
```

`ts` is a Unix timestamp (seconds).

---

### `block.new`

Fired on every new block accepted by the node, whether mined locally or received from
a peer.

```json
{
  "type": "block.new",
  "ts": 1746144032,
  "data": {
    "height": 20300,
    "hash": "000000000735e2852fc54680a93b982de52592594b9fbfbeda711f648598e17e"
  }
}
```

**Public event** — delivered to unauthenticated connections when `IRIUM_WS_PUBLIC=true`.

---

### `offer.created`

Fired when a new offer is added to the local offer store (whether from the local node
or discovered via the P2P marketplace feed).

```json
{
  "type": "offer.created",
  "ts": 1746144100,
  "data": {
    "offer_id": "off_abc123",
    "status": "open"
  }
}
```

**Public event** — delivered to unauthenticated connections when `IRIUM_WS_PUBLIC=true`.

---

### `offer.taken`

Fired when an offer transitions from `open` to `taken`.

```json
{
  "type": "offer.taken",
  "ts": 1746144200,
  "data": {
    "offer_id": "off_abc123",
    "status": "taken"
  }
}
```

---

### `agreement.funded`

Fired when the node processes a funding transaction that matches a known agreement.

```json
{
  "type": "agreement.funded",
  "ts": 1746144300,
  "data": {
    "agreement_hash": "a1b2c3d4e5f6...",
    "txid": "7f8e9d0c1b2a..."
  }
}
```

---

### `agreement.proof_submitted`

Fired when a proof is submitted against an agreement and accepted by the node.

```json
{
  "type": "agreement.proof_submitted",
  "ts": 1746144400,
  "data": {
    "agreement_hash": "a1b2c3d4e5f6...",
    "proof_id": "prf_xyz789"
  }
}
```

---

### `agreement.satisfied`

Fired when release eligibility becomes true for an agreement (proof accepted and
finality depth reached).

```json
{
  "type": "agreement.satisfied",
  "ts": 1746144500,
  "data": {
    "agreement_hash": "a1b2c3d4e5f6..."
  }
}
```

---

### `agreement.timeout`

Fired when an agreement's deadline passes without a valid proof, making the refund
path eligible.

```json
{
  "type": "agreement.timeout",
  "ts": 1746144600,
  "data": {
    "agreement_hash": "a1b2c3d4e5f6..."
  }
}
```

---

### `agreement.auto_released`

Fired when the auto-release watcher (`irium-wallet watch --auto-release`) or
the node's internal release path completes a release transaction for an
agreement. Delivered alongside `agreement.satisfied` so clients that follow
the auto-release flow can react without polling status.

```json
{
  "type": "agreement.auto_released",
  "ts": 1746144700,
  "data": {
    "agreement_hash": "a1b2c3d4e5f6..."
  }
}
```

---

### `offer.relisted`

Fired when a previously-taken or expired offer is relisted back to `open`
(see `IRIUM_OFFER_RELIST_GRACE_BLOCKS`). Delivered to authenticated
connections only.

```json
{
  "type": "offer.relisted",
  "ts": 1746144750,
  "data": {
    "offer_id": "off_abc123",
    "status": "open"
  }
}
```

---

### `offer.expired`

Fired when an offer crosses its `timeout_height` and the node marks it
expired.

```json
{
  "type": "offer.expired",
  "ts": 1746144800,
  "data": {
    "offer_id": "off_abc123",
    "status": "expired"
  }
}
```

---

### `proof.gossip_received`

Fired when a proof arrives at this node via P2P gossip (submitted by another peer,
not locally).

```json
{
  "type": "proof.gossip_received",
  "ts": 1746144800,
  "data": {
    "proof_id": "prf_xyz789",
    "agreement_hash": "a1b2c3d4e5f6..."
  }
}
```

---

### `peer.connected`

Fired when a new P2P peer connects to this node.

```json
{
  "type": "peer.connected",
  "ts": 1746144900,
  "data": {
    "multiaddr": "/ip4/203.0.113.10/tcp/38301"
  }
}
```

---

### `peer.disconnected`

Fired when a P2P peer disconnects from this node.

```json
{
  "type": "peer.disconnected",
  "ts": 1746145000,
  "data": {
    "multiaddr": "/ip4/203.0.113.10/tcp/38301"
  }
}
```

---

## SSE Endpoint

For environments where WebSocket is unavailable (strict HTTP proxies, some CDN
configurations), the same events are available as a Server-Sent Events stream.

```
GET /events
Authorization: Bearer <token>
```

Each event is delivered as a newline-delimited JSON line prefixed with `data:`:

```
data: {"type":"block.new","ts":1746144032,"data":{"height":20300,"hash":"000000..."}}

data: {"type":"peer.connected","ts":1746144900,"data":{"multiaddr":"/ip4/..."}}
```

SSE does not support per-connection subscription filtering. All events for which the
connection is authenticated are streamed. Client-side filtering is the caller's
responsibility.

Example with curl:

```bash
curl -N -H "Authorization: Bearer $IRIUM_RPC_TOKEN" http://localhost:38300/events
```

---

## Reconnection

The WebSocket connection is not automatically re-established if the node restarts or
the connection is dropped. Clients must implement reconnection with backoff. A
recommended pattern:

- Initial reconnect delay: 1 second
- Backoff factor: 2×
- Maximum delay: 60 seconds
- On reconnect: re-send the subscription message

After reconnecting, re-subscribe to the desired event types. The node does not
remember previous subscriptions across connections.

---

## JavaScript Client Example

The following example uses the browser WebSocket API to subscribe to agreement events
and block notifications.

```javascript
class IriumWebSocket {
  constructor(url, token) {
    this.url = url;
    this.token = token;
    this.subscriptions = [];
    this.handlers = {};
    this.reconnectDelay = 1000;
    this._connect();
  }

  _connect() {
    // Note: browser WebSocket does not support custom headers.
    // Pass the token as a query parameter for browser use,
    // or use a Node.js WebSocket library that supports headers.
    const wsUrl = this.token
      ? `${this.url}?token=${this.token}`
      : this.url;

    this.ws = new WebSocket(wsUrl);

    this.ws.onopen = () => {
      console.log("[irium-ws] connected");
      this.reconnectDelay = 1000;
      if (this.subscriptions.length > 0) {
        this._sendSubscribe();
      }
    };

    this.ws.onmessage = (event) => {
      const msg = JSON.parse(event.data);
      if (msg.type === "subscribed") {
        console.log("[irium-ws] subscribed to:", msg.events);
        return;
      }
      const handler = this.handlers[msg.type] || this.handlers["*"];
      if (handler) handler(msg);
    };

    this.ws.onclose = () => {
      console.log(`[irium-ws] disconnected, reconnecting in ${this.reconnectDelay}ms`);
      setTimeout(() => {
        this.reconnectDelay = Math.min(this.reconnectDelay * 2, 60000);
        this._connect();
      }, this.reconnectDelay);
    };

    this.ws.onerror = (err) => {
      console.error("[irium-ws] error:", err);
    };
  }

  _sendSubscribe() {
    this.ws.send(JSON.stringify({
      action: "subscribe",
      events: this.subscriptions
    }));
  }

  subscribe(eventTypes, handler, filter) {
    this.subscriptions = eventTypes;
    if (filter) this.subscriptionFilter = filter;
    this.handlers = {};
    for (const t of eventTypes) {
      this.handlers[t] = handler;
    }
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      const msg = { action: "subscribe", events: eventTypes };
      if (filter) msg.filter = filter;
      this.ws.send(JSON.stringify(msg));
    }
  }

  close() {
    this.ws.onclose = null; // disable reconnect
    this.ws.close();
  }
}

// --- Usage ---

const client = new IriumWebSocket("ws://localhost:38300/ws", "your-token-here");

// Subscribe to all agreement events and new blocks
client.subscribe(
  ["agreement.*", "block.new"],
  (event) => {
    switch (event.type) {
      case "block.new":
        console.log("New block:", event.data.height, event.data.hash);
        break;
      case "agreement.satisfied":
        console.log("Agreement satisfied:", event.data.agreement_hash);
        break;
      case "agreement.disputed":
        console.log("Agreement disputed:", event.data.agreement_hash);
        break;
      default:
        console.log("Event:", event.type, event.data);
    }
  }
);

// Subscribe to events for a specific agreement
client.subscribe(
  ["agreement.*"],
  (event) => {
    console.log("Agreement event:", event.type, event.data);
  },
  { agreement_hash: "a1b2c3d4e5f6..." }
);
```

**Node.js example** (supports `Authorization` header directly):

```javascript
const WebSocket = require("ws");

const ws = new WebSocket("ws://localhost:38300/ws", {
  headers: {
    "Authorization": "Bearer " + process.env.IRIUM_RPC_TOKEN
  }
});

ws.on("open", () => {
  ws.send(JSON.stringify({
    action: "subscribe",
    events: ["block.new", "agreement.satisfied", "proof.gossip_received"]
  }));
});

ws.on("message", (data) => {
  const event = JSON.parse(data.toString());
  console.log(event.type, event.data);
});
```

---

## Environment Variables Reference

| Variable | Default | Description |
|---|---|---|
| `IRIUM_RPC_TOKEN` | (unset) | Bearer token required for WS/SSE auth (shared with the rest of the RPC API) |
| `IRIUM_WS_PUBLIC` | `false` | When `true`, allows unauthenticated WS/SSE connections to receive the public-event subset (`block.new`, `offer.created`); all other events still require auth |
| `IRIUM_NODE_HOST` | `127.0.0.1` | HTTP/WS/SSE listener host (shared) |
| `IRIUM_NODE_PORT` | `38300` | HTTP/WS/SSE listener port (shared — no separate WS port) |
