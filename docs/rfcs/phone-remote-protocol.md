# Phone remote: wire protocol (gateway v1)

Status: working specification for Gate N ([design](phone-remote.md)). The daemon (`daemon/src/gateway/`)
and the app (`phone/core/`) both implement this document. A change here changes both sides.

## Transport

- WebSocket over TCP, binary frames only. Default port `47810`. Path `/v1`.
- The gateway listens only while phone access is on. It accepts peers from loopback, private and
  link-local ranges (`127/8`, `10/8`, `172.16/12`, `192.168/16`, `169.254/16`, `::1`, `fc00::/7`,
  `fe80::/10`) plus ranges the owner adds. Other peers are closed before the WebSocket upgrade.
- The WebSocket upgrade is the only thing an unauthenticated peer ever receives.

## Keys

- Every key is an X25519 key pair. The gateway has one static pair, created the first time phone
  access is turned on. Each device creates its own pair when it pairs.
- The gateway's private key is kept in the daemon's owner-only data directory
  (`gateway/identity.key`, mode 0600). A device's private key is kept in the system keystore.
- A key's fingerprint is the first 16 hex characters of SHA-256 over the public key.

## Handshake

Noise framework, revision 34.

| Use | Pattern | Pre-shared key |
| --- | --- | --- |
| Session | `Noise_IK_25519_ChaChaPoly_SHA256` | none |
| Pairing | `Noise_IKpsk1_25519_ChaChaPoly_SHA256` | SHA-256 of the pairing secret |

The pairing secret enters the first message (`psk1`), so the gateway knows the phone holds the
pairing code before the owner is asked anything. It must not be `psk2`: there the first message
does not depend on the secret, and a phone without it would reach the confirmation.

The prologue is the ASCII text `overseer-gateway-v1`. The device is the initiator; the gateway's
static public key is known to it from pairing.

First frame, device to gateway:

```
byte 0      version, 0x01
byte 1      kind: 0x01 session, 0x02 pairing
bytes 2..   Noise handshake message 1, with an encrypted JSON payload
```

Payload of message 1:

```json
{"device": "<device id, empty when pairing>", "name": "Bilal's iPhone", "platform": "ios",
 "app": "0.1.0", "counter": 1790000000000}
```

`counter` is `max(current time in ms, last counter used + 1)`. The gateway stores the highest
counter it accepted per device and refuses a message whose counter is not higher, so a recorded
handshake cannot be replayed.

Second frame, gateway to device: Noise handshake message 2, with an encrypted JSON payload:

```json
{"protocol": 1, "device": "<device id>", "scope": "full", "gateway": "<Mac name>",
 "fingerprint": "<gateway key fingerprint>"}
```

The gateway sends message 2 only when message 1 decrypts and:

- session: the static key in it belongs to a paired device that is not revoked, and the counter
  is higher than the last one;
- pairing: pairing is open, the pre-shared key matched (the message decrypted), and the owner
  confirmed the device on the Mac. The connection stays open, silent, while the owner decides
  (60 s at most).

Anything else closes the connection without a reply.

## Transport messages

After the handshake every WebSocket frame is one Noise transport message. Its plaintext is:

```
byte 0      0x00 more chunks follow, 0x01 last chunk
bytes 1..   up to 65,000 bytes of the message
```

Chunks of one message are sent in order with nothing between them. The joined chunks are one
UTF-8 JSON value: a message of the daemon's protocol. A frame that fails to decrypt, arrives out
of order or is malformed ends the session. Requests from a device are limited to 1 MiB after
joining, the same as the local socket. Replies and events are limited to 64 MiB.

The device sends a WebSocket ping every 20 s. The gateway ends a session that was silent for 60 s.

## Messages

The daemon's protocol, unchanged: requests `{"id", "method", "params"}`, replies
`{"id", "result"}` or `{"id", "error": {"code", "message", "data"?}}`, and notifications
`{"method", "params"}` (`event`, `replayed`, `resync`).

Additions for devices:

- **Request id.** A request whose method is of class *control* carries `"request_id"`, a UUID the
  device creates once per action and repeats on every retry. The gateway stores the outcome per
  device and request id for 24 hours and answers a retry with the stored outcome. A control
  request without one is refused with `request_id_required`.
- **Classes.** Every method has a class in `protocol/protocol.json`: `self` (the calling device's
  own session and settings), `read`, `control` or `mac_only`. *Watch only* devices may call `self`
  and `read`. *Full control* devices may also call `control`. No device may call `mac_only`.
  Refusals use the codes `watch_only` and `mac_only`.
- **Source.** Every control request a device makes is recorded as an event of kind
  `remote_command` with the source `phone:<device name>`.
- **Gateway notices.** `{"method": "gateway", "params": {"state": "off"}}` is sent to every
  session before phone access is turned off. `{"state": "revoked"}` is sent to a device before
  its session ends because it was revoked.

Error codes added: `watch_only`, `mac_only`, `request_id_required`, `already_answered`
(with `data`: the first answer), `unknown_method`.

## Pairing code

The QR code and the typed code carry the same text: `OVSR1-` followed by unpadded base32 of

```
byte 0        version, 0x01
bytes 1..32   the gateway's static public key
bytes 33..48  the pairing secret, 16 random bytes
bytes 49..50  the port, big endian
byte 51       number of addresses
then, each    1 byte length and the address as ASCII (an IP address or a host name)
```

The secret works once and for two minutes. Five failed pairing attempts close pairing.

## Discovery

While phone access is on the gateway advertises `_overseer._tcp` on its port with the TXT record
`fp=<gateway key fingerprint>`. A device looks for its gateway's fingerprint, never for a name.
A device also tries the addresses it last reached, the addresses from the pairing code, and the
addresses its platform adds (`127.0.0.1` on the iOS simulator, `10.0.2.2` on the Android emulator).
Whatever answers must complete the handshake with the paired key.
