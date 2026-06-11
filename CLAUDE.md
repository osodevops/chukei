# chukei — AI-assisted development context

A transparent, wire-protocol-level proxy for Snowflake (Databricks P1) that
caches, routes, rewrites, right-sizes, suspends, and attributes — with zero
client changes. Full spec: `docs/chukei_prd.md`.

## Build / test / lint

```bash
cargo build
cargo test                                        # unit + e2e (mock Snowflake origin)
cargo fmt --all                                   # CI runs --check
cargo clippy --all-targets --all-features -- -D warnings
```

All three must pass before any commit. CI mirrors these exactly.

## Architecture map

| Path (in `crates/chukei-core/src/`) | What it is |
|---|---|
| `wire/sf/` | Snowflake HTTPS shim; intercepts only `POST /queries/v1/query-request`, everything else passes through verbatim |
| `sql/` | parse (sqlparser-rs Snowflake dialect) → normalise → blake3 hard fingerprint + feature-based soft fingerprint |
| `plugin/` | `Plugin` trait, `Decision` enum, ordered bus with precedence Veto > ServeFromCache > Route > Rewrite > SetWarehouseSize > Annotate |
| `cache/` `router/` `rewrite/` `suspend/` `attribute/` | the P0 plugins |
| `replay/` | offline savings simulator over `query_history` CSV |
| `evidence/` | ECDSA-P256-SHA256 signed reports |
| `config.rs` | YAML config with `${VAR}` env interpolation |

`crates/chukei-cli` is a thin clap wrapper; all logic lives in core.

## Non-negotiable invariants

1. **Hot path is deterministic Rust only.** No LLM, no Python, no network
   calls beyond the upstream forward. Budget: +5 ms p99.
2. **Fail open.** Any chukei-side failure (parse error, plugin panic,
   cache miss) must degrade to verbatim passthrough, never break a query.
3. **False-positive intolerant cache.** Never cache non-deterministic
   queries, writes, or chunked responses. When in doubt, miss.
4. **Credentials are never persisted or logged.** Session tokens live in
   memory only.
5. **Plugins communicate only via `Decision`.** No side channels.

## Version policy

Workspace single-version. Any PR touching `crates/`, `Cargo.toml`,
`Cargo.lock`, or `Dockerfile` must bump `[workspace.package].version`:
patch for fixes, minor for any breaking change to `chukei-core`'s public API.

## Testing conventions

- Every rewrite rule ships positive *and* negative tests (when it must not fire).
- e2e tests use the in-process mock Snowflake origin in
  `crates/chukei-core/tests/proxy_e2e.rs` — extend it rather than mocking ad hoc.
- Every shipped bug becomes a permanent reproducer test (`tests/reproduce-bugN/`).
