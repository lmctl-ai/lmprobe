# lmprobe discover

Find project roots and nested projects.

## Use cases

| Case | Invocation |
| --- | --- |
| Flat roots and marker metadata | `lmprobe discover .` |
| Nested monorepo tree | `lmprobe discover --tree --format markdown .` |
| Only outermost roots | `lmprobe discover --top-level-only .` |
| Bound walk depth | `lmprobe discover --depth 2 .` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe discover [OPTIONS] [PATH]
```

| Argument / option | Behavior |
| --- | --- |
| `[PATH]` | [default: .] |
| `--tree` | Render the parent/child project hierarchy as an indented tree (markdown only; JSON/SARIF carry the `children` field on each Project) |
| `--depth <DEPTH>` | Cap walker recursion depth |
| `--top-level-only` | Emit only projects with no discovered project parent |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::Discover](../../crates/lmprobe/src/cli.rs) and `Commands::Discover`; [dispatch: Commands::Discover](../../crates/lmprobe/src/cli.rs).
- [Engine: sensors::discover](../../crates/lmprobe-engine/src/sensors.rs).
- [Result schema: DiscoverResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: discover](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Use --top-level-only for a compact monorepo map.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [discover_top_level_only](../../crates/lmprobe/tests/discover_top_level_only.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test discover_top_level_only
```

Also check `lmprobe discover -h` and `lmprobe discover --help` after option changes.
