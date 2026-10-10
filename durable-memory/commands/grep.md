# lmprobe grep

Search file contents with a regular expression.

## Use cases

| Case | Invocation |
| --- | --- |
| Rust-regex lines | `lmprobe grep 'TODO\|FIXME' src/` |
| Several explicit roots | `lmprobe grep 'TODO' --path src --path tests` |
| Case and surrounding lines | `lmprobe grep -i 'todo' -C 2 src/` |
| AST context/roles | `lmprobe grep 'SELECT' --parents --body --role --format json src/` |
| Ancestor-kind filtering | `lmprobe grep 'SELECT' --inside-kind for_statement,while_statement src/` |
| Narrow broad scans to projects | `lmprobe grep 'oauth' --quick-projects ~/repos` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe grep [OPTIONS] <PATTERN> [PATH]
```

| Argument / option | Behavior |
| --- | --- |
| `<PATTERN>` | Required command input; see use cases above. |
| `[PATH]` | Single compatibility path. When omitted, `grep` defaults to `.` unless one or more `--path <PATH>` flags are set |
| `--path <PATH>` | Additional scan roots (repeatable); defaults to . when no paths are given |
| `--parents` | Attach enclosing structural declaration chain metadata to JSON/NDJSON hits |
| `--body` | Attach innermost enclosing structural body metadata/content to JSON/NDJSON hits |
| `--inside-kind <INSIDE_KIND>` | Filter hits to those inside one of the listed AST ancestor kinds (comma-separated `tree-sitter` kind names) |
| `-n, --line-number` | Compatibility flag; line numbers are already included |
| `--role` | Attach a mechanical local role classification to JSON/NDJSON hits |
| `--quick-projects` | Narrow scans to npm roots or directories with at least five Perl files |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::Grep](../../crates/lmprobe/src/cli.rs) and `Commands::Grep`; [dispatch: Commands::Grep](../../crates/lmprobe/src/cli.rs).
- [Engine: sensors::grep_many](../../crates/lmprobe-engine/src/sensors.rs).
- [Result schema: GrepResultEnvelope](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: grep](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Rust regex: | alternates; \| matches a literal pipe. PATTERN is positional, not -p. Multiple roots are via --path, not multiple positional paths. Structural expansions/roles appear in JSON/NDJSON. Partial reads must set partial and retain warnings; --line-number is a compatibility no-op.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [grep_parents](../../crates/lmprobe/tests/grep_parents.rs)
- [grep_pattern_recovery](../../crates/lmprobe/tests/grep_pattern_recovery.rs)
- [grep_json_schema](../../crates/lmprobe/tests/grep_json_schema.rs)
- [grep_context_lines](../../crates/lmprobe/tests/grep_context_lines.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test grep_parents
cargo test -p lmprobe --locked --test grep_pattern_recovery
```

Also check `lmprobe grep -h` and `lmprobe grep --help` after option changes.
