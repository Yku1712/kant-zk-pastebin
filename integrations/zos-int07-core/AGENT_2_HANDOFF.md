# Agent 2 handoff

**NOT PRODUCTION-READY.** Review branch `review/zos-int07-core-agent1` targets `Yku1712/kant-zk-pastebin:main`. Its base at implementation is `81e42b19bc11aa03d7f637c8d9ca62914d67d0fd`. GitHub's Draft PR head identifies the exact implementation revision. The standalone crate and the reports live together in `integrations/zos-int07-core/`.

- Reproduce from the Draft PR head with the README commands and verify `SHA256SUMS` from a byte-exact `git archive` export. The prior manifest was stale and has been regenerated; see [independent verification](INT07_INDEPENDENT_VERIFICATION.md).
- Bind retrieval to Artifact MCP's authorized grant. Supply a consistent single-file snapshot and independent authorization for public release of that exact tenant, revision and digest.
- Specify durable operation-to-receipt correlation and independent original-byte readback before implementing a live Pastebin adapter. Leave ambiguous operations unresolved until such evidence exists. Enable idempotent resubmission only against a provider that proves it deduplicates a concurrent first call and retries by operation ID.
- Verify complete UnixFS readback through linked blocks. The Pastebin `/ipfs/{cid}` response carries a local block and cannot supply that proof.
- Keep the isolated crate independent of `nora`. Building the actual Pastebin service requires legitimately provided dependencies.
