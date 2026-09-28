# The phone's code against the real gateway

`phone/core` has its own tests against a gateway written in TypeScript. These tests run the same
library against a real `overseerd`: the Rust gateway, a real pairing confirmed over the daemon's
local socket, fixture agents, and a forwarder that cuts connections. They are the proof that the
two implementations of the wire format (`docs/rfcs/phone-remote-protocol.md`) agree.

```bash
cargo build -p overseerd        # from the repository root; the tests use target/debug/overseerd
npm install && npm test         # here
```

Every daemon runs in its own temporary `OVERSEER_HOME`, with fixture harnesses only and with the
network advertiser off. Nothing touches the owner's daemon.
