# lmprobe secrets

Scan files or Git history for potential secrets.

## Use cases

| Case | Invocation |
| --- | --- |
| Live files with bundled rules | `lmprobe secrets .` |
| Custom rules and baseline | `lmprobe secrets --config gitleaks.toml --baseline baseline.json src/` |
| Full Git history | `lmprobe secrets --source git .` |
| Compatibility comparison | `lmprobe secrets --gitleaks-compat .` |
| AST annotations or suppression | `lmprobe secrets --ast-fp-cut suppress .` |
| Syntax-only credential sinks | `lmprobe secrets --sink-evidence on .` |
| Walk sizing/symlinks | `lmprobe secrets --max-target-megabytes 10 --follow-symlinks .` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe secrets [OPTIONS] [PATH]
```

| Argument / option | Behavior |
| --- | --- |
| `[PATH]` | [default: .] |
| `--config <CONFIG>` | Gitleaks-compatible TOML rules (default: bundled rules) |
| `--max-target-megabytes <MAX_TARGET_MEGABYTES>` | Skip files larger than this many MB, matching gitleaks' target-size terminology [default: 5] |
| `--follow-symlinks` | Follow symlinks during the filesystem walk |
| `--gitleaks-compat` | Ignore ignore-files and use gitleaks-compatible finding exit codes |
| `--ast-fp-cut <AST_FP_CUT>` | AST context mode: annotate findings, suppress non-code findings, or off [default: annotate] [possible values: off, annotate, suppress] |
| `--sink-evidence <SINK_EVIDENCE>` | Include value-independent credential sink evidence (syntax only) [default: off] [possible values: off, on] |
| `--source <SOURCE>` | Scan live files (filesystem) or full Git history (git) [default: filesystem] [possible values: filesystem, git] |
| `--baseline <BASELINE>` | Suppress findings whose fingerprints appear in a gitleaks JSON baseline |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::Secrets](../../crates/lmprobe/src/cli.rs) and `Commands::Secrets`; [dispatch: Commands::Secrets](../../crates/lmprobe/src/cli.rs).
- [Engine: secrets::scan_filesystem](../../crates/lmprobe-engine/src/secrets.rs).
- [Result schema: SecretsResult](../../crates/lmprobe-types/src/schema.rs).
- [Rules, redaction, providers, and Git scan](../../crates/lmprobe-secrets/src/lib.rs).
- [GraphQL resolver: secrets](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Findings must remain redacted. partial=true is independent of finding presence. Gitleaks compatibility changes walking and exit conventions. Git-source implementation is scan_git_history in lmprobe-engine/src/secrets.rs; avoid persisting live-tree secret values.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [secrets_partial](../../crates/lmprobe/tests/secrets_partial.rs)
- [graphql_secrets](../../crates/lmprobe/tests/graphql_secrets.rs)
- [secrets_recipes_cli](../../crates/lmprobe/tests/secrets_recipes_cli.rs)
- [secrets_sources_cli](../../crates/lmprobe/tests/secrets_sources_cli.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test secrets_partial
cargo test -p lmprobe --locked --test graphql_secrets
```

Also check `lmprobe secrets -h` and `lmprobe secrets --help` after option changes.
