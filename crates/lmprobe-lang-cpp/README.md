# `lmprobe-lang-cpp`

Per-language D5 capability impls for **cpp**.

This crate is part of the [lmprobe](https://github.com/lmctl-ai/lmprobe) workspace. It depends on:

- `lmprobe-lang-core` — D5 trait definitions + `SgLang` re-export.
- `lmprobe-types` — D4 envelope shapes (transitively).
- `ast-grep-core` — pattern engine.

It does **not** depend on any other `lmprobe-lang-*` crate.

## Surface

The crate exports 8 unit structs, one per D5 trait:

| Struct | Trait | Purpose |
|--------|-------|---------|
| `CppExtractor` | `NameExtractor` | Per-declaration symbol-name extraction. |
| `CppCallGrammar` | `CallGrammar` | Call-site patterns for `callers` / `callees`. |
| `CppImportGrammar` | `ImportGrammar` | Import-statement patterns. |
| `CppInheritanceGrammar` | `InheritanceGrammar` | Supertype / subtype patterns. |
| `CppDecorationGrammar` | `DecorationGrammar` | Decorator / attribute patterns. |
| `CppTraceTemplate` | `TraceTemplate` | Default `trace` instrumentation template. |
| `CppKindTaxonomy` | `KindTaxonomy` | Portable-kind ↔ tree-sitter-kind mapping. |
| `CppTopLevelKinds` | `TopLevelKinds` | (Data-langs only) child-kind enumeration. |

The `lmprobe-registry` crate's per-language dispatch arms are auto-wired by `cargo xtask new-lang cpp`.

## Tests

```bash
cargo test -p lmprobe-lang-cpp
```

The smoke harness asserts every struct is constructible. Add fixture-based assertions in `tests/smoke.rs` as you implement the trait surface.

## Fixtures

`fixtures/` holds small in-tree cpp files used by smoke tests. Keep them ≤ 5 files, ≤ 30 lines each — the deep-tier corpus lives at `~/search/qa1/repos/cpp/` (workspace-external).
