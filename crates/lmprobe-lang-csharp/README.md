# `lmprobe-lang-csharp`

Per-language D5 capability impls for **csharp**.

This crate is part of the [lmprobe](https://github.com/lmctl-ai/lmprobe) workspace. It depends on:

- `lmprobe-lang-core` — D5 trait definitions + `SgLang` re-export.
- `lmprobe-types` — D4 envelope shapes (transitively).
- `ast-grep-core` — pattern engine.

It does **not** depend on any other `lmprobe-lang-*` crate.

## Surface

The crate exports 8 unit structs, one per D5 trait:

| Struct | Trait | Purpose |
|--------|-------|---------|
| `CSharpExtractor` | `NameExtractor` | Per-declaration symbol-name extraction. |
| `CSharpCallGrammar` | `CallGrammar` | Call-site patterns for `callers` / `callees`. |
| `CSharpImportGrammar` | `ImportGrammar` | Import-statement patterns. |
| `CSharpInheritanceGrammar` | `InheritanceGrammar` | Supertype / subtype patterns. |
| `CSharpDecorationGrammar` | `DecorationGrammar` | Decorator / attribute patterns. |
| `CSharpTraceTemplate` | `TraceTemplate` | Default `trace` instrumentation template. |
| `CSharpKindTaxonomy` | `KindTaxonomy` | Portable-kind ↔ tree-sitter-kind mapping. |
| `CSharpTopLevelKinds` | `TopLevelKinds` | (Data-langs only) child-kind enumeration. |

The `lmprobe-registry` crate's per-language dispatch arms are auto-wired by `cargo xtask new-lang csharp`.

## Tests

```bash
cargo test -p lmprobe-lang-csharp
```

The smoke harness asserts every struct is constructible. Add fixture-based assertions in `tests/smoke.rs` as you implement the trait surface.

## Fixtures

`fixtures/` holds small in-tree csharp files used by smoke tests. Keep them ≤ 5 files, ≤ 30 lines each — the deep-tier corpus lives at `$LMPROBE_QA_CORPUS/csharp/` (workspace-external).
