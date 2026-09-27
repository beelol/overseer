import type { AddressInfo } from "node:net";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { WebSocket as WsWebSocket, WebSocketServer } from "ws";
import type { CloseInfo, Socket, SocketFactory, WebSocketConstructor } from "../src/socket.ts";
import { webSocketFactory } from "../src/socket.ts";
import { waitFor } from "./helpers.ts";

let server: WebSocketServer;
let url: string;
let pings = 0;

beforeEach(async () => {
  pings = 0;
  server = new WebSocketServer({ host: "127.0.0.1", port: 0 });
  await new Promise<void>((resolve) => server.once("listening", resolve));
  url = `ws://127.0.0.1:${(server.address() as AddressInfo).port}/v1`;
  server.on("connection", (ws) => {
    ws.on("ping", () => (pings += 1));
    ws.on("message", (data, isBinary) => {
      const bytes = new Uint8Array(data as Buffer);
      if (!isBinary) return;
      if (bytes[0] === 0xff) ws.send("text, which the protocol never uses");
      else if (bytes[0] === 0xfe) ws.close(4001, "told to go");
      else if (bytes[0] === 0xfd) ws.terminate();
      else ws.send(bytes, { binary: true });
    });
  });
});

afterEach(async () => {
  for (const client of server.clients) client.terminate();
  await new Promise<void>((resolve) => server.close(() => resolve()));
});

interface Seen {
  socket: Socket;
  opened: number;
  messages: Uint8Array[];
  errors: string[];
  closes: CloseInfo[];
  order: string[];
}

function open(factory: SocketFactory, to: string = url): Seen {
  const seen: Seen = { socket: undefined as unknown as Socket, opened: 0, messages: [], errors: [], closes: [], order: [] };
  seen.socket = factory(to, {
    onOpen: () => {
      seen.opened += 1;
      seen.order.push("open");
    },
    onMessage: (data) => {
      seen.messages.push(data);
      seen.order.push("message");
    },
    onError: (error) => {
      seen.errors.push(error.message);
      seen.order.push("error");
    },
    onClose: (info) => {
      seen.closes.push(info);
      seen.order.push("close");
    },
  });
  return seen;
}

const implementations: [string, WebSocketConstructor][] = [
  ["the global WebSocket of Node 24 (the same interface as React Native's)", WebSocket],
  ["the WebSocket of the ws package", WsWebSocket],
];

for (const [name, implementation] of implementations) {
  describe(`the adapter over ${name}`, () => {
    const factory = webSocketFactory(implementation);

    it("opens, and carries binary frames both ways as Uint8Array", async () => {
      const seen = open(factory);
      await waitFor(() => seen.opened === 1, "open");
      const frames = [Uint8Array.of(1, 2, 3), new Uint8Array(0), new Uint8Array(65_017).map((_, i) => i % 250)];
      for (const frame of frames) seen.socket.send(frame);
      await waitFor(() => seen.messages.length === 3, "three echoes");
      expect(seen.messages).toEqual(frames);
      for (const message of seen.messages) expect(message).toBeInstanceOf(Uint8Array);
      seen.socket.close();
      await waitFor(() => seen.closes.length === 1, "close");
    });

    it("sends exactly the bytes of a view into a larger buffer", async () => {
      const seen = open(factory);
      await waitFor(() => seen.opened === 1, "open");
      const large = new Uint8Array(100).map((_, i) => i);
      seen.socket.send(large.subarray(10, 20));
      await waitFor(() => seen.messages.length === 1, "the echo");
      expect(seen.messages[0]).toEqual(Uint8Array.of(10, 11, 12, 13, 14, 15, 16, 17, 18, 19));
      seen.socket.close();
    });

    it("reports a close by the peer once, with its code, and nothing after it", async () => {
      const seen = open(factory);
      await waitFor(() => seen.opened === 1, "open");
      seen.socket.send(Uint8Array.of(0xfe));
      await waitFor(() => seen.closes.length === 1, "close");
      expect(seen.closes).toEqual([{ code: 4001, reason: "told to go" }]);
      seen.socket.close();
      seen.socket.close();
      await new Promise((resolve) => setTimeout(resolve, 30));
      expect(seen.order.filter((e) => e === "close").length).toBe(1);
      expect(seen.order.at(-1)).toBe("close");
    });

    it("reports a connection that was cut", async () => {
      const seen = open(factory);
      await waitFor(() => seen.opened === 1, "open");
      seen.socket.send(Uint8Array.of(0xfd));
      await waitFor(() => seen.closes.length === 1, "close");
      expect(seen.order.at(-1)).toBe("close");
    });

    it("reports an address where nothing listens as an error and a close, never as open", async () => {
      const port = (server.address() as AddressInfo).port;
      await new Promise<void>((resolve) => server.close(() => resolve()));
      const seen = open(factory, `ws://127.0.0.1:${port}/v1`);
      await waitFor(() => seen.closes.length === 1, "close");
      expect(seen.opened).toBe(0);
      expect(seen.errors.length).toBeGreaterThanOrEqual(1);
      expect(seen.order.at(-1)).toBe("close");
      server = new WebSocketServer({ host: "127.0.0.1", port: 0 });
      await new Promise<void>((resolve) => server.once("listening", resolve));
    });

    it("treats a text frame as an error and closes", async () => {
      const seen = open(factory);
      await waitFor(() => seen.opened === 1, "open");
      seen.socket.send(Uint8Array.of(0xff));
      await waitFor(() => seen.closes.length === 1, "close");
      expect(seen.messages).toEqual([]);
      expect(seen.errors).toEqual(["the peer sent a frame that is not binary"]);
    });
  });
}

