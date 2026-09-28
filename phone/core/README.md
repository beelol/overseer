# @overseer/phone-core

The platform-neutral core of Overseer's phone app, in TypeScript: the encrypted session with the
Mac, pairing, and the client of the daemon's protocol. The phone is one more client of the same
daemon that VS Code and the terminal UI talk to; it reaches it through the gateway
([wire protocol](../../docs/rfcs/phone-remote-protocol.md), [design](../../docs/rfcs/phone-remote.md)).

Nothing in `src/` imports React Native, Expo or Node. The same files run unchanged in Node 24
and in React Native's Hermes. Everything that belongs to a platform is injected.

## Commands

Run them in `phone/core/`. They need Node 24 and nothing else: no simulator, no Rust, no daemon.

| Command | What it does |
| --- | --- |
| `npm install` | Installs the three runtime packages and the test tools. |
| `npm test` | Type check (`tsc --noEmit`, source and tests), then every test with vitest. |
| `npm run typecheck` | The type check alone. |
| `npm run bench` | Times both handshakes and a message of 1 MiB in Node. |
| `npm run check:hermes` | Bundles the library, applies React Native's Babel preset and compiles it with the Hermes compiler from `phone/node_modules`. Skipped, with a line that says so, where those tools are not installed. It shows that Hermes accepts the code; it does not run it in Hermes. |

## What is in it

| File | What it holds |
| --- | --- |
| `src/noise.ts` | Noise, revision 34: `Noise_IK_25519_ChaChaPoly_SHA256` (sessions) and `Noise_IKpsk1_25519_ChaChaPoly_SHA256` (pairing), both roles, prologue `overseer-gateway-v1`. |
| `src/frames.ts` | Chunks of 65,000 bytes behind a flag byte; `seal` and `Opener`, as in `daemon/src/gateway/noise.rs`. |
| `src/pairing-code.ts`, `src/base32.ts` | The `OVSR1-` pairing code. |
| `src/socket.ts` | `Socket` and `SocketFactory`, and `webSocketFactory` for the standard `WebSocket`. |
| `src/session.ts`, `src/counter.ts` | The handshakes over a socket, the handshake counter, keepalive, JSON messages in and out. |
| `src/client.ts`, `src/client-types.ts` | `PhoneClient`: states, reconnecting, addresses, pairing, events, requests. |
| `src/control-queue.ts`, `src/outbox.ts`, `src/requests.ts` | Control requests sent exactly once; the stored outbox; replies matched to requests. |
| `src/pairing-store.ts`, `src/saved-value.ts` | What pairing leaves on the phone; values written behind the code that changes them. |
| `src/addresses.ts`, `src/backoff.ts`, `src/uuid.ts`, `src/emitter.ts`, `src/bytes.ts`, `src/json.ts`, `src/errors.ts`, `src/platform.ts` | Small parts. |
| `src/index.ts` | The exports. |
| `test/mock-gateway.ts` | A gateway for tests: the responder of both handshakes, the counter rule, pairing with a confirm hook, a tiny daemon, and faults on demand. |

The types of the protocol and the class of every method come from
`phone/protocol/protocol.generated.ts`, which is generated from `protocol/protocol.json`.

## What the app injects

| Interface | What it is | On the phone |
| --- | --- | --- |
| `random(n): Uint8Array` | `n` bytes from the secure random generator. Device keys, ephemeral keys, request ids and jitter come from it. | `expo-crypto` |
| `now(): number` | Milliseconds since 1970. | `Date.now` |
| `KeyValueStore` | `get`, `set`, `delete` of strings, all asynchronous. | SQLite or AsyncStorage |
| `SecretStore` | The same shape, for keys. | `expo-secure-store` (Keychain, Keystore) |
| `SocketFactory` | Opens a socket of binary frames. | `webSocketFactory(WebSocket)` |
| `log(message)` | Optional. Lines never contain keys, secrets, the pairing code or message content. | |
| `timers` | Optional; the default is the global `setTimeout`. | |

