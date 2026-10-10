# lmprobe impact

Walk callers or callees to estimate change impact.

## Use cases

| Case | Invocation |
| --- | --- |
| Callers recursively | `lmprobe impact handle_request --depth 2 src/` |
| Callees recursively | `lmprobe impact handle_request --direction downstream src/` |
| Include or exclude test nodes | `lmprobe impact handle_request --exclude-tests .` |
| Export a graph | `lmprobe impact handle_request --format mermaid .` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe impact [OPTIONS] <SYMBOL> [PATH]
```

| Argument / option | Behavior |
| --- | --- |
| `<SYMBOL>` | Required command input; see use cases above. |
| `[PATH]` | [default: .] |
| `--direction <DIRECTION>` | Walk callers (upstream) or callees (downstream) Possible values: - upstream:   "Who calls me?" — recurse callers - downstream: "What do I call?" — recurse callees [default: upstream] |
| `--depth <DEPTH>` | Maximum graph traversal depth [default: 3] |
| `--tests-only` | Keep only impact nodes from likely test paths |
| `--exclude-tests` | Drop impact nodes from likely test paths |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::Impact](../../crates/lmprobe/src/cli.rs) and `Commands::Impact`; [dispatch: Commands::Impact](../../crates/lmprobe/src/cli.rs).
- [Engine: impact::impact](../../crates/lmprobe-engine/src/impact.rs).
- [Result schema: ImpactResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: impact](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Path-based test filters are mutually exclusive. Confidence labels are mechanical traversal evidence, not a sound call graph.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [impact_file_filter](../../crates/lmprobe/tests/impact_file_filter.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test impact_file_filter
```

Also check `lmprobe impact -h` and `lmprobe impact --help` after option changes.
