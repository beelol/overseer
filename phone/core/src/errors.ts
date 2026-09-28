/**
 * Errors of the library. Every error has a stable `code` the app can act on and a message in
 * plain words. No message ever contains key material, a pairing secret or decrypted content.
 */

import type { JsonValue } from "./json.ts";

/** Why a connection to one address did not become a session. */
export type ConnectFailure =
  /** Nothing answered at the address, or the WebSocket did not open in time. */
  | "unreachable"
  /** The peer accepted the connection and closed it without answering the handshake. */
  | "refused"
  /** The peer answered the handshake without proving the gateway's key. */
  | "impostor"
  /** The peer did not answer the handshake in time. */
  | "timeout"
  /** The gateway proved its key and speaks a protocol this app does not. */
  | "incompatible"
  /** The attempt was cancelled by this side. */
  | "cancelled";

export type ErrorCode =
  | ConnectFailure
  /** Noise: a message did not decrypt or authenticate. */
  | "decrypt"
  /** Noise or framing: a message has an impossible length. */
  | "length"
  /** Noise: used in the wrong order, or after a failure. */
  | "state"
  /** Noise: the nonce counter is used up; no more messages may be sent with this key. */
  | "nonce_exhausted"
  /** A key has the wrong size or is not a usable X25519 key. */
  | "bad_key"
  /** Framing: a chunk flag that is neither "more" nor "last". */
  | "bad_flag"
  /** Framing: a chunk larger than the protocol allows. */
  | "chunk_too_large"
  /** Framing: the joined message is larger than the limit. */
  | "message_too_large"
  /** A pairing code that cannot be read. `PairingCodeError.reason` says why. */
  | "bad_pairing_code"
  /** The session received something that is not a protocol message. */
  | "protocol"
  /** The session or the client is closed. */
  | "closed"
  /** Nothing was received for too long. */
  | "idle"
  /** A request is larger than 1 MiB. */
  | "request_too_large"
  /** There is no session and this request is not kept for later. */
  | "not_connected"
  /** The session ended before the reply arrived. */
  | "connection_lost"
  /** No reply arrived in time. */
  | "request_timeout"
  /** The app is not paired. */
  | "unpaired"
  /** The app is paired already; pairing happens once. */
  | "already_paired"
  /** Pairing failed at every address. */
  | "pairing_failed"
  /** The same request id was given with another method or other parameters. */
  | "request_id_conflict"
  /** A request id the gateway would refuse (8 to 64 letters, digits and hyphens). */
  | "bad_request_id"
  /** The parameters of a request are not a JSON object. */
  | "invalid_params"
  /** A stored value could not be read. */
  | "storage";

/** The base of every error this library throws. */
export class OverseerError extends Error {
  readonly code: ErrorCode;

  constructor(code: ErrorCode, message: string) {
    super(message);
    this.name = "OverseerError";
    this.code = code;
  }
}

/** The error object of a reply from the daemon or the gateway. */
export interface ReplyError {
  readonly code: string;
  readonly message: string;
  readonly data?: JsonValue;
}

/**
 * A request was answered with an error. `code`, `message` and `data` are the reply's own:
 * for example `already_answered` carries the first answer in `data`, and `outcome_unknown`
 * says that the Mac stopped while the request ran, so it was not run again.
 *
 * An error reply is an answer. The request is never sent again because of it.
 */
export class RequestError extends Error {
  readonly code: string;
  /** What the reply carried besides its code and message, or undefined. */
  readonly data: JsonValue | undefined;
  /** The request id of a control request, so the app can find its outbox entry. */
  readonly requestId: string | undefined;

  constructor(error: ReplyError, requestId?: string) {
    super(error.message || error.code);
    this.name = "RequestError";
    this.code = error.code;
    this.data = error.data;
    this.requestId = requestId;
  }
}
