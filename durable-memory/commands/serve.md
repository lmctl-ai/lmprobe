# lmprobe serve

Start the optional GraphQL HTTP server.

## Use cases

| Case | Invocation |
| --- | --- |
| Loopback service | `lmprobe serve --addr 127.0.0.1:8080` |
| Bearer-protected remote bind | `lmprobe serve --addr 0.0.0.0:8080 --auth-token TOKEN` |
| Protect loopback too | `lmprobe serve --auth-token TOKEN` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe serve [OPTIONS]
```

| Argument / option | Behavior |
| --- | --- |
| `--addr <ADDR>` | HTTP listen address; POST GraphQL documents to /graphql [default: 127.0.0.1:8080] |
| `--auth-token <AUTH_TOKEN>` | Require this bearer token on every request, including loopback |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::Serve](../../crates/lmprobe/src/cli.rs) and `Commands::Serve`; [dispatch: Commands::Serve](../../crates/lmprobe/src/cli.rs).
- [serve_graphql](../../crates/lmprobe/src/cli.rs).
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Requires --features serve. POST /graphql uses build_schema, including the Mutation root. A configured token is enforced even on loopback. Non-loopback without a token fails. --workspace is not forwarded into the daemon schema.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [auth](../../crates/lmprobe/tests/auth.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --features serve --test auth
```

Also check `lmprobe serve -h` and `lmprobe serve --help` after option changes.
