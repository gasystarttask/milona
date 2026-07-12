# Milona — Implementation Roadmap (Rust)

This roadmap turns the design in [ARCHITECTURE.md](ARCHITECTURE.md) into a concrete, phased
Rust implementation plan. For each architectural block it names the crate(s) recommended,
why, and what risk to carry forward. Recommendations favor maturity and low operational
weight over novelty — swap points are called out explicitly so a component can be replaced
without a rewrite.

## Guiding principles

- **Lightweight first.** Prefer embedded/in-process solutions over standing up extra
  services; only add a dedicated system (Neo4j, Qdrant) when a concrete limit is hit.
- **Swappable by design.** Storage, embeddings, and LLM providers sit behind traits so the
  concrete crate can change without touching callers (mirrors the "ADAPTER" node in the
  diagram).
- **One async runtime.** Everything runs on Tokio; no mixing with async-std/smol.
- **No premature distribution.** A single binary/workspace until a phase's target load
  actually requires splitting services.

## Recommended stack at a glance

| Layer | Crate(s) | Notes |
|---|---|---|
| Async runtime | `tokio` | universal default |
| API/CLI/UI presenter | `axum` + `clap` | axum for HTTP, clap for CLI; same handlers reused by both |
| Storage driver | `mongodb` (official) | async-native since v3, targets MongoDB/Atlas or DocumentDB |
| Vectors | Mongo `$vectorSearch` → `LanceDB` if scale demands | avoid a second system until proven necessary |
| Graph | adjacency-list + `$graphLookup` (Atlas) → `neo4rs` if traversal needs grow | **not viable on DocumentDB**, see risks |
| Web/HTML ingestion | `reqwest` + `scraper` + `dom_smoothie` | fetch, DOM query, main-content extraction |
| PDF ingestion | `pdfium-render` | best text-fidelity; ships without bundling PDFium |
| Chunking | `text-splitter` (+ `tiktoken-rs`) | token-aware, recursive/markdown/code-aware |
| Embeddings | `fastembed-rs` (on `ort`) | local ONNX inference, no per-token cost |
| LLM adapter | `genai` behind a thin Milona trait | multi-provider, native protocols, framework-agnostic |
| Tools / MCP | `rmcp` (official SDK) | server + client, stdio & HTTP transports |
| Serialization | `serde` / `serde_json` | unambiguous standard |
| Config | `config` | actively maintained; avoid `figment` (stale) |
| Errors | `thiserror` + `anyhow` (+ `miette` at the CLI boundary) | library errors vs. app errors vs. pretty CLI diagnostics |
| Observability | `tracing` + `tracing-subscriber` (+ `tracing-opentelemetry`) | OTel traces still Beta upstream — expect churn |
| Secrets | env vars via `config`, backed by a secrets manager (Vault / AWS Secrets Manager / GCP Secret Manager) | never plaintext `.env` in production |
| AuthN/AuthZ | `tower` middleware (API keys or OAuth2/OIDC via `oauth2` crate) + a policy layer (Casbin via `casbin-rs`, or hand-rolled RBAC) | gates every axum route from Phase 0 |
| Supply-chain scanning | `cargo audit` + `cargo deny` in CI | RUSTSEC advisories fail the build, not just a manual check |
| Rate limiting / cost control | `tower::limit` / `governor` crate for request throttling; per-tenant token budgets enforced in `milona-adapter` | protects against runaway LLM spend |

## Enterprise requirements this roadmap now designs for

The stack table above optimizes for *lightweight and performant*; the items below are the
non-negotiable additions an enterprise deployment requires. They are pulled forward into
**Phase 0.5**, before any ingestion/storage/application code is written, because retrofitting
tenant isolation and auth after Phase 2/4 land is materially more expensive than designing
for it from the start.

## Key risks to design around from day one

1. **DocumentDB vs. real MongoDB/Atlas is a fork in the road.** DocumentDB does not support
   `$graphLookup`. If DocumentDB is a target deployment, the graph layer cannot rely on it —
   plan the knowledge-graph module behind a trait from the start so it can be backed by
   client-side multi-query traversal or by an external graph DB.
2. **No embedded Rust-native graph DB is currently safe for production** (CozoDB stalled,
   IndraDB alpha, KuzuDB abandoned). The fallback is `neo4rs` against a real Neo4j instance,
   not a pure-Rust embedded engine.