`MemoryStore` is the fake of both stores, for tests.

```ts
import { PhoneClient, webSocketFactory } from "@overseer/phone-core";

const client = new PhoneClient({ socketFactory: webSocketFactory(WebSocket), store, secrets, random, now: Date.now, app: "0.1.0" });
client.on("state", (state) => show(state));            // unpaired, connecting, online, reconnecting, off, unreachable, revoked
client.on("event", async (event, { live }) => apply(event, live));
client.on("truncated", () => reloadStateAndSaySo());
client.on("outbox", (entry) => showMessage(entry));   // queued, sending, done, failed
await client.start();                                  // connects by itself when paired

await client.pair(code, "Bilal's iPhone", "ios");      // once
const state = await client.request("state", {});       // typed by the method
await client.request("run.follow_up", { run_id, prompt });  // a control method: exactly once
client.wake();                                         // the app came to the foreground, the network changed
```

### Using it from the app

- **TypeScript.** Imports in `src/` end in `.ts`, so that Node runs the source as it is. A project
  that imports the source needs `"allowImportingTsExtensions": true`. With that one option the
  library passes the app's own `tsconfig.json`, `noUncheckedIndexedAccess` included (checked).
- **Jest.** The noble packages are ES modules only. A Jest suite that loads this library must
  let Babel transform them (`transformIgnorePatterns`).
- **Keys of the stores.** They start with `overseer.`; `namespace` changes that. In the
  `KeyValueStore`: `pairing`, `counter`, `cursor`, `lastContact`, `lastAddress`, `outbox`. In the
  `SecretStore`: `keys` (the device's key pair and the gateway's public key, as one value of
  about 250 characters, so that they are written together).

## What the tests show

`npm test` runs 13 files. The ones that answer a criterion:

| Criterion | Test file | What it shows |
| --- | --- | --- |
| AC-118 | `noise.test.ts`, `session.test.ts` | Both shared vectors byte for byte (`message1`, `message2`, the handshake hash, every transport frame), in both roles. A wrong key, a tampered message 1, a tampered, replayed, reordered or truncated frame are refused. Nothing of the protocol is readable in the frames that crossed the wire. A recorded first frame that is sent again is refused. |
| AC-121 | `client-resume.test.ts` | The connection is cut at 100 random points during a stream of numbered events, in the middle of replays and between the chunks of one message; the client's sequence is the log of the mock. The same across restarts of the app. Truncated history is reported. |
| AC-122 | `client-exactly-once.test.ts` | One request id sent three times at once runs once and gets three identical replies. Cut after the request ran and before the reply, the retry is automatic and it ran once. A request made without a connection is sent once afterwards. The outbox survives a restart. |
| AC-141 | `client-pair-once.test.ts`, `client-states.test.ts` | After pairing: 20 cycles, a changed address, a gateway restart, phone access off and on, app restarts, thirty days. The pairing path ran once, counted in the app's calls, on the wire and at the gateway. |

The pairing test the change to `psk1` asked for is in both `noise.test.ts` (a gateway with the
right secret cannot read message 1 of a phone with a wrong one) and `session.test.ts` and
`client-states.test.ts` (the mock refuses such a phone and its confirm hook is never called).

## Decisions where the specification was silent

### Handshake and session

1. **Keepalive.** Every 20 s the session sends the request `{"id":0,"method":"ping","params":{}}`
   as an ordinary encrypted message, and also a WebSocket ping where the socket has one. The
   standard `WebSocket` of browsers and of Node cannot ping; React Native's can, and never shows
   the pong. The request works on every transport, a relay included, and its reply is the sign of
   life the phone needs. Request ids of the client start at 1; the session drops the reply to 0.
2. **A silent gateway.** The session ends as `idle` when nothing genuine arrived for 60 s, which
   is three keepalives. Only frames that decrypted count.
3. **Waiting times.** The socket may take 3 s to open. The 10 s (session) and 70 s (pairing) run
   from the moment the first frame was sent.
4. **The first frame** may be 4,096 bytes, the gateway's limit. A device name too long for that
   is refused before anything is sent.
