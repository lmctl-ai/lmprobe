# `lmprobe-lang-elixir`

Per-language D5 capability impls for **elixir**.

This crate is part of the [lmprobe](https://github.com/lmctl-ai/lmprobe) workspace. It depends on:

- `lmprobe-lang-core` — D5 trait definitions + `SgLang` re-export.
- `lmprobe-types` — D4 envelope shapes (transitively).
- `ast-grep-core` — pattern engine.

It does **not** depend on any other `lmprobe-lang-*` crate.

## Surface

The crate exports 8 unit structs, one per D5 trait:

| Struct | Trait | Purpose |
|--------|-------|---------|
| `ElixirExtractor` | `NameExtractor` | Per-declaration symbol-name extraction. |
| `ElixirCallGrammar` | `CallGrammar` | Call-site patterns for `callers` / `callees`. |
| `ElixirImportGrammar` | `ImportGrammar` | Import-statement patterns. |
| `ElixirInheritanceGrammar` | `InheritanceGrammar` | Supertype / subtype patterns. |
| `ElixirDecorationGrammar` | `DecorationGrammar` | Decorator / attribute patterns. |
| `ElixirTraceTemplate` | `TraceTemplate` | Default `trace` instrumentation template. |
| `ElixirKindTaxonomy` | `KindTaxonomy` | Portable-kind ↔ tree-sitter-kind mapping. |
| `ElixirTopLevelKinds` | `TopLevelKinds` | (Data-langs only) child-kind enumeration. |

The `lmprobe-registry` crate's per-language dispatch arms are auto-wired by `cargo xtask new-lang elixir`.

## Tests

```bash
cargo test -p lmprobe-lang-elixir
```

The smoke harness asserts every struct is constructible. Add fixture-based assertions in `tests/smoke.rs` as you implement the trait surface.

## Fixtures

`fixtures/` holds small in-tree elixir files used by smoke tests. Keep them ≤ 5 files, ≤ 30 lines each — the deep-tier corpus lives at `$LMPROBE_QA_CORPUS/elixir/` (workspace-external).
