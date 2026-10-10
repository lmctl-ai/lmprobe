# 04 — Graph-shaped sensors

Verbs in this chapter: `callers`, `callsites`, `callees`, `imports`, `exports`, `decorations`, `supertypes`, `subtypes`, `overrides`, `impact`, `context`. They produce relational edges or call-adjacent evidence around a queried symbol.

The first eight share `Vec<SymbolHit>` envelopes. `impact` ships a DAG envelope (`nodes` + `edges`). `context` ships a composite bundle.

Graph verbs are currently **call/symbol graph oriented**. They capture call targets,
invocation contexts, decorators, inheritance, imports, and definition adjacency.
They do not currently model framework wiring edges such as Angular DI/provider
registration, Angular module declaration/import topology, or Spring injection
stereotype/autowire edges.

Global flags from [02_invocation.md](02_invocation.md) apply. Schema details in [08_envelope_schema.md](08_envelope_schema.md).

## callers

Find sites that invoke or reference a symbol as a callable target.

The `callers` verb returns every site where `<symbol>` is used as a callable or referent target. The exact shapes recognized depend on the language's grammar; the doctrine is uniform: **if the language treats this site as a use of the symbol-as-callable, lmprobe returns it.**

Recognized shapes:

- **Function calls**: `foo(args)` — universal.
- **Method calls**: `obj.foo(args)`, `obj->foo(args)`, `obj::foo(args)` — OO and Cpp/Rust scoped.
- **Constructor invocations**: `new Foo(args)` (C#/Cpp/Java/Kotlin/Swift); `Foo(args)` (Scala, Kotlin no-new sites).
- **JSX/Tsx component sites**: `<Foo />`, `<Foo prop="x" />`, `<Foo>{children}</Foo>` — Tsx and JS-with-JSX. Native HTML elements (`<div>`, `<span>`, etc.) are NOT callers.
- **XML element references**: `<Foo>...</Foo>` element-name match; text content of designated reference elements (POM `<artifactId>`, Spring `<bean class>`, etc.) where the schema treats the text as a callable reference.
- **YAML directive references**: values of `uses`, `extends`, `actions`, `template`, `import` keys.
- **Function-applications** (ML-family / Haskell / Elixir): `Foo x y` head-position application.
- **Message-sends** (Ruby, Smalltalk-style): `obj foo` or `obj.foo` without parens where the language treats this as invocation.
- **Higher-order argument-passing**: `xs.map(foo)` — passing `foo` as a callable into a higher-order context counts as a caller-site.

Excluded (not callers):

- Type annotations: `let x: Foo = ...` — Foo is referenced as a type.
- Pure data references: `Foo` in strings, comments, or literal data structures.
- Re-export / re-import: `import { Foo }` is an `imports` hit, not `callers`.
- HTML native elements (capitalization rule).

For languages with no call-syntax grammar (TOML, JSON pure-data), `callers` returns `NOT_IMPLEMENTED` with a diagnostic.

The same doctrine is enforced identically across the CLI verb and GraphQL `Query.callers` resolver. See [03_sensors_search.md](03_sensors_search.md#ref) for the distinction between `ref` (any identifier mention) and `callers` (invocation / reference-as-callable).

```bash
lmprobe callers Pattern .
lmprobe callers Pattern --tests-only .
lmprobe --format json callers blame_with_follow_semantic crates/lmprobe/src
```

Envelope: `lmprobe.callers.v1` → `CallersResult { hits: Vec<SymbolHit> }`.

Each hit's `enclosing_decl` is the **caller's** own decl name (function/method/etc.) — added in Phase 4D. `impact` upstream BFS reads this field; downstream consumers can use it to attribute call sites without parsing the file again.

`--tests-only` keeps only caller hits from likely test paths. This is path-based targeting evidence, not coverage proof. The heuristic recognizes common forms such as `tests/**`, `test_*.py`, `*_test.py`, `__tests__/**`, `*.test.*`, `*.spec.*`, and `*_test.go`.

Coverage via `CallGrammar` registry:
- **Function-call + method-call**: Rust / Java / JS / TS / Tsx / Python / Go / C / C++ / C# (`StandardCallGrammar`).
- **Phase 6D landings track this section** — per-language extensions for constructor, JSX, XML element-ref, YAML directive, function-application, and message-send shapes are added per-crate.

Languages whose `CallGrammar` returns no patterns emit `NOT_IMPLEMENTED` and skip files in that language.

## Framework DI metadata gap

When framework wiring is the evidence target, `callers`/`callees`/`impact`/`context`
may return empty or incomplete results because those directives are not modeled as
call edges yet. Use symbol/line fallback recipes to collect evidence in the
interim:

```bash
# Symbol mentions across call sites, registrations, and type references.
lmprobe ref SessionSyncInterceptor src

# Structural line evidence inside Angular / Spring metadata blocks.
lmprobe --format json grep --parents --body 'SessionSyncInterceptor' src
lmprobe --format json grep -C 2 --parents 'providers\s*:' src/app.module.ts
```

The fallback does not change wire semantics: it is an alternative evidence path for
framework-heavy code paths until framework-specific wiring extractors are added.

## callsites

Python-first mechanical callsite argument extraction. This is evidence-only: it parses AST call nodes whose final callee segment matches `<callee>` and preserves argument source text, including common multiline calls. It does not claim overload resolution, dynamic dispatch, or that a method call reaches a specific definition.

```bash
lmprobe callsites foo --args --around 1 src/
lmprobe --format json callsites foo --args file.py
```

Envelope: `lmprobe.callsites.v1` -> `CallsitesResult { callsites: Vec<CallsiteArgsHit> }`.

Each hit includes `file_path`, `line_number`, `callee_text`, `positional_args`, `keyword_args { name, value }`, and optional `source_context` when `--around N` is set. Markdown output renders the call summary plus line-numbered source context when `--around N` is requested. The first slice handles Python function calls like `foo(a, b, key=value)`, multiline calls like `Form(default=None, ...)`, and method calls like `obj.foo(x, flag=True)`. Non-Python files produce no callsite hits.

## callees

Symmetric to `callers`. Finds the call-sites *inside* `name`'s body. Locates `name`'s declaration via `CallGrammar::callee_decl_patterns(name)`, walks the body's AST descendants for call-site nodes, returns one hit per call.

```bash
lmprobe callees run .
lmprobe --format json callees fix_apply crates/lmprobe/src/fix_sensor.rs
```

Envelope: `lmprobe.callees.v1` → `CalleesResult { hits: Vec<SymbolHit> }`. `name` on each hit is the called identifier (extracted from `text.split('(').next()` heuristic).

Coverage: same 10 langs as callers.

## imports

Find import / use / require declarations of a module name.

```bash
lmprobe imports clap .                         # Rust `use clap;` etc.
lmprobe imports react .                        # JS/TS `import … from "react"`.
lmprobe imports kotlinx.coroutines.flow.Flow . # Kotlin
```

Envelope: `lmprobe.imports.v1` → `ImportsResult { hits: Vec<SymbolHit> }`.

Coverage via `ImportGrammar` registry: 17 langs. Quoted module names (`"fmt"`) are stripped and emit `QUOTED_INPUT` for transparency.

## exports

Symmetric to `imports`. Find re-exports of a name: `pub use NAME;` (Rust), `export { NAME }` (JS/TS), `__all__ = [..., "NAME"]` (Python), `func NAME(...)` capitalized (Go), etc.

```bash
lmprobe exports Pattern .
lmprobe --format json exports useState .
```

Envelope: `lmprobe.exports.v1` → `ExportsResult { hits: Vec<SymbolHit> }`.

Coverage: 9 langs (Rust/JS/TS/Tsx/Python/Go/Scala/Java/Kotlin). Other langs emit `NOT_IMPLEMENTED`.

## decorations

Find usages of an annotation / attribute / decorator.

```bash
lmprobe decorations derive .                      # Rust #[derive(Debug)] etc.
lmprobe decorations Override -l java .            # @Override
lmprobe decorations Inject .                      # Kotlin @Inject / @field:Inject
lmprobe decorations route -l python .             # @app.route / @bp.route
lmprobe decorations 'Route' -l php .              # PHP 8 #[Route(...)]
```

Envelope: `lmprobe.decorations.v1` → `DecorationsResult { hits: Vec<SymbolHit> }`.

Coverage via `DecorationGrammar` registry: 8 langs (Rust full attribute chain incl. `#[derive(...)]` cascade; Java/Kotlin/Python/JS/TS/Tsx `@`-prefixed; Kotlin use-site targets `@field:` / `@param:`; PHP-8 `#[Attr]`; Swift `@available` / `@MainActor` / `@objc`).

## supertypes / subtypes

Find a class/trait's parents (`supertypes`) or its subclasses/implementers (`subtypes`).

```bash
lmprobe supertypes UserRepository .   # `interface UserRepository : SomeBase` etc.
lmprobe subtypes Matcher .            # `impl Matcher for X` (Rust)
lmprobe subtypes Component -l tsx .   # `class X extends Component` (TS/Tsx)
```

Envelopes: `lmprobe.supertypes.v1` / `lmprobe.subtypes.v1`, both `Vec<SymbolHit>`.

Coverage via `InheritanceGrammar`: 11 langs for super/sub-type (Rust traits, Java, Kotlin, Scala, C++, C#, JS/TS/Tsx, Python, Ruby, PHP, Swift). Others emit `NOT_IMPLEMENTED`.

## overrides

Find method overrides — declarations marked `@Override` (Java), `override fun` (Kotlin), `override def` (Scala), `override $RET name(...)` (C# / C++), `override func` (Swift), TS-4.3+ `override` keyword. Rust doesn't have an override keyword; the verb returns 0 hits (the trait-impl methods are findable via `subtypes <Trait>` instead).

```bash
lmprobe overrides matches -l java .
lmprobe overrides equals -l kotlin .
```

Envelope: `lmprobe.overrides.v1` → `OverridesResult { hits: Vec<SymbolHit> }`.

Coverage: 7 langs (Java/Kotlin/Scala/C#/C++/Swift/JS-TS-with-`override`).

## impact

Recursive callers/callees BFS with depth-banded confidence. `lmprobe.impact.v1` envelope shapes a directed acyclic graph.

| Flag | Default | Purpose |
|------|---------|---------|
| `--direction {upstream\|downstream}` | `upstream` | `upstream` = "who calls me" (recurse callers). `downstream` = "what do I call" (recurse callees). |
| `--depth <N>` | 3 | Max BFS depth. Hard cap 10; requesting >10 emits `IMPACT_DEPTH_CAPPED` and clamps. |
| `--tests-only` | off | Keep only impact nodes whose file path looks test/spec-scoped. |
| `--exclude-tests` | off | Drop impact nodes whose file path looks test/spec-scoped. Mutually exclusive with `--tests-only`. |

Confidence band per node:
- depth 1 → `high`
- depth 2-5 → `medium`
- depth >5 → `low`

Envelope:
```rust
ImpactResult {
    schema, origin, direction, max_depth: u32,
    nodes: Vec<ImpactNode { name, kind, file_path, line_start, depth, confidence }>,
    edges: Vec<ImpactEdge { from, to, edge_type }>,
    warnings, exit_status,
}
```

```bash
# Markdown DAG outline.
lmprobe impact Pattern --depth 3 .

# JSON for tooling.
lmprobe --format json impact Pattern --depth 3 . | jq '.nodes | length'

# Focus on production callers only.
lmprobe impact Pattern --depth 1 --exclude-tests .

# Mermaid for PR comments.
lmprobe --format mermaid impact Pattern --depth 3 .
```

Phase 4D wired faithful caller-name extraction via `enclosing_decl` on `SymbolHit`; `impact upstream` reads that field instead of parsing the line back to find the surrounding `fn`.

See [07_output_formats.md](07_output_formats.md) for Mermaid / DOT rendering of the DAG.

## context

One-shot composite bundle for a symbol. Composes:
- `definition` — first match of `def NAME` (the symbol's own declaration).
- `callers` — 1-hop callers (function-shaped symbols only).
- `callees` — 1-hop callees (function-shaped symbols only).
- `imports` — file-level import lines from the def-containing file.
- `recent_history` — last 3 entries from `history blame NAME`.
- `todos_inline` — TODO/FIXME/XXX/HACK markers found in the def text.
- `file_outline` — body-elided outline of the def-containing file.

For struct/class/enum-shaped symbols, callers/callees are skipped (calls don't apply); the bundle still ships the other components.

| Flag | Default | Purpose |
|------|---------|---------|
| `--def` | on | Definition-anchor mode. This is the default and never falls back to usages. |
| `--usage` | off | Usage/reference-anchor mode. Returns `usages: Vec<GrepHit>` instead of normal definition context. |
| `--depth <N>` | 1 | Reserved; currently 1-hop only. |
| `--import-aware` | off | If `PATH` is a file and the symbol is not declared there, follow one explicit relative import/re-export hop before returning `SYMBOL_NOT_FOUND`. |
| `--recent-history-limit <N>` | 3 | Cap the `recent_history` sub-list. `N` must be at least 1. |
| `--around <N>` | off | Add `source_context: Vec<LineContext>` around the resolved anchor. For `SYMBOL PATH`, the anchor is the definition line; for `PATH:LINE`, it is the requested line. |
| `--parents` | off | In `--usage` mode, attach enclosing structural parent metadata to usage hits. |
| `--body` | off | In `--usage` mode, attach the innermost enclosing declaration/block body to usage hits. |

Envelope: `lmprobe.context.v1` → `ContextResult { definition, callers, callees, imports, recent_history, todos_inline, file_outline, usages?, source_context?, ... }`. All sub-vectors v1-additive (`#[serde(default)]`).

```bash
lmprobe context Pattern .
lmprobe context crates/lmprobe/tests/fixtures/rust/lib.rs:15
lmprobe --format json context --around 3 Pattern .
lmprobe --format json context --usage uninstall LogbackLoggingSystem.java --around 8 --parents
lmprobe --format json context run . | jq '.callers | length'
```

`context` also accepts `PATH:LINE` as the symbol argument. lmprobe resolves the nearest declaration whose source extent contains that line, emits `CONTEXT_LOCATION_RESOLVED`, then returns the normal bundle for the resolved symbol. This is useful when an issue report, compiler error, stack trace, or `grep` hit gives a location instead of a symbol name.

Definition mode is explicit: if `context SYMBOL PATH` cannot find a local definition, it returns `SYMBOL_NOT_FOUND` and does not silently populate usage context. When possible usages are present, the diagnostic hint suggests `context --usage`.

Performance: 187-220 ms warm-cache on the ast-grep workspace (CC3 target <250ms warm). The `recent_history` component reuses the `blob_signatures` cache from Phase 4A — see [09_cache.md](09_cache.md). If `recent_history` measures over budget, the symbol is likely on a high-churn file with many touching commits; see [15_perf.md](15_perf.md).

## NOT_IMPLEMENTED behavior

When a language's relevant capability trait returns an empty pattern Vec (e.g. `OverrideGrammar::override_patterns` for Python, which has no `override` keyword), the sensor emits a `Diagnostic { code: "NOT_IMPLEMENTED", lang: <Lang>, message: "<verb> not implemented for language" }` once per language and skips files in that language. Empty hits + `exit_status: no_match`.

The verb still runs across other languages cleanly. Filter to one language with `-l <lang>` if you only care about coverage for that lang.

## Read next

- [05_history.md](05_history.md) for `history *` verbs and `ast-diff`.
- [10_languages.md](10_languages.md) for the per-language coverage matrix.
- [14_diagnostics.md](14_diagnostics.md) for diagnostic-code reference.
