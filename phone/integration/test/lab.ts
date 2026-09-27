/** A real overseerd for one test, a local-socket client for the Mac's side, and a forwarder that can cut connections. */

import { type ChildProcess, execFileSync, spawn } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import net from "node:net";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { type ConnectionState, type DaemonEvent, decodePairingCode, encodePairingCode, MemoryStore, PhoneClient, webSocketFactory } from "../../core/src/index.ts";

export const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");
const BIN = path.join(ROOT, "target/debug/overseerd");
const FIXTURE = path.join(ROOT, "fixtures/fake-harness/claude-fixture.js");

export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
export type Obj = { [key: string]: Json };

export const sleep = (ms: number): Promise<void> => new Promise(resolve => setTimeout(resolve, ms));

export async function until(what: string, ok: () => boolean | Promise<boolean>, ms = 15_000): Promise<void> {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await ok()) return;
    await sleep(40);
  }
  throw new Error(`${what} did not happen in ${ms} ms`);
}

function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address() as net.AddressInfo;
      server.close(() => resolve(port));
    });
  });
}

export class Daemon {
  readonly home: string;
  private child: ChildProcess | null = null;
  private readonly env: NodeJS.ProcessEnv;
  port = 0;

  constructor(mode: string, extra: Record<string, string> = {}) {
    // A short path: a Unix socket's path is limited to about 100 characters.
    this.home = mkdtempSync(path.join("/tmp", "ovs-ph"));
    writeFileSync(path.join(this.home, "fixture-mode"), mode);
    this.env = {
      ...process.env,
      OVERSEER_HOME: this.home,
      OVERSEER_GATEWAY_MDNS: "off",
      OVERSEER_CLAUDE_PATH: FIXTURE,
      OVERSEER_CODEX_PATH: "/nonexistent/harness-disabled-in-tests",
      OVERSEER_OPENCODE_PATH: "/nonexistent/harness-disabled-in-tests",
      OVERSEER_HARNESS_ENV_PASSTHROUGH: "CLAUDE_FIXTURE_MODE_FILE,FIXTURE_SLOW_MS",
      CLAUDE_FIXTURE_MODE_FILE: path.join(this.home, "fixture-mode"),
      ...extra,
    };
  }

  setMode(mode: string): void {
    writeFileSync(path.join(this.home, "fixture-mode"), mode);
  }

  async start(): Promise<void> {
    this.child = spawn(BIN, ["serve"], { env: this.env, stdio: "ignore" });
    await until("the daemon", async () => (await this.tryCall("hello")) !== undefined, 10_000);
  }

  socketPath(): string {
    return execFileSync(BIN, ["socket-path"], { env: this.env, encoding: "utf8" }).trim();
  }

  private tryCall(method: string, params: Obj = {}): Promise<Obj | undefined> {
    return new Promise(resolve => {
      const socket = net.createConnection(this.socketPath());
      let buffer = "";
      socket.setEncoding("utf8");
      socket.on("connect", () => socket.write(`${JSON.stringify({ id: 1, method, params })}\n`));
      socket.on("data", chunk => {
        buffer += chunk;
        const at = buffer.indexOf("\n");
        if (at >= 0) {
          socket.destroy();
          resolve(JSON.parse(buffer.slice(0, at)) as Obj);
        }
      });
      socket.on("error", () => resolve(undefined));
      socket.on("close", () => resolve(undefined));
    });
  }

  /** A request over the daemon's own socket: what the Mac does. */
  async mac<T extends Json = Obj>(method: string, params: Obj = {}): Promise<T> {
    const reply = await this.tryCall(method, params);
    if (!reply) throw new Error(`${method}: the daemon did not answer`);
    if (reply.error) throw new Error(`${method}: ${JSON.stringify(reply.error)}`);
    return reply.result as T;
  }

  async enablePhoneAccess(): Promise<number> {
    if (!this.port) this.port = await freePort();
    const status = await this.mac("gateway.enable", { port: this.port });
    if (status.enabled !== true) throw new Error("phone access did not turn on");
    return this.port;
  }

  /** Every event the daemon has after `after`. */
  async events(after = 0): Promise<Obj[]> {
    const all: Obj[] = [];
    let cursor = after;
    for (;;) {
      const page = (await this.mac("events.list", { after: cursor, limit: 5000 })).events as Obj[];
      if (page.length === 0) return all;
      cursor = page[page.length - 1]!.seq as number;
      all.push(...page);
    }
  }

  async run(id: string): Promise<Obj> {
    const state = await this.mac("state");
    const found = (state.runs as Obj[]).find(r => r.id === id);
    if (!found) throw new Error(`no run ${id}`);
    return found;
  }

  async waitDone(id: string, ms = 30_000): Promise<Obj> {
    await until(`run ${id} ending`, async () => !["queued", "starting", "running", "waiting_for_user"].includes((await this.run(id)).status as string), ms);
    return this.run(id);
  }

  async waitStatus(id: string, status: string, ms = 20_000): Promise<Obj> {
    await until(`run ${id} ${status}`, async () => (await this.run(id)).status === status, ms);
    return this.run(id);
  }

  kill(): void {
    this.child?.kill("SIGKILL");
    this.child = null;
  }

