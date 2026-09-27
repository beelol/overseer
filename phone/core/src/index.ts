/** @overseer/phone-core: the platform-neutral core of Overseer's phone app. */

export {
  type Address,
  type AddressInput,
  addressKey,
  candidateAddresses,
  type CandidateSources,
  DEFAULT_PORT,
  formatAddress,
  GATEWAY_PATH,
  gatewayUrl,
  isUsableAddress,
  parseAddress,
} from "./addresses.ts";
export { Backoff } from "./backoff.ts";
export { base32Decode, base32Encode, Base32Error, type Base32Problem } from "./base32.ts";
export { bytesToHex, concatBytes, copyBytes, equalBytes, hexToBytes, utf8Decode, utf8Encode, wipe } from "./bytes.ts";
export { PairingError, PhoneClient } from "./client.ts";
export {
  type ClientEvents,
  type ClientOptions,
  type ClientTiming,
  type ConnectAttempt,
  CONNECTION_STATES,
  type ConnectionState,
  type DaemonEvent,
  DEFAULT_CLIENT_TIMING,
  type EventInfo,
  type ForgetReason,
  type GatewayInfo,
  type RawRequestOptions,
  type RequestOptions,
} from "./client-types.ts";
export { HandshakeCounter } from "./counter.ts";
export { type ConnectFailure, type ErrorCode, OverseerError, type ReplyError, RequestError } from "./errors.ts";
export { CHUNK_SIZE, FLAG_LAST, FLAG_MORE, FrameError, Joiner, MAX_REPLY_BYTES, MAX_REQUEST_BYTES, Opener, seal, splitChunks } from "./frames.ts";
export { isJsonObject, type JsonObject, type JsonValue } from "./json.ts";
export {
  CipherState,
  fingerprint,
  generateKeyPair,
  Handshake,
  type HandshakeKind,
  type HandshakeOptions,
  type HandshakeRole,
  KEY_LENGTH,
  type KeyPair,
  MAX_NOISE_MESSAGE,
  NoiseError,
  type NonceStart,
  PATTERN_PAIRING,
  PATTERN_SESSION,
  PROLOGUE,
  pskFromSecret,
  publicKeyOf,
  TAG_LENGTH,
  type TransportKeys,
} from "./noise.ts";
export { type OutboxEntry, type OutboxState } from "./outbox.ts";
export {
  decodePairingCode,
  type EncodeOptions,
  encodePairingCode,
  PAIRING_CODE_PREFIX,
  PAIRING_CODE_VERSION,
  PAIRING_SECRET_LENGTH,
  type PairingCode,
  PairingCodeError,
  type PairingCodeProblem,
} from "./pairing-code.ts";
export { type Pairing } from "./pairing-store.ts";
export {
  type Clock,
  type KeyValueStore,
  type Log,
  MemoryStore,
  platformTimers,
  type RandomSource,
  type SecretStore,
  type TimerHandle,
  type Timers,
} from "./platform.ts";
export {
  ConnectError,
  type ConnectFailureCode,
  connectSession,
  DEFAULT_SESSION_TIMING,
  type DeviceHello,
  FRAME_VERSION,
  type GatewayHello,
  KEEPALIVE_ID,
  KIND_PAIRING,
  KIND_SESSION,
  MAX_FIRST_FRAME,
  PROTOCOL_VERSION,
  type Session,
  type SessionAttempt,
  type SessionConfig,
  type SessionEnd,
  type SessionEndReason,
  type SessionHandlers,
  type SessionTiming,
} from "./session.ts";
export { type CloseInfo, type Socket, type SocketFactory, type SocketHandlers, type WebSocketConstructor, type WebSocketLike, webSocketFactory } from "./socket.ts";
export { isRequestId, uuidV4 } from "./uuid.ts";