5. **The answer of the gateway.** `protocol` must be 1 and `device` must not be empty, otherwise
   the address fails as `incompatible`. A `fingerprint` that is not the one of the proven key is logged and ignored:
   the handshake proves the key, the text proves nothing.
6. **How an address can fail.** `unreachable` (nothing answered), `refused` (the peer closed
   without answering the handshake), `impostor` (the peer answered and could not prove the key),
   `timeout`, `incompatible`, and `hello_failed`. The real gateway never answers what it does not
   accept, so a Mac with another key is `refused`; `impostor` is for a peer that answers anyway.
   Both are passed over.
7. **The counter** is one value for the phone, kept when a pairing is forgotten, because it must
   never go back. When it cannot be stored, nothing is sent.
8. **A public key of low order** gives an all-zero shared secret; the handshake refuses it.
9. **The nonce** is kept as two halves of 32 bits. The last value, 2^64 - 1, is refused, as
   Noise requires. A failed decryption does not move it, as in `snow`.
10. **Text frames** are an error and end the connection.
11. **Limits.** A request over 1 MiB is refused before it is encrypted. A reply or event may be
    64 MiB after joining.

### Pairing

12. **The session of a pairing is not used.** When the gateway answered, the connection is
    closed and a session is opened by the keys, so every session starts the same way.
13. **Addresses.** The addresses of the code are tried, then the platform's extras, each with the
    code's port. Every failure is reported in `PairingError.attempts`.
14. **Reading a code** is strict where the daemon's decoder is lenient: characters outside the
    alphabet, a length that base32 cannot have, leftover bits that are not zero, a port of zero
    and an address that is not a host name or an IP address are errors. No error repeats any part
    of the code.
15. **What is stored where.** The keys go to the `SecretStore`, written before the record, which
    goes to the `KeyValueStore`. A record without its keys, or a private key that does not belong
    to the stored public key, is no pairing.
16. **New keys for every pairing.** `pair` refuses when the app is paired.

### Connection

17. **Revoked only on the gateway's word.** The pairing is forgotten on the notice
    `{"state":"revoked"}` and on an error reply with the code `revoked`, both inside a session
    that proved the gateway's key. Nothing that happens before that proof can make the phone
    delete its keys. A device revoked while it was away sees `refused` and the state
    `unreachable`; see "What the protocol cannot tell the phone".
18. **`off` is not stored.** After a restart of the app while phone access is off, the state is
    `unreachable` until the gateway is back.
19. **The wait between passes.** From 0.5 s doubling to 10 s; half of each wait is fixed and half
    is random. The first pass after a lost session waits too (0.25 to 0.5 s), so that a gateway
    that accepts and drops at once is not hammered. The waits start over when a session was
    greeted and subscribed, not before: a gateway that greets and then refuses is retried ever
    more slowly. `wake()` and a newly discovered address end the wait at once. While `off` the
    wait is 10 s.
20. **`wake()` on a live session** asks for a sign of life and replaces the session when none
    comes within 3 s: after the app was in the background the socket is often dead without
    having said so.
21. **`stop()`** ends everything and stores what is pending. The state afterwards is
    `reconnecting` when paired.
22. **`lastContact`** counts frames that decrypted. It is stored at most every 5 s while frames
    arrive, and when a session ends.
23. **`hello` is part of connecting.** The state is `online` when `hello` was answered. Its
    result is kept in `client.hello`; the name of the Mac and the scope are stored when they
    changed.

### Events

24. **The cursor moves after the app applied the event.** Listeners of `event` may return a
    promise; the next event waits for it. The cursor is written behind (one write at a time, the
    newest value wins) and `stop()` waits for it. An app that is killed receives again what came
    after the stored cursor, and nothing is missing; within one run and across `stop()` every
    event arrives exactly once.
25. **News and history.** Each event comes with `live`: false until the daemon said `replayed`,
    and again after `resync`. It is the flag VS Code's client has.
26. **A new pairing starts at cursor 0.**

