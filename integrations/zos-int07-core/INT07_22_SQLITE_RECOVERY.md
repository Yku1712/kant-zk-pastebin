# INT07-22 — SQLite recovery

**PASS for the standalone implementation and local process-crash tests. NOT PRODUCTION-READY.**

The journal uses a dedicated SQLite file with rollback journal mode `DELETE`, synchronous mode `FULL`, a five-second busy timeout and `BEGIN IMMEDIATE` for every write transition. It rejects memory mode, unknown app/schema versions, and nonempty unrelated databases. The database stores operation bindings, state, and confirmed remote identity. Artifact bodies, credentials, and unverified receipt data stay out of the journal.

| Persisted state | Meaning | Replay |
| --- | --- | --- |
| `PREPARED` | Intent exists. No caller has committed dispatch authority. | Reauthorize and rehash, then compete for the first dispatch. |
| `UNRESOLVED` | A caller committed dispatch authority. It may have stopped before sending, during submission, after a remote write, or during verification. | Reauthorize and perform read-only lookup. Confirm only with verified receipt evidence. A missing lookup leaves the operation unresolved. An explicitly selected provider may retry the same operation only under a durable atomic idempotency contract. |
| `CONFIRMED` | Verified receipt was committed locally. | Reauthorize and return the persisted receipt. Repeating the same confirmation is safe. A different receipt produces a journal conflict. |

The operation ID is SHA-256 of the fixed domain `zos-int07/public-single-file/v1\0`, followed by length-prefixed UTF-8 source installation, tenant, artifact and destination IDs, then the big-endian eight-byte revision and 32 digest bytes. Each length is an unsigned eight-byte big-endian value. Unique revision/destination scope also rejects contradictory digests supplied for one source revision. `pending()` reconstructs persisted intents across restarts. A caller still needs authorized access to the exact revision before replay.

At the dispatch boundary, a failed or interrupted local commit yields no authority to send. A fresh connection checks the durable state. An interrupted confirmation is resolved the same way. The first dispatcher does its external work after SQLite releases the transaction. Other connections can observe and reconcile it concurrently. The destination's idempotent retry option must cover overlapping original and retry calls. Local locks alone do not impose exactly-once behavior on HTTP or IPFS.

## Process-termination evidence

The crash test starts a fresh subprocess for each case, waits for a marker immediately before or after the indicated commit, kills the process and opens the database again. Before-commit cases execute the corresponding SQL transition inside an uncommitted transaction. After-commit cases call the actual journal methods.

| Transition | Terminate before commit | Terminate after commit |
| --- | --- | --- |
| Prepare | No row | PREPARED |
| Claim for dispatch | PREPARED | UNRESOLVED |
| Confirm receipt | UNRESOLVED | CONFIRMED |

Integration tests use a real bundled SQLite engine and synthetic external providers. They cover concurrent connections while a submission is blocked, an accepted submission with a lost response, a failure with no observable receipt, and open failures returned through Rust errors. This establishes application-process crash recovery under SQLite's storage assumptions. It does not simulate filesystem failure, power loss, or live Pastebin/IPFS durability.

For current Pastebin, an ambiguous submission must stay unresolved unless a trusted operator or future adapter obtains independent operation-correlated evidence. There is no proof of absence in an alias miss, 404, or timeout. The crate does not include automatic tombstoning or receipt mutation.
