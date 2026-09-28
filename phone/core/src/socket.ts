/**
 * The socket the library talks through: binary frames in, binary frames out, four events.
 * The app injects a `SocketFactory`; `webSocketFactory` makes one from the standard `WebSocket`
 * constructor, which React Native and Node 24 both have as a global.
 */

/** How a socket ended. */
export interface CloseInfo {
  /** The WebSocket close code, or null when there was none. */
  readonly code: number | null;
  readonly reason: string;
}

/** The events of one socket. `onClose` is called exactly once and is the last event. */
export interface SocketHandlers {
  onOpen(): void;
  /** One binary frame. */
  onMessage(data: Uint8Array): void;
  /** Something went wrong. `onClose` follows. */
  onError(error: Error): void;
  onClose(info: CloseInfo): void;
}

/** One connection. */
export interface Socket {
  /** Sends one binary frame. Throws when the socket is not open. */
  send(data: Uint8Array): void;
  /** Closes the socket. Safe to call more than once. */
  close(code?: number, reason?: string): void;
  /** Sends a WebSocket ping, where the platform can. */
  ping?(): void;
}

/** Opens a socket to `url`. Events only arrive after the factory returned. */
export type SocketFactory = (url: string, handlers: SocketHandlers) => Socket;

/** The part of a `WebSocket` that every implementation types the same way. */
export interface WebSocketLike {
  send(data: ArrayBuffer): void;
  close(code?: number, reason?: string): void;
}

/** A `WebSocket` constructor: the global of React Native or Node 24, or the `ws` package's class. */
export type WebSocketConstructor = new (url: string) => WebSocketLike;

interface MessageEventLike {
  readonly data?: unknown;
}

interface CloseEventLike {
  readonly code?: unknown;
  readonly reason?: unknown;
}

interface ErrorEventLike {
  readonly message?: unknown;
}

/** The members the adapter uses. Implementations type their events differently, so this is internal. */
interface StandardWebSocket extends WebSocketLike {
  binaryType: string;
  onopen: ((event: unknown) => void) | null;
  onmessage: ((event: MessageEventLike) => void) | null;
  onerror: ((event: ErrorEventLike) => void) | null;
  onclose: ((event: CloseEventLike) => void) | null;
  ping?: () => void;
}

function isArrayBuffer(data: unknown): data is ArrayBuffer {
  // `instanceof` fails for a buffer made in another realm, as test runners do.
  return data instanceof ArrayBuffer || Object.prototype.toString.call(data) === "[object ArrayBuffer]";
}

function toBytes(data: unknown): Uint8Array | null {
  if (isArrayBuffer(data)) return new Uint8Array(data);
  if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength).slice();
  return null;
}

function exactBuffer(data: Uint8Array): ArrayBuffer {
  const copy = new Uint8Array(data.length);
  copy.set(data);
  return copy.buffer;
}

/** How the app's platform wants its sockets. */
export interface WebSocketOptions {
  /**
   * Whether the socket's own `ping` may be used (true unless said). React Native's WebSocket on
   * Android sends its "ping" as an empty binary message, not a ping frame; the gateway would read
   * it as a frame, so the app turns this off there and keeps to its encrypted keepalive.
   */
  readonly ping?: boolean;
}

/**
 * Makes a `SocketFactory` from a standard `WebSocket` constructor. Frames are binary
 * (`binaryType = "arraybuffer"`); a text frame is an error and closes the socket.
 */
export function webSocketFactory(WebSocketClass: WebSocketConstructor, options: WebSocketOptions = {}): SocketFactory {
  return (url, handlers) => {
    let done = false;
    let ws: StandardWebSocket | null = null;
    const finish = (info: CloseInfo): void => {
      if (done) return;
      done = true;
      if (ws) {
        ws.onopen = null;
        ws.onmessage = null;
        ws.onerror = null;
        ws.onclose = null;
      }
      handlers.onClose(info);
    };
    const fail = (message: string): void => {
      if (done) return;
      handlers.onError(new Error(message));
    };

    try {
      ws = new WebSocketClass(url) as unknown as StandardWebSocket;
      ws.binaryType = "arraybuffer";
    } catch {
      // Reported after the factory returned, so the caller holds its socket before any event.
      void Promise.resolve().then(() => {
        fail("the socket could not be created");
        finish({ code: null, reason: "" });
      });
      return {
        send() {
          throw new Error("the socket is not open");
        },
        close() {
          finish({ code: null, reason: "" });
        },
      };
    }

    const socket = ws;
    socket.onopen = () => {
      if (!done) handlers.onOpen();
    };
    socket.onmessage = (event) => {
      if (done) return;
      const bytes = toBytes(event.data);
      if (bytes) {
        handlers.onMessage(bytes);
        return;
      }
      fail("the peer sent a frame that is not binary");
      try {
        socket.close();
      } catch {
        // Closing is best effort; the close below is what the caller sees.
      }
      finish({ code: null, reason: "a frame that is not binary" });
    };
    socket.onerror = (event) => {
      fail(typeof event?.message === "string" && event.message ? event.message : "the socket failed");
    };
    socket.onclose = (event) => {
      finish({
        code: typeof event?.code === "number" ? event.code : null,
        reason: typeof event?.reason === "string" ? event.reason : "",
      });
    };

    const out: Socket = {
      send(data) {
        socket.send(exactBuffer(data));
      },
      close(code, reason) {
        try {
          if (code === undefined) socket.close();
          else socket.close(code, reason);
        } catch {
          // A socket that cannot close properly is still given up.
        }
      },
    };
    if (options.ping !== false && typeof socket.ping === "function") {
      out.ping = () => {
        try {
          (socket.ping as () => void)();
        } catch {
          // A ping on a socket that is closing is of no use and no harm.
        }
      };
    }
    return out;
  };
}
