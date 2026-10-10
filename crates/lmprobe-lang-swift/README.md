# `lmprobe-lang-swift`

Per-language D5 capability impls for **swift**.

This crate is part of the [lmprobe](https://github.com/lmctl-ai/lmprobe) workspace. It depends on:

- `lmprobe-lang-core` — D5 trait definitions + `SgLang` re-export.
- `lmprobe-types` — D4 envelope shapes (transitively).
- `ast-grep-core` — pattern engine.

It does **not** depend on any other `lmprobe-lang-*` crate.

## Surface

The crate exports 8 unit structs, one per D5 trait:

| Struct | Trait | Purpose |
|--------|-------|---------|
| `SwiftExtractor` | `NameExtractor` | Per-declaration symbol-name extraction. |
| `SwiftCallGrammar` | `CallGrammar` | Call-site patterns for `callers` / `callees`. |
| `SwiftImportGrammar` | `ImportGrammar` | Import-statement patterns. |
| `SwiftInheritanceGrammar` | `InheritanceGrammar` | Supertype / subtype patterns. |
| `SwiftDecorationGrammar` | `DecorationGrammar` | Decorator / attribute patterns. |
| `SwiftTraceTemplate` | `TraceTemplate` | Default `trace` instrumentation template. |
| `SwiftKindTaxonomy` | `KindTaxonomy` | Portable-kind ↔ tree-sitter-kind mapping. |
| `SwiftTopLevelKinds` | `TopLevelKinds` | (Data-langs only) child-kind enumeration. |

The `lmprobe-registry` crate's per-language dispatch arms are auto-wired by `cargo xtask new-lang swift`.

## Tests

```bash
cargo test -p lmprobe-lang-swift
```

The smoke harness asserts every struct is constructible. Add fixture-based assertions in `tests/smoke.rs` as you implement the trait surface.

## Fixtures

`fixtures/` holds small in-tree swift files used by smoke tests. Keep them ≤ 5 files, ≤ 30 lines each — the deep-tier corpus lives at `$LMPROBE_QA_CORPUS/swift/` (workspace-external).
