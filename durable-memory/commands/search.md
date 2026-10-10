# lmprobe search

Find declarations by name, kind, or AST pattern.

## Use cases

| Case | Invocation |
| --- | --- |
| Name substring | `lmprobe search --name Parser --path src/` |
| Portable or raw kind | `lmprobe search --kind function --outline --path src/` |
| AST pattern with a parser | `lmprobe search -p 'foo($X)' -l python src/` |
| Infer parser from one file | `lmprobe search -p 'foo($X)' src/app.py` |
| Relational rules | `lmprobe search --kind function --has 'kind: call' --path src/` |
| Data child filters | `lmprobe search --has-child database --outline --type yaml --path config/` |
| YAML rule file | `lmprobe search --rule rule.yaml --path src/` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe search [OPTIONS] [QUERY] [PATH]
```

| Argument / option | Behavior |
| --- | --- |
| `[QUERY]` | Deprecated positional query; prefer --name or -p |
| `[PATH]` | [default: .] |
| `--path <PATH>` | Explicit search root; avoids positional query/path ambiguity |
| `--outline` | Render the body-elided declaration outline |
| `--body` | Return full declaration bodies |
| `-p, --pattern <PATTERN>` | AST pattern; directories need -l, a single file can infer its language |
| `--strictness <STRICTNESS>` | Match strictness for the pattern engine [possible values: cst, smart, ast, relaxed, signature, template] |
| `--name <NAME>` | Case-insensitive substring filter on declaration names |
| `--kind <KIND>` | Portable kind (function, class, struct, etc.) or parser-specific kind |
| `--raw-kind <RAW_KIND>` | Tree-sitter literal kind — bypasses the taxonomy |
| `--has <HAS>` | Require a matching descendant (inline YAML rule) |
| `--inside <INSIDE>` | Require a matching ancestor (inline YAML rule) |
| `--precedes <PRECEDES>` | Require a following match (inline YAML rule) |
| `--follows <FOLLOWS>` | Require a preceding match (inline YAML rule) |
| `--not <NOT>` | Exclude nodes matching this inline YAML rule |
| `--all <ALL>` | Require every rule (repeatable inline YAML) |
| `--any <ANY>` | Require at least one rule (repeatable inline YAML) |
| `-r, --rule <RULE>` | YAML rule file applied as a descendant filter on candidate declarations |
| `--has-child <HAS_CHILD>` | Filter by meaningful direct-child text (declarations or data structure children) |
| `-h, --help` | Print help (see a summary with '-h') |
| `--has-decoration <TEXT>` (hidden) | Deprecated decoration substring filter; prefer --has. |
| `--has-attr <TEXT>` (hidden) | Compatibility decoration/attribute filter; inspect search dispatch before changing. |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::Search](../../crates/lmprobe/src/cli.rs) and `Commands::Search`; [dispatch: Commands::Search](../../crates/lmprobe/src/cli.rs).
- [Engine: sensors::search_symbol_hits](../../crates/lmprobe-engine/src/sensors.rs).
- [Result schema: SearchResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: search](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Deprecated positional queries emit diagnostics. `--name` wins positional ambiguity; use `--path` for clarity. `--body` is accepted but full bodies are already the default. `--outline` conflicts with `--body`; `--name` conflicts with `-p`. `--rule` is a descendant filter, not a replacement candidate matcher. Hidden compatibility flags `--has-decoration` and `--has-attr` still exist.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [search_name_path_hint](../../crates/lmprobe/tests/search_name_path_hint.rs)
- [search_auto_language_detect](../../crates/lmprobe/tests/search_auto_language_detect.rs)
- [python_typed_def_patterns](../../crates/lmprobe/tests/python_typed_def_patterns.rs)
- [pattern_strictness_recovery](../../crates/lmprobe/tests/pattern_strictness_recovery.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test search_name_path_hint
cargo test -p lmprobe --locked --test search_auto_language_detect
```

Also check `lmprobe search -h` and `lmprobe search --help` after option changes.