3. **Qdrant's embedded mode ("Edge") is private beta**, not GA — don't architect around it.
   LanceDB is the more production-ready embedded, Rust-native vector store if/when Mongo's
   native vector search stops being enough.
4. **OpenTelemetry's Rust tracing API/SDK is still Beta** (Logs/Metrics are stable). Fine to
   adopt now, expect breaking changes before 1.0.
5. **`rmcp` has a known HIGH-severity advisory: RUSTSEC-2026-0189** (DNS-rebinding flaw in
   the Streamable HTTP server transport), fixed in ≥1.4.0. Pin `rmcp` to a patched version
   before Phase 5, and add `cargo audit`/`cargo deny` to CI in Phase 0 so future advisories
   like this fail the build automatically instead of relying on someone noticing.
6. **Untrusted ingested content is an LLM attack surface.** Web pages and PDFs pulled in by
   Phase 1 can carry prompt-injection payloads. There must be an explicit trust boundary
   between "ingested content" (data) and "system instructions" (control) in the Phase 3
   application loop — this is a design requirement, not a hardening afterthought.
7. **The Mongo adjacency-list graph model is not a thin abstraction over Neo4j.** The
   `GraphStore` trait hides the *interface* migration but not the underlying one: adjacency
   list documents → labeled property graph is a full data re-model, and `$graphLookup`
   recursive lookup has different depth/cycle/perf semantics than Cypher pattern matching.
   Budget Phase 6's Neo4j fallback as a migration project, not a config flip.

---

## Phase 0 — Project scaffolding

- Cargo workspace with crates per architectural block: `milona-core` (domain types, traits),
  `milona-ingest`, `milona-storage`, `milona-knowledge`, `milona-tools`, `milona-adapter`,
  `milona-presenter` (axum + clap binary).
- Wire up `serde`, `thiserror`/`anyhow`, `tracing`, `config` and a `.env`/config-file loading
  convention. CI: `cargo fmt`, `cargo clippy -D warnings`, `cargo test`.
- Define the core traits early since everything else implements against them:
  `DocumentSource`, `Chunker`, `Embedder`, `VectorStore`, `GraphStore`, `LlmProvider`, `Tool`.
- Add `cargo audit` and `cargo deny` (license + advisory checks) to CI alongside
  fmt/clippy/test, so supply-chain risk is caught automatically from the first commit —
  this would have caught `rmcp`'s RUSTSEC-2026-0189 rather than requiring manual discovery.

## Phase 0.5 — Enterprise foundations

These are cross-cutting requirements that touch every later phase, so they're designed now
rather than bolted on after Phase 2 (storage) and Phase 4 (presenter) have already shipped
schemas and API surfaces that assume a single tenant and no auth.

**Identity & access**
- Define a tenant model (`tenant_id`) as a first-class field from the very first schema and
  the very first trait signature — `DocumentSource`, `VectorStore`, and `GraphStore` methods
  all take a `tenant_id`/`TenantContext`, not just a document ID. Retrofitting this later
  means touching every query in Phase 2 and Phase 3.
- AuthN on the axum API: API keys for service-to-service, OAuth2/OIDC (`oauth2` crate) for
  human users. No unauthenticated route except a liveness health check.
- AuthZ: a policy layer (RBAC via `casbin-rs`, or a hand-rolled permission check) gating
  which tenant/role can query which knowledge scope — enforced in `milona-knowledge`, not
  just at the HTTP edge, so internal callers (tools, MCP) can't bypass it.

**Secrets & transport security**
- All credentials (LLM provider API keys, Mongo/Neo4j connection strings) come from a
  secrets manager (Vault, AWS/GCP Secrets Manager) or KMS-encrypted env injection — never
  committed config, never plaintext at rest. `config` crate loads the *reference*, not the
  secret value, in non-local environments.
- TLS enforced on every network hop: Mongo (`tls=true`), Neo4j (`bolt+s://`), and outbound
  LLM provider calls (default for `reqwest`/`genai`, verify no insecure fallback is enabled).

**Tenant data isolation**
- Vector search: every `$vectorSearch` query includes a `tenant_id` filter in the same
  aggregation stage — not a post-filter — so a query literally cannot retrieve another
  tenant's vectors. Same discipline applies if/when LanceDB is adopted in Phase 6 (separate
  table/dataset per tenant, or a mandatory partition column).
