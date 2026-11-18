# Handoff to the primary implementation agent

**NOT PRODUCTION-READY.** Branch `review/zos-int07-security-agent2` is a stacked Draft PR against the reviewed Agent 1 branch. Its PR identifies the exact head and files.

- Frozen `d53b4ca8cafbaea5cf11bbb508bb75ced5081585` passed all original Rust checks. The new suites add 8 defensive and 3 local provider tests.
- The local provider confirms that an atomic destination ledger plus read-only original-byte verification can recover a lost response without duplicate records. Keep the live provider in lookup-only mode until its own contract proves those properties.
- Before live wiring, obtain an authoritative scoped release record or guard, authenticate durable destination receipts, bound body/readback sizes, and verify full UnixFS traversal when a CID is required.
- Git-byte manifest verification passed at the frozen head and failed at the newer review base, whose digests match CRLF-normalized content. The Agent 2 manifest covers exact committed bytes.
