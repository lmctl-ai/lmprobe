# lmprobe route-anchors

Find route declarations and literal HTTP paths.

## Use cases

| Case | Invocation |
| --- | --- |
| Auto-detect supported route syntax | `lmprobe route-anchors src/` |
| Explicit FastAPI | `lmprobe route-anchors src/ --framework fastapi --around 2 --parents --body` |
| Generic literal route calls | `lmprobe route-anchors src/ --framework generic` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe route-anchors [OPTIONS] <PATH>
```

| Argument / option | Behavior |
| --- | --- |
| `<PATH>` | Scan root. |
| `--framework <FRAMEWORK>` | Route syntax to scan (auto-detected when omitted) [possible values: fastapi, generic] |
| `--around <AROUND>` | Include N source lines around route anchors |
| `--parents` | Attach enclosing structural declaration chain |
| `--body` | Attach innermost enclosing structural body |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::RouteAnchors](../../crates/lmprobe/src/cli.rs) and `Commands::RouteAnchors`; [dispatch: Commands::RouteAnchors](../../crates/lmprobe/src/cli.rs).
- [Engine: api_facts::route_anchors](../../crates/lmprobe-engine/src/api_facts.rs).
- [Result schema: RouteAnchorsResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: route_anchors](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

FastAPI decorators and generic literal .get/.post-style calls only; not runtime routing resolution.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [api_facts](../../crates/lmprobe/tests/api_facts.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test api_facts
```

Also check `lmprobe route-anchors -h` and `lmprobe route-anchors --help` after option changes.