- Graph traversal: every adjacency-list edge document carries `tenant_id`, and `$graphLookup`
  stages include it in `restrictSearchWithMatch` — untested tenant-scoping here is the
  single most likely silent cross-tenant leak in this architecture.
- Revisit at Phase 6: per-tenant encryption keys and noisy-neighbor throttling if a tenant's
  ingestion or query volume can starve others.

**Prompt-injection & content trust boundary**
- Ingested content (Phase 1: web pages, PDFs) is data, never control. The Phase 3 GenAI loop
  must keep system instructions and retrieved/ingested content in clearly separated message
  roles, and should treat any instruction-like text found inside ingested content as
  untrusted — do not let it alter tool-use permissions or system behavior.
- Basic sanitization pass on ingested text (strip obvious instruction-injection patterns,
  cap content length fed into context) before it reaches embedding/persistence.

**Cost control & resilience for LLM calls**
- Wrap every `LlmProvider` call (via `genai`) with retry/backoff and a circuit breaker
  (`tower::retry` + a breaker crate, or hand-rolled) so a provider outage degrades gracefully
  instead of cascading.
- Per-tenant rate limiting and token/cost budgets enforced in `milona-adapter`, using
  `governor` or `tower::limit` at the API layer — a single runaway tenant or tool-use loop
  must not be able to exhaust the shared LLM budget.

**Compliance & governance**
- Audit log (distinct from debug tracing): who queried what, when, and which documents/tools
  were touched — append-only, tenant-scoped, queryable for compliance requests.
- Data residency: document which region each tenant's Mongo/Atlas cluster and LLM provider
  calls land in, since the provider list (OpenAI/Anthropic/Gemini/DeepSeek/Qwen) spans
  US/EU/China jurisdictions — this is a per-deployment config decision, not a code change,
  but the config surface must exist.
- PII handling: a redaction/detection pass on ingested documents before persistence if the
  corpus may contain personal data, plus a retention/deletion workflow that can purge a
  document's chunks, embeddings, and graph edges together (right-to-be-forgotten).

**High availability & disaster recovery**
- Run Mongo/Atlas as a replica set (minimum 3 nodes); confirm the `mongodb` driver's
  retryable-reads/retryable-writes are enabled and understand its server-selection timeout
  behavior during primary stepdown/failover.
- Define backup cadence and RPO/RTO targets for Mongo and (if Phase 6 adds it) Neo4j —
  Neo4j clustering for HA is an Enterprise-licensed feature, factor that into the Phase 6
  cost/benefit before committing to it.

## Phase 1 — Ingestion pipeline

- `DocumentSource` implementations: local file (TXT), PDF (`pdfium-render`), web
  (`reqwest` + `scraper` + `dom_smoothie` for main-content extraction).
- Normalize to a common `RawDocument { text, metadata }`.
- Chunk with `text-splitter`, token-aware via `tiktoken-rs`, markdown-aware for structured
  sources. Default to ~400-512 token chunks with 10-20% overlap as a starting point —
  fixed-size/recursive chunking is the safer default; only reach for semantic chunking on
  narrow, heterogeneous document sets.
- Embed with `fastembed-rs` behind the `Embedder` trait, so a hosted embedding API can be
  swapped in later without touching pipeline code.
- Persist chunks + embeddings + extracted graph edges through the `VectorStore`/`GraphStore`
  traits (implemented in Phase 2).

## Phase 2 — Storage layer

- Implement `VectorStore` over the `mongodb` driver using `$vectorSearch` (Atlas or
  DocumentDB) as the default backend — one less system to operate. Every query stage
  includes the `tenant_id` filter defined in Phase 0.5; add a test that asserts a
  cross-tenant query returns zero results.
- Implement `GraphStore` as an adjacency-list model over `mongodb`, using `$graphLookup` for
  traversal **only when targeting real MongoDB/Atlas**. Gate this behind a capability check
  so a DocumentDB deployment fails fast with a clear error instead of silently degrading.
  Every edge document and every `$graphLookup` stage carries `tenant_id` per Phase 0.5.
- Run Mongo/Atlas as a replica set from the first deployment (not just production) so
  failover/retryable-write behavior is exercised in staging, not discovered in an incident.
- Add integration tests against both a local MongoDB container and (if in scope) a DocumentDB
  emulation, to catch operator-support gaps early.
