# lmprobe guard-sites

Find sensitive actions and nearby guard anchors.

## Use cases

| Case | Invocation |
| --- | --- |
| Sensitive/guard regexes | `lmprobe guard-sites . --sensitive 'Delete\|exec' --guard 'auth\|allowed'` |
| Same-declaration and nearby evidence | `lmprobe guard-sites src/ --sensitive 'SaveUploadedFile' --guard 'auth\|middleware' --around 8 --parents --body` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe guard-sites [OPTIONS] --sensitive <SENSITIVE> --guard <GUARD> <PATH>
```

| Argument / option | Behavior |
| --- | --- |
| `<PATH>` | Scan root. |
| `--sensitive <SENSITIVE>` | Regex for sensitive-action anchors |
| `--guard <GUARD>` | Regex for nearby guard-looking anchors |
| `--around <AROUND>` | Include N source lines around sensitive anchors and search guards in that window |
| `--parents` | Attach enclosing structural declaration chain and prefer same-declaration guards |
| `--body` | Attach innermost enclosing structural body |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::GuardSites](../../crates/lmprobe/src/cli.rs) and `Commands::GuardSites`; [dispatch: Commands::GuardSites](../../crates/lmprobe/src/cli.rs).
- [Engine: guard_sites::guard_sites](../../crates/lmprobe-engine/src/guard_sites.rs).
- [Result schema: GuardSitesResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: guard_sites](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Reports local syntactic/lexical evidence, not ownership, control flow, authorization, compliance, or data-flow correctness. Preserve source anchors and diagnostics when extending coverage.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [guard_sites](../../crates/lmprobe/tests/guard_sites.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test guard_sites
```

Also check `lmprobe guard-sites -h` and `lmprobe guard-sites --help` after option changes.
