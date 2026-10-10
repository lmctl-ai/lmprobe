# lmprobe evolution

Show a symbol's changes through Git history.

## Use cases

| Case | Invocation |
| --- | --- |
| Exact name milestones | `lmprobe evolution Parser .` |
| Substring name | `lmprobe evolution parse --regex .` |
| Refresh milestones | `lmprobe evolution Parser --force .` |
| Bypass memoized milestones | `lmprobe evolution Parser --no-cache .` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe evolution [OPTIONS] <NAME> [PATH]
```

| Argument / option | Behavior |
| --- | --- |
| `<NAME>` | Required command input; see use cases above. |
| `[PATH]` | [default: .] |
| `--force` | Refresh symbol milestones instead of using cached milestones |
| `--regex` | Use substring matching instead of an exact identifier |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::Evolution](../../crates/lmprobe/src/cli.rs) and `Commands::Evolution`; [dispatch: Commands::Evolution](../../crates/lmprobe/src/cli.rs).
- [Engine: sensors::evolution_with_regex](../../crates/lmprobe-engine/src/sensors.rs).
- [Result schema: EvolutionResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: evolution](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

--force refreshes symbol milestones; it is not a mutation safety override. Git/cache history is separate from live-tree parsing.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [smoke](../../crates/lmprobe/tests/smoke.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test smoke
```

Also check `lmprobe evolution -h` and `lmprobe evolution --help` after option changes.
