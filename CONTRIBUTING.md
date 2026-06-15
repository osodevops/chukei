# Contributing to chukei

chukei is a fair-source Snowflake cost proxy written in Rust. Contributions are
welcome, with one launch constraint: Snowflake is the only production target for
the public launch. Databricks and other adapters should stay as roadmap/design
discussion unless maintainers explicitly ask for implementation work.

## Ground Rules

- Keep the hot path deterministic. No LLM calls, network lookups, or probabilistic
  decisions in query forwarding.
- Fail open. Parse errors, plugin failures, cache misses, and policy uncertainty
  must pass traffic through to Snowflake.
- Treat cache false positives as security bugs. Any cache hit must be
  equivalence-checked or conservative.
- Do not commit customer data, query text from private workloads, credentials,
  connection strings, account locators tied to real tenants, or signed evidence
  bundles from real customers.
- Keep public copy accurate: use "fair-source", "source-available", "FSL", and
  "Apache in 2 years"; do not describe chukei itself as open source.

## Development Setup

Install stable Rust, then run:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

Optional release checks:

```bash
cargo package --workspace --allow-dirty --no-verify
docker build -t chukei:local .
docker run --rm chukei:local --version
```

Configuration examples live in `config/`. Deployment manifests live in
`deploy/`. The public documentation lives in the separate
`osodevops/chukei-docs` repository.

## Issues

Good first issues should be small, testable, and scoped to one crate or one docs
page. Maintainers use these labels:

- `bug`
- `enhancement`
- `documentation`
- `good-first-issue`
- `help-wanted`
- `plugin`
- `chukei-lab`
- `protocol`
- `performance`

Security issues should not be filed publicly. Follow `SECURITY.md`.

## Pull Requests

Before opening a PR:

1. Run formatting, clippy, and tests.
2. Add or update tests when behavior changes.
3. Update README/docs when commands, config, or deployment behavior changes.
4. Keep unrelated refactors out of functional changes.
5. Explain the user-visible behavior change and any rollback risk.

Small PRs are preferred. If a change touches protocol behavior, cache validity,
evidence signing, or fail-open behavior, call that out explicitly in the PR body.

## Licensing

By contributing, you agree that your contribution is licensed under the same
license as the project: Functional Source License 1.1, ALv2 Future License
(`FSL-1.1-ALv2`). Each release converts to Apache-2.0 on the second anniversary
of the date it is made available.
