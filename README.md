# Milona

[![CI](https://github.com/gasystarttask/milona/actions/workflows/ci.yml/badge.svg)](https://github.com/gasystarttask/milona/actions/workflows/ci.yml)

A model-agnostic GenAI application: ingest documents, store them as both a knowledge graph
and vector embeddings, and query them through a tenant-isolated API/CLI backed by a
swappable LLM adapter. See [ARCHITECTURE.md](ARCHITECTURE.md) for the design and
[ROADMAP.md](ROADMAP.md) for the phased implementation plan this codebase follows.

## Status

All seven roadmap phases (0 through 6) are implemented: workspace scaffolding, tenant/authz
foundations, ingestion, storage, the GenAI application loop, the presenter (API/CLI), tools,
and CI hardening. Three components are currently **mocked/stubbed**, all for the same
reason — see [Known limitations](#known-limitations) before you rely on them.

## Prerequisites

- Rust **1.87** (the workspace pins `rust-version = "1.87"` — see
  [Known limitations](#known-limitations) for why)
- Docker, only if you want to run against a real MongoDB instead of the in-memory default

## Quickstart

Build the workspace and run the CLI directly against the in-memory default — no external
services required:

```bash
cargo build --workspace

# Ask a one-off question (uses a fresh random tenant if --tenant is omitted)
cargo run -p milona-presenter --bin milona -- query "What is Milona?" --subject alice
```

Or start the HTTP API:

```bash
cargo run -p milona-presenter --bin milona -- serve --addr 127.0.0.1:8080
```

With no `MILONA_API_KEYS` set, `serve` generates a single development API key and logs it
at startup (`tracing::warn!`) — copy it from the log line and use it as shown below. Set
`MILONA_API_KEYS` yourself for anything beyond local experimentation (see
[Configuration](#configuration)).

```bash
curl -H "x-api-key: <KEY-FROM-LOG>" \
     -H "content-type: application/json" \
     -d '{"question":"What is Milona?"}' \
     http://127.0.0.1:8080/v1/query
```

`GET /healthz` is the only unauthenticated route (liveness check, no API key required).

## Configuration

Everything below is optional; with no environment variables set, Milona runs entirely
in-memory with one auto-generated admin API key.

| Variable | Purpose | Default |
|---|---|---|
| `MILONA_STORAGE_BACKEND` | `mongo` to use real MongoDB; anything else/unset stays in-memory | in-memory |
| `MILONA_MONGO_URI` | Mongo connection string (required if backend is `mongo`) | — |
| `MILONA_MONGO_DB` | Mongo database name (required if backend is `mongo`) | — |
| `MILONA_MONGO_BACKEND` | `document_db` to mark the deployment as DocumentDB, which disables graph traversal instead of silently degrading (see [ROADMAP.md](ROADMAP.md) Key Risk #1) | `atlas` |
| `MILONA_API_KEYS` | `key1:tenant-uuid:role:subject,key2:...` — see below, role is one of `admin`/`member`/`readonly`/`service` | one auto-generated dev key |

Auth: every route except `/healthz` requires an `x-api-key` header; the middleware resolves
it to a `TenantContext` used by every downstream query. Requests are rate-limited per
authenticated key (default: 60 requests/minute via the `governor` crate).

**`MILONA_API_KEYS` is not just an API key — each entry is `key:tenant-uuid:role:subject`,
four colon-separated fields.** A bare value like `export MILONA_API_KEYS=<some-uuid>` will
not parse: it's silently skipped (logged as `skipping malformed MILONA_API_KEYS entry` at
warn level) and the server starts with **zero** valid keys, so every request — including
one using that same value as `x-api-key` — gets `401 {"error":"unauthorized","reason":"invalid
api key"}`. The second field must be a **tenant UUID**, not the API key itself; the API key
(first field) can be any string you choose.

**Only the first field is the API key — send just that in the `x-api-key` header, not the
whole `MILONA_API_KEYS` entry.** The tenant UUID/role/subject are metadata the server parses
once at startup and looks up *from* the key; they are never sent by the client.

```bash
export MILONA_API_KEYS="dev-key-123:$(uuidgen):admin:alice"
cargo run -p milona-presenter --bin milona -- serve --addr 127.0.0.1:8090
# then — note the header is just "dev-key-123", NOT the full MILONA_API_KEYS value:
curl -H "x-api-key: dev-key-123" -H "content-type: application/json" \
     -d '{"question":"What is Milona?"}' http://127.0.0.1:8090/v1/query
```

If you don't need to pin a specific key, it's simpler to leave `MILONA_API_KEYS` unset
entirely and use the auto-generated dev key logged at startup instead (see
[Quickstart](#quickstart)).

### Running against real MongoDB

```bash
docker compose up -d mongodb
export MILONA_STORAGE_BACKEND=mongo
export MILONA_MONGO_URI="mongodb://localhost:27017/?directConnection=true&replicaSet=rs0"
export MILONA_MONGO_DB=milona
cargo run -p milona-presenter --bin milona -- serve
```

`docker-compose.yml` also has an opt-in Neo4j service (`docker compose --profile neo4j up -d
neo4j`) documented as the Phase 6 graph-store fallback if `$graphLookup` limitations are hit
— it isn't wired into any code yet.

## Workspace layout

| Crate | Role |
|---|---|
| `milona-core` | Tenant model, AuthZ policy, and the trait contracts every other crate implements (`DocumentSource`, `Chunker`, `Embedder`, `VectorStore`, `GraphStore`, `LlmProvider`, `Tool`) |
| `milona-ingest` | Document sources (text/PDF/web), chunking, embedding |
| `milona-storage` | `VectorStore`/`GraphStore` over MongoDB, plus in-memory fakes for tests |
| `milona-knowledge` | Retrieval facade combining vector search + graph traversal, AuthZ-enforced |
| `milona-adapter` | LLM provider adapter with retry/backoff and per-tenant token budgets |
| `milona-tools` | Native tools + tool registry; MCP integration |
| `milona-presenter` | axum HTTP API and clap CLI (binary: `milona`) |

## Known limitations

Three components are mocked or stubbed, all for the **same root cause**: the workspace pins
`rust-version = "1.87"`, and each real dependency below transitively requires `rustc >= 1.88`
(via `darling 0.23` or, for `rmcp`, its own edition-2024 let-chain syntax). Each substitution
is documented in detail in the affected crate's top-of-file doc comment.

| Component | Currently | Real dependency blocked by MSRV |
|---|---|---|
| `milona-adapter`'s `LlmProvider` | `MockLlmProvider` (canned responses) | `genai` |
| `milona-ingest`'s `Embedder` | `MockEmbedder` (deterministic hash-based pseudo-embedding) | `fastembed-rs` / `ort` |
| `milona-tools`'s MCP transport | Stubbed trait methods returning `CoreError::Unsupported` | `rmcp` |

Retry/backoff, token-budget enforcement, native tools, the tool registry, and storage
(`MongoVectorStore`/`MongoGraphStore` alongside the in-memory fakes) are real, not mocked.

**To unblock all three at once:** raise the workspace's `rust-version` in the root
`Cargo.toml` to `>= 1.88`, then swap each mock/stub for its real implementation — the trait
seams already in place make this additive, not a redesign.

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md) for the full local validation loop (fmt, clippy,
test, `cargo audit`, `cargo deny`) and how to run the Docker-based MongoDB integration
tests — it's kept in lockstep with what `.github/workflows/ci.yml` runs on every push/PR.

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## License

Dual-licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option, per `Cargo.toml`.
