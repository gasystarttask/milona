# Contributing to Milona

This document is the local mirror of what `.github/workflows/ci.yml` runs. If you can get
a clean run through every step below, CI should pass (module network-dependent advisory
database freshness, which can't be perfectly reproduced offline — see the note at the end).

## Prerequisites

- Rust via `rustup`, matching `rust-version` in the root `Cargo.toml` (currently 1.87) or
  newer. `rustfmt` and `clippy` components installed (`rustup component add rustfmt clippy`).
- Docker + Docker Compose, for the local MongoDB (and optional Neo4j) integration-test
  infrastructure in `docker-compose.yml`.
- `cargo-audit` and `cargo-deny` (see [Supply-chain scanning](#supply-chain-scanning) below
  for exact install commands and known version-compatibility notes).

## The full local validation loop

Run these in order; each corresponds 1:1 to a CI job/step in `.github/workflows/ci.yml`.

```bash
# 1. Formatting
cargo fmt --all -- --check

# 2. Lints (warnings are build failures, matching CI's -D warnings)
cargo clippy --workspace --all-targets -- -D warnings

# 3. Unit + integration tests
#    Most crates' tests use in-memory fakes and need no external services. A few
#    Phase 2 storage integration tests are Docker-gated (see below) or #[ignore]-gated.
cargo test --workspace

# 4. Supply-chain: advisories
cargo audit

# 5. Supply-chain: licenses, bans, sources (config in ./deny.toml)
cargo deny check
```

If all five pass locally, open a PR with confidence (see
[Branch and tag protection](#branch-and-tag-protection) below — direct pushes to `main` are
rejected, so this is required, not optional).

## Local integration-test infrastructure (`docker-compose.yml`)

Some Phase 2 storage tests need a real MongoDB **replica set** (not just a standalone
instance) because `$vectorSearch`, `$graphLookup`, and change streams all require one. Start
it with:

```bash
docker compose up -d mongodb
# wait a few seconds for the replica set to reach PRIMARY, then:
export MONGODB_URI="mongodb://localhost:27017/?directConnection=true&replicaSet=rs0"
cargo test -p milona-storage --test mongo_integration -- --ignored --test-threads=1
```

One of these (`vector_search_is_tenant_scoped_against_atlas`) stays `#[ignore]`d even with
the replica set running: self-hosted MongoDB doesn't support `$vectorSearch` at all — it
needs a real Atlas cluster with a `$vectorSearch` index named `milona_vector_index`. The
other three only need the local replica set above.

Tear down with `docker compose down -v` (the `-v` also drops the Mongo data volume, so you
get a clean replica set next time).

The optional Neo4j service (Phase 6 graph-layer fallback, see `docker-compose.yml`'s
comments and ROADMAP.md Key Risk #7) is not started by default:

```bash
docker compose --profile neo4j up -d neo4j
```

## Supply-chain scanning

### Installing `cargo-audit` / `cargo-deny`

```bash
cargo install cargo-audit --locked
cargo install cargo-deny --locked
```

**Version-compatibility note observed in this sandbox:** the latest `cargo-audit`
(0.22.x) and `cargo-deny` (0.20.x) require rustc ≥ 1.88. If your toolchain is pinned at the
workspace's `rust-version = "1.87"`, `cargo install` will suggest an older compatible
version (e.g. `cargo-audit 0.21.2`, `cargo-deny 0.18.3`) — that's fine for day-to-day use,
but see the next note. CI always uses a current `stable` toolchain (via
`dtolnay/rust-toolchain@stable`), so it runs the latest audit/deny without this constraint;
install a matching toolchain locally (`rustup toolchain install stable`) if you want your
local run to exactly match CI, e.g.:

```bash
rustup toolchain install 1.88.0 --profile minimal
cargo +1.88.0 install cargo-audit --locked --force
```

**Advisory-database/CVSS-parser skew:** older `cargo-deny` builds (whose bundled
`rustsec`/`cvss` crate predates CVSS 4.0 support) will fail to *load* the advisory database
once it contains any CVSS 4.0-scored advisory, with an error like:

```
unsupported CVSS version: 4.0
```

This is a version-skew bug in the audit tool itself, not a finding about Milona's
dependencies — confirmed by the fact that a `cargo-audit` build against a newer toolchain
scans the identical `Cargo.lock` cleanly. If you hit this locally, update `cargo-deny`
(requires the newer rustc noted above) rather than trying to work around it in
`deny.toml`.

### Two separate config files — keep them in sync

`cargo audit` (and the `rustsec/audit-check` GitHub Action CI uses, which shells out to it)
reads **`.cargo/audit.toml`** only. `cargo deny check` reads **`deny.toml`** only — it does
not read `.cargo/audit.toml`. Any advisory-ignore decision must be added to *both* files
with matching ids and reasoning, or one tool will still fail even after the other passes.

### What's currently ignored, and why

Two real vulnerabilities and four warnings are ignored in both files (each dated for review
by **2026-10-01**):

| ID | Crate | Why it's ignored |
|---|---|---|
| `RUSTSEC-2026-0187` | `lopdf` (via `pdf-extract`, `milona-ingest`) | Fix (`lopdf >= 0.42`) transitively requires `rustc >= 1.88`; workspace pins 1.87 |
| `RUSTSEC-2026-0009` | `time` (pinned `=0.3.36` in `milona-storage`, itself required by `mongodb`'s MSRV) | Fix (`time >= 0.3.47`) requires `rustc >= 1.88` |
| `RUSTSEC-2026-0174` | `http-types` (via `wiremock`) | Dev-dependency only (HTTP mocking in `milona-ingest`'s tests), never shipped |
| `RUSTSEC-2025-0057` | `fxhash` (via `wiremock`) | Dev-dependency only, never shipped |
| `RUSTSEC-2024-0384` | `instant` (via `wiremock`) | Dev-dependency only, never shipped |
| `RUSTSEC-2026-0097` | `rand 0.7.3` (via `wiremock`) | Dev-dependency only, never shipped |

The two real vulnerabilities are the same MSRV wall as `genai`/`fastembed-rs`/`rmcp` — see
[README.md's Known limitations](README.md#known-limitations). Raising the workspace's
`rust-version` past 1.87 unblocks all of them together; re-run `cargo update` on the
affected crates and delete the corresponding ignores from both files at that point.

### What `deny.toml` enforces

- **Advisories**: any RUSTSEC advisory (vulnerability/unmaintained/unsound/notice) fails
  `cargo deny check` unless explicitly (and temporarily, with a review-by date noted in the
  `reason` string — this `cargo-deny` version's schema has no separate `expires` key) ignored.
- **Licenses**: an allow-list of permissive licenses (MIT, Apache-2.0, BSD, ISC,
  Unicode-3.0/DFS-2016, Zlib, CC0-1.0, MIT-0, CDLA-Permissive-2.0, MPL-2.0) per ROADMAP.md's
  licensing note — copyleft with a *distribution/network-service* trigger (GPL/AGPL/LGPL
  family) is intentionally excluded, so any such dependency (direct or transitive) fails the
  build and forces an explicit compliance review. MPL-2.0 was reviewed and added
  2026-07-13: it's file-level copyleft that only affects modifications to the MPL-licensed
  files themselves, not code that merely uses the library (`scraper`'s CSS-parsing deps, used
  unmodified in `milona-ingest`). CDLA-Permissive-2.0 is a genuinely permissive data license
  used by `webpki-root-certs` (transitive via `reqwest`'s TLS stack), just outside the
  original MIT/Apache/BSD-family list. If a *new* copyleft dependency shows up, don't assume
  it's the same case — review it on its own terms before adding it here.
- **Bans**: flags duplicate major versions of the same crate (bloat/audit-surface) as
  warnings, and denies a couple of explicitly unwanted crates (e.g. `openssl-sys`, since the
  workspace's recommended stack standardizes on `rustls`).
- **Sources**: only crates.io is an allowed registry source; unknown git dependencies are
  denied by default.

## Presenter OTel tracing seam

`crates/milona-presenter/src/otel.rs` is a feature-flagged (`--features otel`) seam for
`tracing-opentelemetry` export, left intentionally inert (no collector wiring) per
ROADMAP.md's note that the Rust OpenTelemetry tracing API/SDK is still Beta upstream. See
the `TODO(otel)` comments in that file for what a real integration requires. Default builds
(`cargo build`, no extra flags) are unaffected — the feature adds zero dependencies unless
explicitly enabled.

## Branch and tag protection

`main` and all tags (`refs/tags/*`) are protected by repository rulesets (GitHub → repo →
Rules), enforced for everyone including admins — there is no bypass:

- **`main`**: no force-pushes, no deletion. Merging requires a pull request with at least
  one approval (stale approvals are dismissed on new pushes) and all three CI jobs green and
  up to date with `main`: `fmt + clippy`, `test (workspace + MongoDB replica set)`,
  `cargo audit + cargo deny`.
- **Tags**: no deletion, no force-updates of any tag once created.

In practice: always work on a branch and open a PR — a direct `git push origin main` will
be rejected.

## Scope notes for this workspace

- Every storage/knowledge/tool call must carry a `TenantContext`; every query filters by
  `tenant_id` at the query/aggregation level (never as a post-filter).
- Ingested content is untrusted data (`Chunk::trust_label`), never treated as instructions.
- One async runtime (`tokio`) throughout; don't introduce `async-std`/`smol`.
- Only use workspace-level dependency versions declared in the root `Cargo.toml`
  `[workspace.dependencies]` where present; otherwise pin a version directly in the crate's
  own `Cargo.toml`.
