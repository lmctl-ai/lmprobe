# `lmprobe-lang-bash`

Per-language D5 capability impls for **bash**.

This crate is part of the [lmprobe](https://github.com/lmctl-ai/lmprobe) workspace. It depends on:

- `lmprobe-lang-core` — D5 trait definitions + `SgLang` re-export.
- `lmprobe-types` — D4 envelope shapes (transitively).
- `ast-grep-core` — pattern engine.

It does **not** depend on any other `lmprobe-lang-*` crate.

## Surface

The crate exports 8 unit structs, one per D5 trait:

| Struct | Trait | Purpose |
|--------|-------|---------|
| `BashExtractor` | `NameExtractor` | Per-declaration symbol-name extraction. |
| `BashCallGrammar` | `CallGrammar` | Call-site patterns for `callers` / `callees`. |
| `BashImportGrammar` | `ImportGrammar` | Import-statement patterns. |
| `BashInheritanceGrammar` | `InheritanceGrammar` | Supertype / subtype patterns. |
| `BashDecorationGrammar` | `DecorationGrammar` | Decorator / attribute patterns. |
| `BashTraceTemplate` | `TraceTemplate` | Default `trace` instrumentation template. |
| `BashKindTaxonomy` | `KindTaxonomy` | Portable-kind ↔ tree-sitter-kind mapping. |
| `BashTopLevelKinds` | `TopLevelKinds` | (Data-langs only) child-kind enumeration. |

The `lmprobe-registry` crate's per-language dispatch arms are auto-wired by `cargo xtask new-lang bash`.

## Tests

```bash
cargo test -p lmprobe-lang-bash
```

The smoke harness asserts every struct is constructible. Add fixture-based assertions in `tests/smoke.rs` as you implement the trait surface.

## Fixtures

`fixtures/` holds small in-tree bash files used by smoke tests. Keep them ≤ 5 files, ≤ 30 lines each — the deep-tier corpus lives at `~/search/qa1/repos/bash/` (workspace-external).
