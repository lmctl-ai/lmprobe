# lmprobe history evolution

Show a symbol's changes through Git history.

## Use cases

| Case | Invocation |
| --- | --- |
| Top-level evolution alias | `lmprobe history evolution Parser .` |
| Refresh or substring mode | `lmprobe history evolution parse --regex --force .` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe history evolution [OPTIONS] <NAME> [PATH]
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

- [CLI arguments: HistoryCmd::Evolution](../../crates/lmprobe/src/cli.rs) and `HistoryCmd::Evolution`; [dispatch: HistoryCmd::Evolution](../../crates/lmprobe/src/cli.rs).
- [Engine: sensors::evolution_with_regex](../../crates/lmprobe-engine/src/sensors.rs).
- [Result schema: EvolutionResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: evolution](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Alias dispatch currently creates default WalkOpts rather than using global walk filters. It still forwards --force, --regex, and --no-cache. Keep this distinction visible when changing alias parity.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [smoke](../../crates/lmprobe/tests/smoke.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test smoke
```

Also check `lmprobe history evolution -h` and `lmprobe history evolution --help` after option changes.
