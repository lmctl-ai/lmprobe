# `lmprobe-lang-html`

Per-language D5 capability impls for **html**.

This crate is part of the [lmprobe](https://github.com/lmctl-ai/lmprobe) workspace. It depends on:

- `lmprobe-lang-core` — D5 trait definitions + `SgLang` re-export.
- `lmprobe-types` — D4 envelope shapes (transitively).
- `ast-grep-core` — pattern engine.

It does **not** depend on any other `lmprobe-lang-*` crate.

## Surface

The crate exports 8 unit structs, one per D5 trait:

| Struct | Trait | Purpose |
|--------|-------|---------|
| `HtmlExtractor` | `NameExtractor` | Per-declaration symbol-name extraction. |
| `HtmlCallGrammar` | `CallGrammar` | Call-site patterns for `callers` / `callees`. |
| `HtmlImportGrammar` | `ImportGrammar` | Import-statement patterns. |
| `HtmlInheritanceGrammar` | `InheritanceGrammar` | Supertype / subtype patterns. |
| `HtmlDecorationGrammar` | `DecorationGrammar` | Decorator / attribute patterns. |
| `HtmlTraceTemplate` | `TraceTemplate` | Default `trace` instrumentation template. |
| `HtmlKindTaxonomy` | `KindTaxonomy` | Portable-kind ↔ tree-sitter-kind mapping. |
| `HtmlTopLevelKinds` | `TopLevelKinds` | (Data-langs only) child-kind enumeration. |

The `lmprobe-registry` crate's per-language dispatch arms are auto-wired by `cargo xtask new-lang html`.

## Tests

```bash
cargo test -p lmprobe-lang-html
```

The smoke harness asserts every struct is constructible. Add fixture-based assertions in `tests/smoke.rs` as you implement the trait surface.

## Fixtures

`fixtures/` holds small in-tree html files used by smoke tests. Keep them ≤ 5 files, ≤ 30 lines each — the deep-tier corpus lives at `~/search/qa1/repos/html/` (workspace-external).