### Requests

27. **The class decides.** `request` takes the methods a phone may call, typed by
    `protocol.generated.ts`; a method of class `control` goes through the outbox. `requestRaw`
    takes any name; its `control` option counts only for a method the description does not know.
28. **The request id** is beside `id`, `method` and `params`, not inside `params`
    (`run.permission` has a parameter of the same name for another thing).
29. **Order.** Queued requests are sent in the order they were made, one after the other on the
    wire, without waiting for each reply.
30. **An error reply is an answer.** The entry becomes `failed`, leaves the stored outbox, and is
    never sent again. This holds for `outcome_unknown` above all. The error reaches the caller as
    a `RequestError` with `code`, `message`, `data` and the request id.
31. **Too old to retry.** An entry that was sent, never answered, and whose first sending is more
    than 24 hours ago is not sent again: it fails with `outcome_unknown`. The gateway promises to
    keep outcomes for 24 hours, and after that a retry could run the action twice. An entry that
    was never sent has no such limit.
32. **No time limit by default** for a control request: its promise waits for the answer.
    `timeoutMs` ends the waiting and leaves the request in the outbox.
33. **The same request id again** joins the request that exists; it is not sent twice on one
    connection. After the answer it returns the answer. With another method or other parameters
    it is refused (`request_id_conflict`).
34. **What cannot be stored is not sent** (`storage`).
35. **Requests that only read are not kept.** Without a session they fail with `not_connected`;
    when the session ends first, with `connection_lost`.
36. **Answered entries** stay in memory for the app to show, the latest 50, until `dismiss`.

### Code

37. **Imports end in `.ts`**, and the syntax is erasable only (`erasableSyntaxOnly`), so Node 24
    runs the source without a build step.
38. **UTF-8.** `TextEncoder` and `TextDecoder` are used where the platform has them and they are
    strict; otherwise the library's own strict code. Both are tested against each other.
39. **No globals.** Timers and the text codecs are reached through `globalThis` in two files.
    The compiler runs with `lib: ["ES2022"]` and no types of any platform, so a use of Node or
    the DOM does not compile.

## What the protocol cannot tell the phone

These are limits of the protocol as it is, not of this library. They are listed so that the app
and the daemon can decide about them.

- **Revoked while away.** The gateway closes the connection of a revoked device without a word,
  as it does for every peer it does not accept. A phone that was not connected when it was
  revoked can never learn it: it shows `unreachable`, with `refused` for every address, and keeps
  its keys. The same happens when the notice is lost on its way.
- **A daemon whose event log started over** with sequence numbers below the phone's cursor sends
  nothing until the numbers pass the cursor, and `history_truncated` is false.

## Speed

`npm run bench`, Node 24.20 on the development Mac (Apple silicon), medians of three runs. The
Mac was building other things during all of them (load average 15 to 41 where it was read), so
these are upper bounds, and the spread between the runs is the load, not the code.

| | Run 1 | Run 2 (load 41) | Run 3 (load 15) |
| --- | --- | --- | --- |
| Pairing handshake, both sides in one process | 6.3 ms | 15.1 ms | 8.2 ms |
| Session handshake, both sides in one process | 6.0 ms | 9.2 ms | 7.7 ms |
| Pairing handshake, the phone's side alone | 2.9 ms | 4.8 ms | 4.0 ms |
| Session handshake, the phone's side alone | 2.9 ms | 4.5 ms | 3.8 ms |
| Pairing over a WebSocket on loopback, confirmed at once | 7.1 ms | 27.7 ms | 9.2 ms |
| Session over a WebSocket on loopback | 6.8 ms | 26.2 ms | 10.3 ms |
| Seal a message of 1 MiB (17 frames) | 3.2 ms | 7.2 ms | 4.0 ms |
| Open a message of 1 MiB | 3.2 ms | 7.2 ms | 4.1 ms |

These are numbers for Node. The key exchange is arithmetic on large integers, which Hermes does
much more slowly than Node; the time on a phone has to be measured in the app.
