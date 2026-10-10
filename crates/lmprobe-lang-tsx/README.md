# `lmprobe-lang-tsx`

Per-language D5 capability impls for **tsx**.

This crate is part of the [lmprobe](https://github.com/lmctl-ai/lmprobe) workspace. It depends on:

- `lmprobe-lang-core` — D5 trait definitions + `SgLang` re-export.
- `lmprobe-types` — D4 envelope shapes (transitively).
- `ast-grep-core` — pattern engine.

It does **not** depend on any other `lmprobe-lang-*` crate.

## Surface

The crate exports 8 unit structs, one per D5 trait:

| Struct | Trait | Purpose |
|--------|-------|---------|
| `TsxExtractor` | `NameExtractor` | Per-declaration symbol-name extraction. |
| `TsxCallGrammar` | `CallGrammar` | Call-site patterns for `callers` / `callees`. |
| `TsxImportGrammar` | `ImportGrammar` | Import-statement patterns. |
| `TsxInheritanceGrammar` | `InheritanceGrammar` | Supertype / subtype patterns. |
| `TsxDecorationGrammar` | `DecorationGrammar` | Decorator / attribute patterns. |
| `TsxTraceTemplate` | `TraceTemplate` | Default `trace` instrumentation template. |
| `TsxKindTaxonomy` | `KindTaxonomy` | Portable-kind ↔ tree-sitter-kind mapping. |
| `TsxTopLevelKinds` | `TopLevelKinds` | (Data-langs only) child-kind enumeration. |

The `lmprobe-registry` crate's per-language dispatch arms are auto-wired by `cargo xtask new-lang tsx`.

## Tests

```bash
cargo test -p lmprobe-lang-tsx
```

The smoke harness asserts every struct is constructible. Add fixture-based assertions in `tests/smoke.rs` as you implement the trait surface.

## Fixtures

`fixtures/` holds small in-tree tsx files used by smoke tests. Keep them ≤ 5 files, ≤ 30 lines each — the deep-tier corpus lives at `$LMPROBE_QA_CORPUS/tsx/` (workspace-external).
