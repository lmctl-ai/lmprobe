# lmprobe def

Find a symbol definition by exact name.

## Use cases

| Case | Invocation |
| --- | --- |
| Exact symbol and skeleton | `lmprobe def Parser src/` |
| Substring name | `lmprobe def parse --regex src/` |
| Full body and nearby lines | `lmprobe def --body --around 3 parse src/` |
| Method in a specific type | `lmprobe def render --class Widget src/` |
| Prefer implementations in one file | `lmprobe def render --this-file src/widget.ts` |
| Include data-language definitions | `lmprobe def database --scope all .` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe def [OPTIONS] <NAME> [PATH]
```

| Argument / option | Behavior |
| --- | --- |
| `<NAME>` | Required command input; see use cases above. |
| `[PATH]` | [default: .] |
| `--regex` | Use substring matching instead of an exact identifier |
| `--class <CLASS>` | Restrict method/function definitions to a containing class/type name |
| `--this-file` | Require PATH to be a file and prefer concrete implementation hits over interface/trait signatures in that file |
| `--body` | Return full declaration bodies instead of skeletonized content |
| `--around <AROUND>` | Include N source lines before and after each definition hit |
| `--scope <SCOPE>` | Choose programming languages (code) or all supported languages (all) [default: code] [possible values: code, all] |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::Def](../../crates/lmprobe/src/cli.rs) and `Commands::Def`; [dispatch: Commands::Def](../../crates/lmprobe/src/cli.rs).
- [Engine: sensors::def](../../crates/lmprobe-engine/src/sensors.rs).
- [Result schema: DefResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: def](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

`--regex` is substring mode, not an arbitrary regex engine. `--this-file` requires a file and prefers concrete implementations. Default content is skeletonized; `--body` restores full source. Scope defaults to code.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [def_body_mode](../../crates/lmprobe/tests/def_body_mode.rs)
- [def_class_filter](../../crates/lmprobe/tests/def_class_filter.rs)
- [def_this_file](../../crates/lmprobe/tests/def_this_file.rs)
- [python_typed_def_patterns](../../crates/lmprobe/tests/python_typed_def_patterns.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test def_body_mode
cargo test -p lmprobe --locked --test def_class_filter
```

Also check `lmprobe def -h` and `lmprobe def --help` after option changes.