- Leave a clean seam to swap `VectorStore` to LanceDB and `GraphStore` to `neo4rs` — these
  become Phase 5+ work only if scale or feature needs justify it. Budget the Neo4j swap as a
  data re-modeling project (see Key Risk #7), not a config flip.

## Phase 3 — GenAI application layer

- `milona-knowledge`: query façade combining vector similarity search and graph traversal
  into a single retrieval API consumed by the GenAI application logic.
- `milona-adapter`: wrap `genai` behind a Milona `LlmProvider` trait so provider selection
  (OpenAI, Anthropic, Gemini, DeepSeek, Qwen, ...) is a config value, not a code change.
- Core GenAI application loop: question → retrieval (knowledge) → tool use (if needed) →
  generation → response, instrumented with `tracing` end to end.

## Phase 4 — Presenter (API / CLI / UI)

- `axum` HTTP API exposing question/response endpoints, health checks, and streaming
  responses (SSE) for chat-style interactions. Every non-health route sits behind the
  authN/authZ middleware from Phase 0.5 — no route ships unauthenticated by default.
- Rate limiting (`governor`/`tower::limit`) applied at the API layer per Phase 0.5, so a
  single API key/tenant cannot exhaust shared capacity.
- `clap`-based CLI reusing the same core application handlers (thin presenter, shared logic).
- `miette` for readable CLI-side error diagnostics.
- UI is out of scope for this roadmap's Rust backend work; the API is designed to serve any
  frontend.

## Phase 5 — Tools & MCP

- `milona-tools`: `Tool` trait for internal tools; register both native Rust tools and
  MCP-discovered tools uniformly.
- Integrate `rmcp` to expose Milona's own capabilities as an MCP server and to consume
  external MCP servers as a client, matching the "MCP / CLI Area" block in the architecture
  diagram.

## Phase 6 — Hardening & scale-out (only as needed)

- Swap `VectorStore` to LanceDB if Mongo-native vector search hits latency/scale ceilings.
- Stand up Neo4j (`neo4rs`) for the graph layer if `$graphLookup` limitations, DocumentDB
  constraints, or traversal-depth needs make the Mongo-native graph model insufficient.
- Add `tracing-opentelemetry` export to a collector once the team is ready to absorb Beta-API
  churn; add `metrics` for dashboards/alerting, with per-tenant token/cost usage as a named
  metric (not just an aside) since Phase 0.5 introduces per-tenant budgets.
- Define SLOs/error budgets for the API and set up alerting against them; add the audit log
  (Phase 0.5) as a queryable store distinct from debug tracing.
- Add chaos/failure-injection testing (kill Mongo primary mid-write, simulate LLM provider
  timeouts/5xx) and a canary/blue-green deploy strategy for the axum service.
- Define a schema-migration strategy for Mongo documents (versioned documents + backfill
  jobs) before the adjacency-list graph schema or chunk/embedding schema needs to change.
- Load-test ingestion and query paths; revisit chunking parameters and embedding batch sizes
  based on real corpus characteristics.

## Explicitly deferred / not recommended now

- **Embedded Rust-native graph DB** (Cozo, IndraDB, Kuzu) — none currently safe for
  production; revisit only if the ecosystem matures.
- **Qdrant Edge (embedded)** — private beta, not GA.
- **`candle` for embeddings** — only if a bleeding-edge HF checkpoint isn't available via
  ONNX; `fastembed-rs`/`ort` is faster and lower-maintenance for standard embedding models.
- **Hand-rolled multi-provider LLM client** — `genai` covers this at lower risk than a
  from-scratch implementation.

## Licensing note

All recommended crates (`mongodb`, `neo4rs`, `dom_smoothie`, `fastembed-rs`, `genai`, `rmcp`,
`text-splitter`, `pdfium-render`, `ort`, `tiktoken-rs`) are MIT/Apache-2.0 — no copyleft
obligations. Two things still need a one-time compliance sign-off before shipping a binary:
- The **PDFium binary** (BSD-3-Clause, via `pdfium-render`) bundles third-party libraries
  (FreeType, lcms2, libjpeg-turbo, OpenJPEG, ICU, zlib/libpng) each requiring attribution/
  notice retention on redistribution — produce a NOTICE file/SBOM entry for it.
- Any embedding model pulled at runtime by `fastembed-rs` (from Hugging Face) carries its own
  model license independent of the crate's Apache-2.0 license — check per model selected.
