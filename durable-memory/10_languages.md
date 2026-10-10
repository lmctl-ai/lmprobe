# 10 — Languages

lmprobe recognises 29 languages via `ast_grep_language::SupportLang`. They split into two tiers for verdict-tracking and `--has-child` semantics.

## Tier — Code vs Data

The `LangTier` enum in `crates/lmprobe/src/langs.rs` partitions:

- **Code** (23 langs): Bash, C, C++, C#, Dart, Elixir, Go, Haskell, HCL, Java, JavaScript, Kotlin, Lua, Nix, PHP, Python, Ruby, Rust, Scala, Solidity, Swift, TSX, TypeScript.
- **Data** (6 langs): CSS, HTML, JSON, TOML, XML, YAML.

The tier drives:
- `--has-child` filter: code-tier matches descendants whose kind contains `method`/`function`/`field`/`declaration`; data-tier matches `TopLevelKinds::child_kinds()` per language.
- Verdict criteria (deep-tier test): code-tier scores over `{def, ref, search --name, callers, imports}`; data-tier scores over `{search --name, search --kind class, --has-child, --outline}`. See [16_testing.md](16_testing.md).

## Capability traits

Each language opts into the traits it can support. The trait hierarchy and per-language coverage is in [11_extending.md](11_extending.md). At a glance:

| Trait | Coverage |
|-------|----------|
| `NameExtractor` | All 29 (per-lang or `GenericExtractor`). |
| `KindTaxonomy` | All 29; degree of coverage varies. |
| `CallGrammar` | 10 (StandardCallGrammar): Rust, Java, JS, TS, Tsx, Python, Go, C, C++, C#. |
| `ImportGrammar` | 17: Rust, Java, JS, TS, Tsx, Python, Go, Ruby, Nix, Lua, PHP, Swift, Scala, Solidity, Kotlin, Elixir, HCL, Haskell, Bash. |
| `DecorationGrammar` | 8: Rust, Java, JS, TS, Tsx, Python, Kotlin, PHP, Swift. |
| `TraceTemplate` | 6: Rust, JS, TS, Tsx, Python, Java, Go. |
| `InheritanceGrammar` | 12 (super/sub): Rust, Java, Kotlin, Scala, C++, C#, JS, TS, Tsx, Python, Ruby, PHP, Swift. 7 (override): Java, Kotlin, Scala, C#, C++, Swift, JS-TS-with-`override`. |
| `TopLevelKinds` | 6 data tiers. |

A verb returning empty Vec for a language emits `NOT_IMPLEMENTED` and skips that file. See [14_diagnostics.md](14_diagnostics.md).

## LANG_VERDICTS — coverage matrix

The deep tier (`tests/deep.rs`) measures every language's verdict by running tier-appropriate probes against a fixture and counting non-zero hit results. As of the current baseline:

