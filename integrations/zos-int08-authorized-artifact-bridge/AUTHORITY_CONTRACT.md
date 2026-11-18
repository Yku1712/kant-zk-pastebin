# Artifact MCP authority contract (source audit)

**Audited source:** `artifact-mcp` at `731d5b6e3436cc4bdee961f8510eda6ea2a0d9eb`.
The checkout was verified with `git rev-parse HEAD` and `git status --short` before reading.
All references below are to that exact source. No production service was contacted.

This document is the static source audit. The Rust tests use a clearly labeled contract-compatible
synthetic provider; they do **not** run Artifact MCP.

## 1. Authenticated principal

| Fact | Exact reference |
| --- | --- |
| `PublisherIdentity { client_id, org, label, role, scopes }`; `scopes: None` means legacy API key, `Some` means OAuth and never inherits unrestricted API-key access | `src/model/identity.rs:101-111` |
| `is_admin()` is derived only from `org == "admin"`; there is no stored admin flag | `src/model/identity.rs:119-122`; `src/security/access.rs:350-352` |
| `is_oauth()` and exact scope check | `src/model/identity.rs:125-137` |
| API-key authenticator: bearer key → active key row → identity | `src/security/auth.rs:312-327` |
| Identity copied from the DB row; `scopes: None` | `src/security/auth.rs:335-352` |
| Active key query requires `revoked_at IS NULL` | `src/security/auth.rs:257-267`; `src/adapters/directories.rs:47-49` |
| OAuth verifies JWT and maps claims to identity with `scopes: Some(...)` | `src/security/oauth.rs:116-235` |
| Scope constants include `artifacts:read`, `artifacts:publish`, `artifacts:visibility` | `src/security/oauth.rs:24-42` |
| API key first, then OAuth; storage failures abort | `src/security/oauth.rs:268-287` |
| Human viewer resolved from Cloudflare Access/JWT/header trust; failures become unsigned `Viewer::default()` | `src/security/identity.rs:613-647` |
| Viewer admin derived from configured admin emails/domains | `src/security/identity.rs:593-607,271-274` |

**Bridge consequence:** the principal must be server-derived. The bridge accepts only
`AuthenticatedPrincipal`, whose fields are private and whose only public issuer in this crate is
`CredentialVerifier::authenticate`.

## 2. AccessPolicy grant issuance and validation

| Fact | Exact reference |
| --- | --- |
| `AuthorizedArtifact(ArtifactMeta)` and `OwnedArtifact(ArtifactMeta)` have private tuple fields | `src/security/access.rs:105-135` |
| Only sanctioned constructors: `authorize_viewer`, `authorize_publisher_read/write/delete`, `authorize_share`, `into_authorized`; compile-fail doctests reject forgery | `src/security/access.rs:22-39,64-96` |
| Missing artifact is concealed first; unsigned and foreign viewers collapse to `ConcealedNotFound` | `src/security/access.rs:268-312` |
| `resolve_for_viewer` does `find_meta` then `authorize_viewer`; subordinate reads happen only afterwards | `src/security/access.rs:547-557` |
| Publisher read/write/delete grants return `OwnedArtifact`; missing and foreign both become `NotFound("Unknown artifact: {id}")` | `src/security/access.rs:441-501` |
| Share grant is issued only when `grant.org == artifact.org` | `src/security/access.rs:503-531` |
| Invalid, expired, revoked, and stale shares are deliberately indistinguishable | `src/persistence/shares.rs:5-20` |
| Share SQL joins on `(artifact_id, org)`, requires `revoked_at IS NULL` and unexpired time | `src/persistence/shares.rs:65-68` |
| Re-tenancy and revocation delete/mark shares | `src/persistence/shares.rs:77-82` |
| Public-share route resolves token, finds meta, then authorizes share | `src/http/routes/public_share.rs:29-47` |

**Bridge consequence:** ordinary read access is not publication authority. Grants are minted only
on the trusted source side; the bridge validates principal binding, exact revision binding, epoch,
revocation, and snapshot consistency before exposing bytes.

## 3. ArtifactPublicView transformation

| Fact | Exact reference |
| --- | --- |
| The type deliberately does not implement `Serialize`; it is "a snapshot, not an authorization witness" | `src/model/artifact_public_view.rs:1-4` |
| `WITHHELD_META_FIELDS = ["client_id", "owner_email", "body_sha256"]` | `src/model/artifact_public_view.rs:45-53` |
| `From<ArtifactMeta>` exhaustively destructures and discards `client_id` and `owner_email`; renames `body_sha256` to `content_digest` | `src/model/artifact_public_view.rs:62-104` |
| Revision projection is a separate, broader contract | `src/model/artifact_public_view.rs:106-149` |

**Bridge consequence:** a public view, serialized snapshot, or echoed metadata is never accepted
as a grant. Raw `ArtifactMeta` is not supplied by the publication caller.

## 4. Authoritative exact-revision retrieval

