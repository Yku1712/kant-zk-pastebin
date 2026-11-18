# Fail-closed security analysis

**Scope:** isolated Rust authority bridge only. **Verdict:** ACCEPT FOR EXPERIMENTAL REVIEW.
**Live Artifact MCP integration:** BLOCKED. **NOT PRODUCTION-READY.**

## Trust boundaries

1. **Credential boundary.** `CredentialVerifier` issues `AuthenticatedPrincipal` from server-held
   credential records. The principal's fields are private; downstream callers cannot forge one.
2. **Authoritative source boundary.** `AuthoritativeSource::snapshot` is the only way the bridge
   obtains revision facts. The real implementation must authenticate, resolve fresh metadata, and
   mint grants through Artifact MCP's sanctioned constructors.
3. **Grant boundary.** `ReadGrant` and `PublicReleaseGrant` are private types. Neither can be
   constructed from `ArtifactRevision`, `ArtifactPublicView`, a visibility flag, a share token, or
   caller-supplied metadata.
4. **Publication boundary.** The bridge implements `zos_int07_core::AuthorizedSource`; the existing
   journal and destination checks remain unchanged.

## Fail-closed checks performed before any body is returned

| Check | Failure |
| --- | --- |
| Snapshot exists for the exact `(source, tenant, artifact, revision)` | `SourceUnavailable` |
| Snapshot binding equals the requested binding, including digest | `InvalidBinding` |
| Principal binding equals the authenticated principal used by the source | `AccessDenied` |
| Snapshot consistency hash matches all included facts | `InvalidBinding` |
| Read grant principal, binding, and epoch match the snapshot | `AccessDenied` |
| Public-release grant is present, distinct, exact-bound, and current | `Ineligible` |
| Classification is explicitly `Public` | `Ineligible` |
| Body kind is `SingleFile` | `UnsupportedBody` |
| Recomputed SHA-256 equals the binding digest | `DigestMismatch` |

`Publisher::execute` reauthorizes on every attempt, including journal replay. A confirmed operation
is not reused after authorization revocation because the source check precedes the journal claim.

## Adversarial coverage

All 12 tests in `tests/adversarial.rs` use a contract-compatible synthetic provider.

| Required case | Test |
| --- | --- |
| 1. Authorized public revision | `authorized_public_revision` |
| 2. Unauthorized principal | `unauthorized_principal` |
| 3. Cross-tenant request | `cross_tenant_request` |
| 4. Stale grant | `stale_grant` |
| 5. Revision substitution | `revision_substitution` |
| 6. Digest mismatch | `digest_mismatch` |
| 7. Public-to-private policy transition | `public_to_private_policy_transition` |
| 8. Missing artifact | `missing_artifact` |
| 9. Inconsistent source snapshot | `inconsistent_source_snapshot` |
| 10. Reuse after revocation | `confirmed_operation_cannot_be_reused_after_revocation` |
| Additional: reader has read but not release | `reader_with_read_access_still_lacks_public_release` |
| Additional: source namespace isolation | `source_namespace_is_part_of_the_grant` |

The synthetic provider tests policy shape only. The real source audit is in
`AUTHORITY_CONTRACT.md`; no test in this crate starts Artifact MCP, Pastebin, IPFS, or a network
service.

## Why live integration is blocked

- Artifact MCP has no production boundary that mints an `AuthorizedArtifact` from a public view or
  serializable snapshot. All grants come from five sanctioned constructors that require an
  authenticated principal and a live meta lookup (`src/security/access.rs:302,441,465,485,521,132`).
- There is no first-class public-release permission. The closest gates are the
  `artifacts:visibility` OAuth scope plus owner/org policy; `hidden` is unlisted, not private
  (`src/security/oauth.rs:28,329`; `src/http/routes/mcp/admission.rs:347-369`;
  `src/persistence/migrations/m011_m024.rs:8-17`).
- A live implementation must add an Artifact MCP endpoint/adapter that performs the existing
  authenticated grant flow and returns a one-shot, exact-revision authoritative snapshot. That
  production change is outside this isolated review.
- The real artifact may be a bundle; this bridge intentionally rejects bundles rather than
  reimplementing Artifact MCP's UTF-16-ordered bundle manifest digest.

## Residual risks and non-claims

- No real credential authentication, OAuth verification, or API-key revocation lookup is performed
  here. `CredentialVerifier` is a test boundary.
- Epoch-based grant freshness models revocation; a production implementation must bind it to the
  real credential/share revocation state.
- A single in-process snapshot cannot prove cross-process SQLite consistency with a live database.
  The production port must do that inside Artifact MCP's authorization and storage path.
- No live MCP/Pastebin/IPFS operation was attempted. No production database, deployment, protected
  branch, private client repository, or private Cargo registry was accessed.
- Apache-2.0 license text is included; dependencies are recorded in `evidence/toolchain.txt`.

## Result

The typed bridge and its adversarial tests pass under Rust 1.95.0. The experimental review is
accepted on that basis. Live integration remains blocked until Artifact MCP exports the authoritative
grant and explicit release boundary described above.
