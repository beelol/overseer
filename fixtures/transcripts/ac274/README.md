# AC274 frozen native request vectors

These are sanitized synthetic requests derived from the installed Codex0.158.0 JSON schemas and Claude2.1.288 source excerpts. They are not captured paid/native/browser flows. Every path, command, host, ID, field value and proof is synthetic. Full original schema files are copied verbatim; hashes and evidence versions are in `provenance.json`.

`native-vectors.json` specifies private native request, intended typed family, public answer shape, exact native response or explicit rejection. Codec expectations are independent of the implementation. Integer7 and string"7" are distinct identities. Requested profiles include explicit denies, globs and special roots so merely deleting entries cannot be mistaken for reduced authority. Unknown dialogs cannot forge a user cancellation; machine callbacks cannot turn into owner approvals.

Claude question mappings retain the original questions and use their text as keys. Multi-select comma-space strings follow the [official SDK input contract](https://code.claude.com/docs/en/agent-sdk/user-input). Verification/open-form/URL accepted results are deliberately unqualified until their native authority/renderer proof exists; declining/cancelling a known elicitation has its native action shape.

Task1 is an unconnected adapter codec. These vectors do not qualify pending arbitration, renderers, browser capability, external authorization, transport effects, or native execution. Task2 must bind private requests to actual stored run/session/process generation and serialize pending claims before any new family is dispatched. Existing grant/Mods/queue behavior remains intact.

Offline fixture validation: `python3 fixtures/transcripts/ac274/check-vectors.py` checks the frozen schema hashes and every schema-attributed native request/expected response, with a fail-closed checker for the schema keywords actually used here. It also validates string-versus-int64 identity and the intentionally invalid ID vectors. This checks fixture consistency; it is not an adapter RED/green run.
