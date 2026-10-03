# Owned decoder exit correction: five exact checks pass

Root session16949 is terminal: all five exact selected cases passed once at
docs head247e505, whose production correction is
`f56816dd1ed1e6d22d0082411f96c1ae0e25bd94`. Fresh standalone build21.195s and
test compilation22.349s passed; source/dep-info/executable hashes and
fresh:false artifact receipts are retained in [receipt.json](receipt.json).
Cleanup found zero owned/zero other leftovers. Process inventory is omitted;
[SHA256.json](SHA256.json) retains the original receipt hash and copied files.

The successful decoder-exit witness now reaches valid folder selection. Its
malformed-stdout and actual failed-native-decode controls reach their intended
refusal guards; the held-live injected unreadable-memory control still refuses.
The incumbent truncated-WAV/undecodable-MP3 case also passes. No assertion or
deadline was weakened. The preserved [baseline](../pack-exit-baseline-1f0d4bb/README.md)
records the earlier memory refusal and unreached later negative guards.

This establishes the bounded owned-child exit correction and selected refusal
controls. It does not establish the cause of the intermittent missing auth cue,
real OS memory enforcement, native FD playback, whole-group cleanup, shutdown,
canonical preview integration, spoken assets, full-suite readiness or AC closure.
The three actual shutdown failures remain a separate implementation slice.