| Lang | Tier | Verdict | Notes |
|------|------|---------|-------|
| bash | Code | Ok | |
| c | Code | Ok | |
| cpp | Code | Ok | |
| csharp | Code | Ok | |
| css | Data | Ok | |
| dart | Code | Ok | (Dart `class_definition` was a 2C plan-correction → `class_declaration`.) |
| elixir | Code | Partial | `def Plug` finds the module; `ref/callers/imports` don't apply for Elixir's `use Plug` macro syntax. |
| go | Code | Ok | |
| haskell | Code | Partial | Probe `main` is the entry point; `ref/callers/imports` can't fire by definition. |
| hcl | Code | Ok | |
| html | Data | Ok | |
| java | Code | Ok | |
| javascript | Code | Ok | |
| json | Data | Ok | |
| kotlin | Code | Ok | |
| lua | Code | Ok | |
| nix | Code | Ok | |
| php | Code | Ok | |
| python | Code | Ok | |
| ruby | Code | Ok | |
| rust | Code | Ok | |
| scala | Code | Ok | |
| solidity | Code | Ok | |
| swift | Code | Ok | |
| toml | Data | Ok | (Routed to in-tree fixture; qa1 corpus is too thin.) |
| tsx | Code | Ok | (4D's `JsExtractor` for `lexical_declaration` arrow-const exports.) |
| typescript | Code | Partial | qa1 fixture `z` is re-exported from external.ts; better symbol pending. |
| xml | Data | Ok | |
| yaml | Data | Ok | |

Distribution: **27 Ok / 2 Partial (elixir, haskell) / 0 Fail** across 29 languages.

The Partial budget is ≤4 per the Phase 2 exit criterion; we ship at 2.

## Per-language quirks

### Rust

- `KindTaxonomy::Class` maps to `struct_item`; `Trait` to `trait_item`; `Enum` to `enum_item`. There is no portable `class` in Rust.
- `derive_macro_invocation` does NOT exist in tree-sitter-rust. `#[derive(Debug)]` parses as `attribute_item` (sibling of declarations, not a descendant). Use `--kind struct --has 'kind: attribute_item, regex: "clap"'` to find structs deriving from clap.
- `DecorationGrammar` covers the full attribute chain: outer `#[name]`, inner `#![name]`, called `#[name(...)]`, and the `#[derive(..., NAME, ...)]` cascade so callers can find a specific derived trait.
- `subtype_patterns(name)` returns `impl name for $T { ... }` shapes — Rust's analogue to subclasses.
- `override_patterns(name)` returns method shapes; Rust has no `override` keyword.

### TypeScript / JavaScript / TSX

- `JsExtractor` handles `export const Foo = <arrow_function>` correctly: walks `lexical_declaration → variable_declarator.name`. Pre-Phase-2C this returned the keyword `let`/`const`.
- TS 4.3+ `override` keyword is recognised by `JsInheritanceGrammar`.
- React JSX/TSX: tree-sitter-tsx is used for `.tsx`, tree-sitter-typescript for `.ts`.

### Python

- `Class` portable kind covers `class_definition`. `Function` covers `function_definition`.
- `DecorationGrammar` includes dotted decorators: `@app.route(...)` matches via `@$X.{name}($$$)`.
- `OverrideGrammar` returns empty Vec — Python has no `override` keyword. `override <name>` returns `NOT_IMPLEMENTED`.

### Lua

- `LuaExtractor` handles `function M.setup(opts)`: rsplits on `.` / `:` to surface the trailing component (`setup`).
- `init.lua` is a `discover` marker; one project per init.lua (root + nested).

### Data languages (JSON / YAML / TOML / XML / HTML / CSS)

- `KindTaxonomy::Class` maps to the **container** kind: object/block_mapping/table/element/element/rule_set respectively. `--kind class` is the portable way to select containers.
- `TopLevelKinds::child_kinds()` is what `--has-child` matches as a "child" entry.
- Per-language NameExtractors (TomlExtractor, JsonExtractor, etc.) walk to the proper key/tag/attribute identifier — the generic `field("name")` fallback returns punctuation (`[`, `<`, `"`).

### Dart

- `class_declaration` is the actual tree-sitter kind (NOT `class_definition` as some docs suggest). Plan-correction in 2C.

### Elixir

- Uses `call`-as-everything: `defmodule Plug do ... end` parses as a `call`. Per-call-target disambiguation in `ElixirExtractor`.
- `ImportGrammar` emits the four module-introduction macros: `import`, `alias`, `use`, `require`.

### HCL (Terraform)

- `block` (resource/data/module) maps to portable `Struct`. `versions.tf` is the `discover` marker.
- Function call shapes (`length(...)`, `lookup(...)`) are not modelled; out of scope for code-tier verdict.

### Solidity

- `contract_declaration` → `Class`; `interface_declaration` → `Interface`; `library_declaration` → `Trait` (pragmatic mapping); `modifier_definition` → `Macro`.

### Bash

- `function_declaration` → portable `Function`. `source <file>` and `. <file>` shapes both recognised by `ImportGrammar`.

## Fixture sources

The qa1 corpus at `$LMPROBE_QA_CORPUS/<lang>/<repo>/` provides one canonical repo per language. Two languages were re-routed to in-tree fixtures (`crates/lmprobe/tests/fixtures/<lang>/`) because the qa1 fixture was too thin:

Operationally, the harness uses a two-track fixture strategy:
- **In-repo fixtures**: compact, curated samples for smoke/regression checks and trait-level behavior.
- **qa1 corpus fixtures**: broader real-repo signal for deep language validation.

This is why deep rows primarily assert against external fixtures, while in-tree fixtures mainly support fast and stable smoke checks plus gap-filling when corpus coverage is weak.

| Lang | qa1 fixture | Re-routed to | Why |
|------|-------------|--------------|-----|
| toml | `/qa1/repos/toml/toml/` | `crates/lmprobe/tests/fixtures/toml/` | Spec repo; no substantive `.toml` content. |
| lua | `/qa1/repos/lua/kickstart.nvim/` | `crates/lmprobe/tests/fixtures/lua/` | Neovim config; `setup` only as nested call. |

In-tree fixtures live under `crates/lmprobe/tests/fixtures/<lang>/` for 14 languages. They're idiomatic snippets ≤15 lines each — minimal but substantive enough to exercise the relevant ImportGrammar / DecorationGrammar / NameExtractor.

## How to add a new language

See [11_extending.md](11_extending.md) § Adding a language. The short version: ensure `ast_grep_language::SupportLang` knows the language; wire `KindTaxonomy`, `NameExtractor` if generic isn't enough, and the relevant capability traits; add an in-tree fixture; add a verdict row to `LANG_VERDICTS` in `tests/deep.rs`.

## Read next

- [11_extending.md](11_extending.md) for the D5 capability traits and registration pattern.
- [16_testing.md](16_testing.md) for the verdict criteria + how to interpret Ok/Partial/Fail.
