# lmprobe http-client-calls

Find HTTP client calls with literal URLs.

## Use cases

| Case | Invocation |
| --- | --- |
| Literal URL call anchors | `lmprobe http-client-calls src/` |
| Source and structure | `lmprobe http-client-calls src/ --around 2 --parents --body` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe http-client-calls [OPTIONS] <PATH>
```

| Argument / option | Behavior |
| --- | --- |
| `<PATH>` | Scan root. |
| `--around <AROUND>` | Include N source lines around client call anchors |
| `--parents` | Attach enclosing structural declaration chain |
| `--body` | Attach innermost enclosing structural body |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::HttpClientCalls](../../crates/lmprobe/src/cli.rs) and `Commands::HttpClientCalls`; [dispatch: Commands::HttpClientCalls](../../crates/lmprobe/src/cli.rs).
- [Engine: api_facts::http_client_calls](../../crates/lmprobe-engine/src/api_facts.rs).
- [Result schema: HttpClientCallsResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: http_client_calls](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Literal URLs only; dynamic URL expressions are not resolved. Python requests/httpx and JS/TS fetch/axios are the implemented slice.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [api_facts](../../crates/lmprobe/tests/api_facts.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test api_facts
```

Also check `lmprobe http-client-calls -h` and `lmprobe http-client-calls --help` after option changes.