describe("the adapter", () => {
  it("offers ping only where the implementation has one", async () => {
    const plain = open(webSocketFactory(WebSocket));
    const withPing = open(webSocketFactory(WsWebSocket));
    await waitFor(() => plain.opened === 1 && withPing.opened === 1, "open");
    expect(plain.socket.ping).toBeUndefined();
    expect(typeof withPing.socket.ping).toBe("function");
    withPing.socket.ping?.();
    await waitFor(() => pings === 1, "the ping");
    plain.socket.close();
    withPing.socket.close();
  });

  it("sets binaryType to arraybuffer", () => {
    const made: { binaryType: string }[] = [];
    class Fake {
      binaryType = "blob";
      constructor(_url: string) {
        made.push(this);
      }
      send(_data: ArrayBuffer): void {}
      close(): void {}
    }
    open(webSocketFactory(Fake));
    expect(made.map((m) => m.binaryType)).toEqual(["arraybuffer"]);
  });

  it("reports a constructor that throws after it returned, as an error and a close", async () => {
    class Broken {
      constructor(_url: string) {
        throw new Error("not allowed here");
      }
      send(_data: ArrayBuffer): void {}
      close(): void {}
    }
    const seen = open(webSocketFactory(Broken));
    expect(seen.order).toEqual([]);
    await waitFor(() => seen.closes.length === 1, "close");
    expect(seen.order).toEqual(["error", "close"]);
    expect(() => seen.socket.send(Uint8Array.of(1))).toThrowError(/not open/);
  });

  it("accepts an ArrayBuffer of another realm and a typed array as a frame", async () => {
    const handlers: { onmessage?: (event: { data: unknown }) => void } = {};
    class Fake {
      binaryType = "";
      onopen = null;
      onerror = null;
      onclose = null;
      set onmessage(handler: (event: { data: unknown }) => void) {
        handlers.onmessage = handler;
      }
      constructor(_url: string) {}
      send(_data: ArrayBuffer): void {}
      close(): void {}
    }
    const seen = open(webSocketFactory(Fake));
    const { runInNewContext } = await import("node:vm");
    const foreign = runInNewContext("new Uint8Array([7, 8, 9]).buffer") as ArrayBuffer;
    expect(foreign instanceof ArrayBuffer).toBe(false);
    handlers.onmessage?.({ data: foreign });
    handlers.onmessage?.({ data: new Uint8Array([1, 2, 3, 4]).subarray(1, 3) });
    handlers.onmessage?.({ data: Buffer.from([5, 6]) });
    expect(seen.messages).toEqual([Uint8Array.of(7, 8, 9), Uint8Array.of(2, 3), Uint8Array.of(5, 6)]);
  });
});
