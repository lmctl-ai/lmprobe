# lmprobe py-slice-candidates

Find Python slice/unsliced-variable forwarding suspects.

## Use cases

| Case | Invocation |
| --- | --- |
| Static suspects across files/roots | `lmprobe py-slice-candidates src/ tests/` |
| Candidate IDs and watch hints | `lmprobe py-slice-candidates src/ --trace-plan` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe py-slice-candidates [OPTIONS] <PATHS>...
```

| Argument / option | Behavior |
| --- | --- |
| `<PATHS>...` | Required Python files or directories; use . to scan the current directory |
| `--trace-plan` | Include deterministic candidate ids and trace watch plan hints |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::PySliceCandidates](../../crates/lmprobe/src/cli.rs) and `Commands::PySliceCandidates`; [dispatch: Commands::PySliceCandidates](../../crates/lmprobe/src/cli.rs).
- [Engine: sensors::py_slice_candidates](../../crates/lmprobe-engine/src/sensors.rs).
- [Result schema: PySliceCandidatesResult](../../crates/lmprobe-types/src/schema.rs).
- No direct same-name GraphQL resolver; inspect query composition before extending surface parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Python only, low confidence. --trace-plan adds deterministic IDs and watch suggestions; it does not execute a trace.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [py_slice_candidates](../../crates/lmprobe/tests/py_slice_candidates.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test py_slice_candidates
```

Also check `lmprobe py-slice-candidates -h` and `lmprobe py-slice-candidates --help` after option changes.
