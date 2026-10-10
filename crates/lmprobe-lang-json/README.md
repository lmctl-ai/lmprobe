# `lmprobe-lang-json`

Per-language D5 capability impls for **json**.

This crate is part of the [lmprobe](https://github.com/lmctl-ai/lmprobe) workspace. It depends on:

- `lmprobe-lang-core` — D5 trait definitions + `SgLang` re-export.
- `lmprobe-types` — D4 envelope shapes (transitively).
- `ast-grep-core` — pattern engine.

It does **not** depend on any other `lmprobe-lang-*` crate.

## Surface

The crate exports 8 unit structs, one per D5 trait:

| Struct | Trait | Purpose |
|--------|-------|---------|
| `JsonExtractor` | `NameExtractor` | Per-declaration symbol-name extraction. |
| `JsonCallGrammar` | `CallGrammar` | Call-site patterns for `callers` / `callees`. |
| `JsonImportGrammar` | `ImportGrammar` | Import-statement patterns. |
| `JsonInheritanceGrammar` | `InheritanceGrammar` | Supertype / subtype patterns. |
| `JsonDecorationGrammar` | `DecorationGrammar` | Decorator / attribute patterns. |
| `JsonTraceTemplate` | `TraceTemplate` | Default `trace` instrumentation template. |
| `JsonKindTaxonomy` | `KindTaxonomy` | Portable-kind ↔ tree-sitter-kind mapping. |
| `JsonTopLevelKinds` | `TopLevelKinds` | (Data-langs only) child-kind enumeration. |

The `lmprobe-registry` crate's per-language dispatch arms are auto-wired by `cargo xtask new-lang json`.

## Tests

```bash
cargo test -p lmprobe-lang-json
```

The smoke harness asserts every struct is constructible. Add fixture-based assertions in `tests/smoke.rs` as you implement the trait surface.

## Fixtures

`fixtures/` holds small in-tree json files used by smoke tests. Keep them ≤ 5 files, ≤ 30 lines each — the deep-tier corpus lives at `$LMPROBE_QA_CORPUS/json/` (workspace-external).