  async restart(): Promise<void> {
    this.kill();
    await sleep(100);
    await this.start();
  }

  async stop(): Promise<void> {
    try {
      const active = (await this.tryCall("run.active"))?.result as Obj[] | undefined;
      for (const run of active ?? []) await this.tryCall("run.interrupt", { run_id: run.id as string });
    } catch {
      /* the daemon may be gone already */
    }
    this.kill();
    try {
      execFileSync("pkill", ["-9", "-f", this.home], { stdio: "ignore" });
    } catch {
      /* nothing left to stop */
    }
    rmSync(this.home, { recursive: true, force: true });
  }
}

/** A repository with two commits of content, for fixture agents to work in. */
export function makeRepo(): string {
  const dir = mkdtempSync(path.join(tmpdir(), "ovs-phone-repo-"));
  const git = (...args: string[]): void => void execFileSync("git", args, { cwd: dir, stdio: "ignore" });
  git("init", "-q", "-b", "main");
  git("config", "user.name", "T");
  git("config", "user.email", "t@example.invalid");
  git("config", "commit.gpgsign", "false");
  writeFileSync(path.join(dir, "README.md"), "# fixture\n");
  writeFileSync(path.join(dir, "a.txt"), "a\n");
  git("add", ".");
  git("commit", "-q", "-m", "base");
  return dir;
}

/** Forwards TCP to the gateway and can cut every connection at once, like a network that drops. */
export class Forwarder {
  private readonly server: net.Server;
  private readonly open = new Set<net.Socket>();
  port = 0;
  /** Connections accepted so far. */
  accepted = 0;
  /** When true, new connections are refused: the Mac cannot be reached. */
  down = false;

  constructor(private readonly target: number) {
    this.server = net.createServer(inbound => {
      if (this.down) {
        inbound.destroy();
        return;
      }
      this.accepted += 1;
      const outbound = net.createConnection(this.target, "127.0.0.1");
      this.open.add(inbound).add(outbound);
      const end = (): void => {
        inbound.destroy();
        outbound.destroy();
        this.open.delete(inbound);
        this.open.delete(outbound);
      };
      inbound.on("error", end).on("close", end);
      outbound.on("error", end).on("close", end);
      inbound.pipe(outbound);
      outbound.pipe(inbound);
    });
  }

  listen(): Promise<number> {
    return new Promise(resolve => {
      this.server.listen(0, "127.0.0.1", () => {
        this.port = (this.server.address() as net.AddressInfo).port;
        resolve(this.port);
      });
    });
  }

  cut(): void {
    for (const socket of this.open) socket.destroy();
    this.open.clear();
  }

  close(): Promise<void> {
    this.cut();
    return new Promise(resolve => this.server.close(() => resolve()));
  }
}

/** What a phone keeps between launches. */
export interface Phone {
  client: PhoneClient;
  store: MemoryStore;
  secrets: MemoryStore;
  events: DaemonEvent[];
  states: ConnectionState[];
  truncated: number;
}

export interface PhoneOptions {
  store?: MemoryStore;
  secrets?: MemoryStore;
  extras?: { host: string; port?: number }[];
  timing?: Record<string, number>;
}

export function makePhone(options: PhoneOptions = {}): Phone {
  const store = options.store ?? new MemoryStore();
  const secrets = options.secrets ?? new MemoryStore();
  const phone: Phone = { client: undefined as unknown as PhoneClient, store, secrets, events: [], states: [], truncated: 0 };
  phone.client = new PhoneClient({
    socketFactory: webSocketFactory(WebSocket as never),
    store,
    secrets,
    random: n => crypto.getRandomValues(new Uint8Array(n)),
    now: () => Date.now(),
    app: "0.1.0-integration",
    extras: options.extras ?? [],
    timing: { backoffMinMs: 50, backoffMaxMs: 400, offRetryMs: 300, ...options.timing },
  });
  phone.client.on("event", event => void phone.events.push(event));
  phone.client.on("state", state => void phone.states.push(state));
  phone.client.on("truncated", () => void (phone.truncated += 1));
  return phone;
}

/** Pairs the way the owner does: a code from the Mac, presented by the phone, confirmed on the Mac. */
export async function pair(daemon: Daemon, phone: Phone, name: string, through?: number): Promise<Obj> {
  const started = await daemon.mac("gateway.pair_start");
  // Through a forwarder: the code names the forwarder's port, so the phone knows no other way
  // to the Mac. The key and the secret are the Mac's own.
  const code = through ? encodePairingCode({ ...decodePairingCode(started.code as string), port: through, addresses: ["127.0.0.1"] }) : (started.code as string);
  const pairing = phone.client.pair(code, name, "ios");
  let confirmed = false;
  void (async () => {
    await until("the phone asking to pair", async () => {
      const status = await daemon.mac("gateway.status");
      const waiting = ((status.pairing as Obj | null)?.waiting as Obj[] | undefined) ?? [];
      if (waiting.length === 0) return false;
      await daemon.mac("gateway.pair_confirm", { request: waiting[0]!.request as string, accept: true });
      confirmed = true;
      return true;
    });
  })().catch(() => undefined);
  const gateway = await pairing;
  if (!confirmed) throw new Error("paired without the Mac's confirmation");
  return gateway as unknown as Obj;
}
