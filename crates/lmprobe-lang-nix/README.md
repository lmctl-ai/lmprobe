# `lmprobe-lang-nix`

Per-language D5 capability impls for **nix**.

This crate is part of the [lmprobe](https://github.com/lmctl-ai/lmprobe) workspace. It depends on:

- `lmprobe-lang-core` — D5 trait definitions + `SgLang` re-export.
- `lmprobe-types` — D4 envelope shapes (transitively).
- `ast-grep-core` — pattern engine.

It does **not** depend on any other `lmprobe-lang-*` crate.

## Surface

The crate exports 8 unit structs, one per D5 trait:

| Struct | Trait | Purpose |
|--------|-------|---------|
| `NixExtractor` | `NameExtractor` | Per-declaration symbol-name extraction. |
| `NixCallGrammar` | `CallGrammar` | Call-site patterns for `callers` / `callees`. |
| `NixImportGrammar` | `ImportGrammar` | Import-statement patterns. |
| `NixInheritanceGrammar` | `InheritanceGrammar` | Supertype / subtype patterns. |
| `NixDecorationGrammar` | `DecorationGrammar` | Decorator / attribute patterns. |
| `NixTraceTemplate` | `TraceTemplate` | Default `trace` instrumentation template. |
| `NixKindTaxonomy` | `KindTaxonomy` | Portable-kind ↔ tree-sitter-kind mapping. |
| `NixTopLevelKinds` | `TopLevelKinds` | (Data-langs only) child-kind enumeration. |

The `lmprobe-registry` crate's per-language dispatch arms are auto-wired by `cargo xtask new-lang nix`.

## Tests

```bash
cargo test -p lmprobe-lang-nix
```

The smoke harness asserts every struct is constructible. Add fixture-based assertions in `tests/smoke.rs` as you implement the trait surface.

## Fixtures

`fixtures/` holds small in-tree nix files used by smoke tests. Keep them ≤ 5 files, ≤ 30 lines each — the deep-tier corpus lives at `~/search/qa1/repos/nix/` (workspace-external).
