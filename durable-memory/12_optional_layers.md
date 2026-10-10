# 12 — Optional layer: `serve` (HTTP daemon)

GraphQL is part of the default lmprobe build — `lmprobe query '<gql>'` runs the full schema in-process. The `serve` subcommand wraps **the same schema** in a long-running HTTP daemon. It's the only opt-in feature-gated layer; the default CLI (including `query`) does not pull in axum or multi-thread tokio.

This is the doctrine restated in [01_overview.md](01_overview.md):

> Long-running surfaces (server, daemon, REPL) are opt-in layers, not the default. A user who runs `lmprobe search Foo .` should never accidentally start a daemon.

For the GraphQL surface itself (`lmprobe query`, available on every build) see [17_graphql.md](17_graphql.md). This chapter is the daemon.

## Feature flags

From `crates/lmprobe/Cargo.toml`:

```toml
[features]
default = []
serve = ["dep:async-graphql-axum", "dep:axum", "dep:subtle", "tokio/rt-multi-thread", "tokio/net"]
```

| Feature | Adds | Cost |
|---------|------|------|
| `serve` | async-graphql-axum, axum, subtle, multi-thread tokio | ~3 MB extra binary; ~10 transitive crates beyond the default set. |

Build matrix:

```bash
cargo build -p lmprobe --release                  # default CLI + in-process `query`
cargo build -p lmprobe --release --features serve # adds the HTTP daemon
```

Both configurations build warning-free. CI exercises both.

## When to pick `serve` vs `query`

For most agents and scripts, `lmprobe query '<gql>'` (CLI subprocess) is the right shape:
- Default build, no extra deps.
- One-shot — no daemon to manage.
- Stateless — every invocation parses the working tree fresh.
- Returns the standard `lmprobe.query.v1` envelope.

Pick `serve` when a long-running HTTP server is genuinely the right shape:
- A CI service with hundreds of concurrent agents (subprocess startup × 100/sec is a real cost).
- Embedding lmprobe behind a web UI that needs the GraphQL schema directly.
- Cross-language clients that already speak HTTP/GraphQL.

If you're running `lmprobe query` more than ~10 times/sec from one process, `serve` will pay for itself. Otherwise `query` is simpler and cheaper.

## Without `serve`

The `serve` subcommand exists in the CLI surface (so `--help` is consistent across builds), but invoking it on a default build prints:

```
Error: the `serve` subcommand requires `--features serve`; rebuild with `cargo build --features serve`
```

and exits 2.

## Running the daemon

```bash
lmprobe serve --addr 127.0.0.1:8080
```

Endpoint: `POST /graphql`. The schema is identical to the in-process surface — all read/mutation fields documented in [17_graphql.md](17_graphql.md). Schema-first invariant: the JSON shape is identical whether you call via CLI verb, `lmprobe query`, or `serve`.

`--workspace` mode is not yet wired into the daemon (single-root only). Plumbing it in remains a follow-up if user demand emerges.

## Auth

Loopback binds (`127.0.0.1`, `::1`, `localhost`) skip auth.

Non-loopback binds **require** `--auth-token`:

```bash
lmprobe serve --addr 0.0.0.0:8080 --auth-token "$(uuidgen)"
```

Without `--auth-token`, non-loopback bind is refused at startup.

Each request must carry `Authorization: Bearer <token>`. Token compare uses constant-time via `subtle::ConstantTimeEq` (timing-oracle hardening). Length-mismatched compares are rejected by `ct_eq` on length; the implementation also explicitly checks length before bytes-compare so the constant-time bytes path runs only on equal-length slices.

Auth integration test: `crates/lmprobe/tests/auth.rs` (`#![cfg(feature = "serve")]`-gated).

## Doctrine rationale

Three reasons the HTTP daemon is opt-in:

1. **Smaller default binary.** axum + multi-thread tokio + subtle are ~3 MB and ~10 transitive crates; the default user (one-shot CLI + `query`) doesn't pay for them.
2. **Smaller attack surface.** Default users have no network listener. The bearer-token middleware + constant-time compare exists, but the safest exposure is "no exposure."
3. **Honest mental model.** A CLI that sometimes-is-a-daemon-too leads to "wait, did I start a daemon?" confusion. Opt-in eliminates that.

## Historical note: MCP

A stdio MCP server shipped behind `--features mcp` in Phase 1.10 and was removed in Phase 4E. Agents now consume the unified GraphQL contract — one schema, one envelope shape, one set of resolvers — via either `lmprobe query` (subprocess, one-shot, default build) or `serve` (HTTP daemon, opt-in).

## Read next

- [17_graphql.md](17_graphql.md) for the GraphQL surface (default-built; same schema `serve` exposes).
- [02_invocation.md](02_invocation.md) for the build matrix.
- [13_workspace.md](13_workspace.md) for the multi-repo CLI mode (separate from server modes).