| Fact | Exact reference |
| --- | --- |
| `find_meta(id)` is the sole bare-id read; body/history/bundle reads require a grant | `src/ports/artifacts.rs:19-23` |
| `read_body`, `read_bundle_file`, `read_revision_body`, `list_bundle_files`, `list_revisions` take `&AuthorizedArtifact` | `src/ports/artifacts.rs:64-111` |
| Read layer explicitly does not authorize; caller must hold the wrapper | `src/artifacts/read.rs:1-14` |
| Exact historical revision reads parse a pinned revision and return concealed-not-found on miss | `src/http/routes/raw.rs:153-186,210-230` |
| Historical revision SQL pins `artifact_id` and `revision` and excludes the live row | `src/artifacts/lifecycle/sql.rs:268-285` |
| Retained revision list reloads the live row and orders by descending revision | `src/artifacts/lifecycle/metadata.rs:97-111` |
| Retention limit and physical history layout | `src/config/limits.rs:18-22`; `src/artifacts/paths.rs:6-13` |
| Revision table has `PRIMARY KEY (artifact_id, revision)` and composite org FK with cascade | `src/persistence/migrations/m001_m010.rs:230-242` |
| Cast-pin requires `revision.body_sha256` to match current metadata and recomputed SHA-256, and rejects bundles | `src/http/routes/raw.rs:57-81` |
| Cast IDs are config-pinned `(id, revision)` pairs | `src/config/cast.rs:9-39`; `src/config/mod.rs:123-124,248` |

**Bridge consequence:** the source port must return one internally consistent snapshot for the
exact requested source, tenant, artifact, revision, and digest. `ArtifactMcpBridge` recomputes the
single-file digest and rejects any binding substitution. Bundle bodies are unsupported.

## 5. Public-release permission

There is **no first-class public-release grant** in the audited source.

| Nearest production gate | Exact reference |
| --- | --- |
| OAuth scope `artifacts:visibility` | `src/security/oauth.rs:28,329` |
| MCP admission rejects when the required scope is absent | `src/http/routes/mcp/admission.rs:347-369` |
| `set_visibility` still calls `AccessPolicy::authorize_publisher_*` | `src/mcp/tools/artifacts.rs:368` |
| Share creation still calls `publisher_delete` then obtains an authorized artifact | `src/mcp/tools/shares.rs:23` |
| `hidden` means unlisted, not private | `src/persistence/migrations/m011_m024.rs:8-17` |

**Bridge consequence:** release permission is modeled as a separate typed capability,
`PublicReleaseGrant`, requiring both a valid read grant and an explicit `Classification::Public`
source fact. This is a conservative interface requirement; it is not a claim that Artifact MCP
already exports such a capability. Live integration is therefore blocked until a production
boundary supplies it.

## 6. Canonical body digest

| Fact | Exact reference |
| --- | --- |
| Frozen digest algorithm; digest addresses previews and is the recovery commit marker | `src/artifacts/digest.rs:1-17` |
| `sha256_hex` produces lowercase hex SHA-256 | `src/artifacts/digest.rs:26-40` |
| `single_body_digest(content) = sha256_hex(content)` | `src/artifacts/digest.rs:42-46` |
| Bundle manifest digest sorts paths by UTF-16 code-unit order and hashes compact JSON | `src/artifacts/digest.rs:48-88` |
| `body_digest_at_path` and bundle entry walk | `src/artifacts/digest.rs:90-133` |
| Validation computes single-body digest after blank/size checks | `src/artifacts/validation.rs:265-291` |
| Publish and update recompute the digest | `src/artifacts/lifecycle/publish.rs:104-117`; `src/artifacts/lifecycle/update.rs:312-318` |

**Bridge consequence:** single-file bodies use `ContentDigest::of`, which is SHA-256 over the raw
body bytes. Bundles are rejected rather than risking manifest-order divergence.

## 7. Tenant and organization isolation

| Fact | Exact reference |
| --- | --- |
| API keys carry `org`; artifacts carry `org`; unique `(id, org)` | `src/persistence/migrations/m001_m010.rs:19,27,121` |
| Revisions and shares use composite `(artifact_id, org)` FKs with cascade | `src/persistence/migrations/m001_m010.rs:230-242`; `src/persistence/migrations/m011_m024.rs:64-72` |
| Artifact keys are unique per org | `src/persistence/migrations/040-artifact-keys.sql:3-5` |
| Ownership requires both `client_id` and org; read/write/delete require same-org or admin | `src/security/access.rs:373-379,396-432` |
| Share grant requires org match | `src/security/access.rs:508-510` |
| Active-key org comes from the database row, never the request | `src/adapters/directories.rs:47-58` |
| Non-admin requested `org` is discarded; explicit org target requires admin | `src/security/auth.rs:372-400` |

**Bridge consequence:** tenant is part of both the grant binding and the exact revision binding.
A cross-tenant request fails before any body is exposed.

## 8. Missing production boundary

Every production grant originates from a small set of sanctioned constructors:
`src/security/access.rs:302,441,465,485,521,132`. Their inputs are an authenticated
`Viewer`/`PublisherIdentity`, a live `find_meta` result, or a resolved `ShareGrant`. No production
function accepts `ArtifactPublicView`, serialized metadata, or an imported external grant and
returns an `AuthorizedArtifact`. `ArtifactPublicView` is explicitly not an authorization witness.

**Conclusion:** a real bridge cannot be given authority merely by receiving a public artifact view.
It must re-enter through the authenticated route/admission path, resolve fresh metadata, and use
one of the sanctioned constructors. This crate therefore stops at a typed interface:
`AuthoritativeSource` is the required production port, and `ContractCompatibleProvider` is only a
test double. No live integration is claimed.
