# `lmprobe-lang-haskell`

Per-language D5 capability impls for **haskell**.

This crate is part of the [lmprobe](https://github.com/lmctl-ai/lmprobe) workspace. It depends on:

- `lmprobe-lang-core` — D5 trait definitions + `SgLang` re-export.
- `lmprobe-types` — D4 envelope shapes (transitively).
- `ast-grep-core` — pattern engine.

It does **not** depend on any other `lmprobe-lang-*` crate.

## Surface

The crate exports 8 unit structs, one per D5 trait:

| Struct | Trait | Purpose |
|--------|-------|---------|
| `HaskellExtractor` | `NameExtractor` | Per-declaration symbol-name extraction. |
| `HaskellCallGrammar` | `CallGrammar` | Call-site patterns for `callers` / `callees`. |
| `HaskellImportGrammar` | `ImportGrammar` | Import-statement patterns. |
| `HaskellInheritanceGrammar` | `InheritanceGrammar` | Supertype / subtype patterns. |
| `HaskellDecorationGrammar` | `DecorationGrammar` | Decorator / attribute patterns. |
| `HaskellTraceTemplate` | `TraceTemplate` | Default `trace` instrumentation template. |
| `HaskellKindTaxonomy` | `KindTaxonomy` | Portable-kind ↔ tree-sitter-kind mapping. |
| `HaskellTopLevelKinds` | `TopLevelKinds` | (Data-langs only) child-kind enumeration. |

The `lmprobe-registry` crate's per-language dispatch arms are auto-wired by `cargo xtask new-lang haskell`.

## Tests

```bash
cargo test -p lmprobe-lang-haskell
```

The smoke harness asserts every struct is constructible. Add fixture-based assertions in `tests/smoke.rs` as you implement the trait surface.

## Fixtures

`fixtures/` holds small in-tree haskell files used by smoke tests. Keep them ≤ 5 files, ≤ 30 lines each — the deep-tier corpus lives at `~/search/qa1/repos/haskell/` (workspace-external).
