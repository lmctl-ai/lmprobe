# 11 — Extending lmprobe (D5 capability traits)

This chapter is the canonical reference for how the language registry composes per-language behaviour, how to add a new language, and how to add a new sensor.

The core pattern (D5 in `crates/lmprobe/ARCHITECTURE.md`): per-language behaviour is composed from **capability traits**, not a monolithic struct. Each trait has a `_for(SgLang)` dispatch function in `lmprobe-registry` returning `&'static dyn Trait`. The `_` arm of each dispatch fn falls through to `lmprobe-lang-core::langs::*_for` for languages that haven't been extracted to a dedicated `lmprobe-lang-<name>` crate yet; the per-language arms (`SgLang::Rust => &lmprobe_lang_rust::RustExtractor`) take precedence when the crate exists. Languages without an impl render as `NOT_IMPLEMENTED` via the trait's default `Default::default() / empty / None` returns.

## Workspace layout (Phase 5A)

```
lmprobe-types/        D4 envelope shapes + scalars (no parser deps)
lmprobe-lang-core/    D5 trait definitions + SgLang re-export
lmprobe-registry/     Per-language dispatch fan-out (xtask-managed)
lmprobe-engine/       Sensors (search, def, callers, history, …)
lmprobe-lang-<name>/  Per-language D5 trait impls (one per language)
lmprobe/              Binary: clap CLI + GraphQL Query/Mutation + output
```

Dependency direction: `binary → engine → registry → {lang-core, all lmprobe-lang-* crates} → types`. No cycles. Each per-language crate depends only on `lmprobe-lang-core` + `lmprobe-types` + its own `tree-sitter-<lang>`; no cross-language deps.

## The eight capability traits

All in `crates/lmprobe-lang-core/src/langs.rs`.

### `NameExtractor`

Per-language identifier extraction from a declaration node. Replaces the generic `field("name") || field("key") || child(0)` fallback with language-specific walks.

```rust
pub trait NameExtractor: Send + Sync {
    fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String>;
}
```

Registry: `name_extractor_for(lang) -> &'static dyn NameExtractor`. Default: `GenericExtractor`.

Bespoke extractors (16 of 29 langs): Kotlin, Ruby, Nix, Elixir, Haskell, HCL, Solidity, Swift, CSS, C, C++, Toml, Json, Yaml, Xml, Html, Lua, JS/TS/Tsx (shared `JsExtractor`).

### `KindTaxonomy`

Maps the 12-variant `PortableKind` enum to per-language tree-sitter literal kinds.

```rust
pub trait KindTaxonomy: Send + Sync {
    fn kind_for(&self, p: PortableKind) -> &'static [&'static str];
    fn all_declaration_kinds(&self) -> Vec<&'static str> { ... }
}

pub enum PortableKind {
    Function, Method, Class, Struct, Interface, Trait,
    Enum, TypeAlias, Const, Macro, Import, Route,
}
```

Declared via `taxonomy!` macro:

```rust
taxonomy!(RustTaxonomy {
    Function  => ["function_item"],
    Method    => ["function_item"],
    Class     => [],
    Struct    => ["struct_item"],
    Interface => ["trait_item"],
    Trait     => ["trait_item"],
    Enum      => ["enum_item"],
    TypeAlias => ["type_item"],
    Const     => ["const_item", "static_item"],
    Macro     => ["macro_definition"],
    Import    => ["use_declaration"],
    Route     => [],
});
```

Empty Vec for a portable kind = "this language doesn't expose that kind." `--kind <portable>` skips that file.

Registry: `taxonomy_for(lang) -> &'static dyn KindTaxonomy`. All 29 langs covered.

### `CallGrammar`

Call-site + declaration patterns.

```rust
pub trait CallGrammar: Send + Sync {
    fn call_patterns(&self, name: &str) -> Vec<String>;        // for `callers`
    fn callee_decl_patterns(&self, name: &str) -> Vec<String>; // for `callees`
}
```

Coverage: 10 langs via `StandardCallGrammar` (Rust/Java/JS/TS/Tsx/Python/Go/C/C++/C#). Empty Vec for others = `NOT_IMPLEMENTED` per [04_sensors_graph.md](04_sensors_graph.md).

### `ImportGrammar`

Import (consumer-side) + export (producer-side) patterns.

```rust
pub trait ImportGrammar: Send + Sync {
    fn import_patterns(&self, name: &str) -> Vec<String>;    // for `imports`
    fn export_patterns(&self, name: &str) -> Vec<String>;    // for `exports`
}
```

Coverage: 17 langs for imports; 9 langs for exports. Empty Vec = `NOT_IMPLEMENTED`.

### `DecorationGrammar`

Annotation / attribute / decorator patterns.

```rust
pub trait DecorationGrammar: Send + Sync {
    fn decoration_patterns(&self, name: &str) -> Vec<String>;
}
```

Coverage: 8 langs. Rust covers the full attribute chain (`#[derive(..., NAME, ...)]` cascade); Kotlin handles use-site targets (`@field:Foo`); PHP-8 `#[Attr]` syntax; Swift `@available` / `@MainActor` / `@objc(...)`.

### `TraceTemplate`

Default debug-print template for `trace`'s instrumentation.

```rust
pub trait TraceTemplate: Send + Sync {
    fn template_for(&self, var: &str) -> Option<String>;
}
```

Coverage: 6 langs (Rust, JS/TS/Tsx, Python, Java, Go). `None` for other langs = `NO_TRACE_TEMPLATE` (caller must pass `--template`).

### `TopLevelKinds`

Data-language structural child kinds for `--has-child` filtering.

```rust
pub trait TopLevelKinds: Send + Sync {
    fn child_kinds(&self) -> &'static [&'static str];
}
```

Coverage: 6 data tiers (JSON/YAML/TOML/XML/HTML/CSS). The companion "container kinds" live in `KindTaxonomy::kind_for(PortableKind::Class)` — users select containers via `--kind class`.

### `InheritanceGrammar`

Super/sub-type + override patterns. Single trait covers all three because the grammars are tightly coupled.

```rust
pub trait InheritanceGrammar: Send + Sync {
    fn supertype_decl_patterns(&self, name: &str) -> Vec<String>;
    fn subtype_patterns(&self, name: &str) -> Vec<String>;
    fn override_patterns(&self, name: &str) -> Vec<String>;
}
```

Coverage: 12 langs for super/sub-type; 7 for override.

## Adding a new language

Phase 5A.6 seeded 28 placeholder `lmprobe-lang-<X>/` directories (each with `.gitkeep`) so one Coder session could start per language; `cargo xtask new-lang <X>` then converted each placeholder into a real crate. The current tree has these migrations completed, and this chapter is now the historical reference for the migration pattern plus the ongoing template for future language additions.

### Per-Coder workflow (Phase 5B model)

For the deep-dive playbook Coder Lead sends to each per-language Coder — 8-step workflow, simple-vs-bespoke trait patterns, Step 6 dead-impl deletion, the 9-gate sweep, troubleshooting catalogue — see [18_phase5b_extraction_playbook.md](18_phase5b_extraction_playbook.md).

For each of the 28 unscaffolded languages, a dedicated Coder session works the crate end-to-end:

1. **Coder Lead** (already done as part of Phase 5A.6): every `crates/lmprobe-lang-<X>/.gitkeep` is checked in; the workspace `[workspace] exclude` list lists all 28.
2. **Per-lang Coder session spawns** with `sessiondir = .../crates/lmprobe-lang-<X>/`. The dir contains only `.gitkeep`.
3. From the workspace root, the Coder runs:
   ```bash
   cargo xtask new-lang <X>
   ```
   xtask detects the seeded empty dir, deletes `.gitkeep`, scaffolds the 5 template files into the now-empty directory, inserts 8 dispatch arms into `crates/lmprobe-registry/src/lib.rs`, appends the `lmprobe-lang-<X>` path-dep to `crates/lmprobe-registry/Cargo.toml`, AND removes `crates/lmprobe-lang-<X>` from the workspace `[workspace] exclude` list. The crate is now a real workspace member.
4. The Coder fills in trait impls in `src/lib.rs`, populates `fixtures/` with ≤5 small in-tree files, writes assertions in `tests/smoke.rs`.
5. Verify locally: `cargo build -p lmprobe-lang-<X>`, `cargo test -p lmprobe-lang-<X>`, `cargo xtask check-lang <X>`, plus the binary's full smoke (`cargo test -p lmprobe --release --test smoke`).
6. Single PR per language. Coder Lead reviews + arbitrates Cargo.lock conflicts.

Concrete steps — adding hypothetical "Zig" (SgLang variant added upstream first; not pre-seeded):

1. **Confirm `ast_grep_language::SupportLang::Zig` exists.** If not, that's an upstream addition first; out of scope for lmprobe.

2. **Scaffold the per-language crate** with xtask:

   ```bash
   cargo xtask new-lang zig
   ```

   xtask handles three pre-existing-dir cases:
   - **Dir doesn't exist** (Zig wasn't pre-seeded — typical for post-baseline additions): xtask creates it + scaffolds.
   - **Dir exists with only `.gitkeep`** (the Phase 5A.6 seeded-empty-dir case for the original 28): xtask removes the placeholder, scaffolds, and removes the dir from the workspace `[workspace] exclude` list.
   - **Dir exists with other files**: xtask refuses with a clear error (avoids clobbering work).

   This generates `crates/lmprobe-lang-zig/` from `xtask/templates/lang/`:

   ```
   crates/lmprobe-lang-zig/
     Cargo.toml          # workspace deps + tree-sitter-zig
     src/lib.rs          # 8 unit structs (ZigExtractor, ZigCallGrammar, ...)
                         # each with a default-empty trait impl
     tests/smoke.rs      # asserts every struct is constructible
     fixtures/           # add small in-tree Zig files here
     README.md           # per-crate quick-reference
   ```

   The xtask also:
   - inserts 8 dispatch arms (`SgLang::Zig => &lmprobe_lang_zig::Zig*`) into `crates/lmprobe-registry/src/lib.rs` (inside the `xtask:per-lang:<fn>:start/:end` marker regions);
   - appends `lmprobe-lang-zig` as a path dep in `crates/lmprobe-registry/Cargo.toml`.

3. **Implement the trait surface in `crates/lmprobe-lang-zig/src/lib.rs`.** Override only the traits Zig needs; defaults cover the rest. Concrete pattern from the Rust extraction (`crates/lmprobe-lang-rust/src/lib.rs`):

   ```rust
   use lmprobe_lang_core::langs::{
       CallGrammar, ImportGrammar, KindTaxonomy, NameExtractor,
       PortableKind, SgNode, StandardCallGrammar,
   };

   pub struct ZigKindTaxonomy;
   impl KindTaxonomy for ZigKindTaxonomy {
       fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
           match p {
               PortableKind::Function => &["FnProto", "FnDecl"],
               PortableKind::Struct   => &["ContainerDecl"],
               PortableKind::Const    => &["VarDecl"],
               PortableKind::Import   => &["BUILTINIDENTIFIER"],
               _ => &[],
           }
       }
   }

   pub struct ZigImportGrammar;
   impl ImportGrammar for ZigImportGrammar {
       fn import_patterns(&self, name: &str) -> Vec<String> {
           vec![format!("@import(\"{}\");", name)]
       }
   }

   // ZigExtractor, ZigCallGrammar, etc. — keep the empty default
   // impls if Zig doesn't need bespoke logic.
   ```

   **W4 trait API constraint**: only `lmprobe-lang-core` traits + `ast-grep-core` types in method signatures. Never `lmprobe-types` envelope refs — keeps async-graphql out of per-language builds.

4. **Add small in-tree fixtures** at `crates/lmprobe-lang-zig/fixtures/` — one or two files, ≤30 lines each, idiomatic Zig. Used by the per-crate smoke test and any future cross-language smoke harness. The deep-tier corpus stays at `~/search/qa1/repos/zig/` (workspace-external), with in-tree fixtures as the smoke tier baseline when external corpus depth is insufficient.

5. **Verify**:

   ```bash
   cargo build -p lmprobe-lang-zig
   cargo test  -p lmprobe-lang-zig          # per-crate smoke
   cargo xtask check-lang zig                # drift-detector lint
   cargo test  -p lmprobe --release --test smoke   # binary's full smoke
   ```

6. **Add a `LANG_VERDICTS` row** in `crates/lmprobe/tests/deep.rs` (29-language table). Run `cargo test -p lmprobe --release --test deep -- --ignored lang_verdicts_match --nocapture` to measure the live verdict; reconcile the table.

7. **Add a smoke golden** if you want a stable regression-pin: `crates/lmprobe/tests/smoke.rs` `cases()` table, then `UPDATE_GOLDENS=1 cargo test -p lmprobe --release --test smoke` to bless.

8. **Build clean** on both feature configs: `cargo build --release` and `cargo build --release --features serve` both return 0 warnings across the whole workspace.

## xtask drift-detector

`cargo xtask check-lang [<name>]` lints every (or one) `lmprobe-lang-*` crate against the template:

- Required files exist: `Cargo.toml`, `src/lib.rs`, `tests/smoke.rs`, `fixtures/`, `README.md`.
- The crate is registered as a path-dep on `lmprobe-registry/Cargo.toml`.

Dispatch-arm presence in `lmprobe-registry/src/lib.rs` is intentionally not asserted: a crate may legitimately have unwired arms during in-flight Coder work. A future `wire-check` task could enforce arm-presence for shipped (non-stub) crates.

Run periodically during 5B parallel-extraction work to catch hand-rolled drift.

## Concurrent xtask invocations (Phase 5B coordination)

Two per-language Coders running `cargo xtask new-lang <A>` and `cargo xtask new-lang <B>` independently in their own sessions both touch three files: `crates/lmprobe-registry/src/lib.rs` (8 marker-region edits each), `crates/lmprobe-registry/Cargo.toml` (one path-dep line each, both anchored after `lmprobe-lang-core = …`), and `Cargo.lock` (variable). The new crate dirs themselves (`crates/lmprobe-lang-<name>/`) are disjoint and don't collide.

When both PRs land, **git's three-way merge produces conflicts** on all three shared files. The conflicts are mechanical to resolve — keep both arms inside each marker region, keep both path-dep lines under the anchor — but they ARE conflicts and need resolution.

**Coordination strategy** for 5B Coder Lead:
- Land per-language PRs sequentially (one merge → next Coder rebases).
- Or land them in parallel and the Coder Lead resolves conflicts (each is ~30 seconds of work).
- The marker-region shape (`xtask:per-lang:<fn>:start/:end`) is intentional: it gives git's diff algorithm clear hunk boundaries so the conflict markers point exactly at the per-language arm to keep.

**xtask is idempotent**: re-running `cargo xtask new-lang <X>` on a crate that's already wired skips the dispatch-arm insertion (matches by exact arm-line content) and the path-dep insertion (matches by exact dep line). Useful when re-running after a conflict resolution.

## Adding a new sensor

The shape — add hypothetical `lmprobe dependents <symbol>` (find every workspace dependency on a symbol):

1. **Define the envelope** in `crates/lmprobe-types/src/schema.rs`:

   ```rust
   envelope!(DependentsResult, "lmprobe.dependents.v1", hits: Vec<SymbolHit>);
   ```

   Or hand-write a struct if you need first-class context fields beyond a single Vec — see `EvolutionResult` / `ImpactResult` / `ContextResult` patterns.

2. **Implement the sensor** in `crates/lmprobe/src/<your_module>.rs`:

   ```rust
   pub fn dependents(name: &str, root: &Path, opts: &WalkOpts) -> Result<DependentsResult> {
       // walker logic
   }
   ```

   Use `apply_walk_opts` + `walk_pre_check` from `sensors.rs` for the standard parallel walk skeleton. See `crates/lmprobe/src/graph_sensors.rs::pattern_walk` for a generic helper.

3. **Add the CLI variant** to the `Commands` enum in `cli.rs` with one-line `///` doc:

   ```rust
   /// Find every workspace dependency on `<NAME>`.
   Dependents {
       name: String,
       #[clap(default_value = ".")]
       path: PathBuf,
   },
   ```

4. **Add the dispatch arm** in `run()`:

   ```rust
   Commands::Dependents { name, path } => {
       let res = your_module::dependents(&name, &path, &walk_opts)?;
       render_by_format(&res, format, "dependents", |r| {
           render_symbol_hits_markdown(&r.hits)
       });
       ("dependents", res.exit_status, res.warnings)
   }
   ```

5. **Add a markdown renderer** in `output.rs` if the verb's hits aren't a plain `Vec<SymbolHit>` (otherwise `render_symbol_hits_markdown` is enough).

6. **Add a smoke case** in `tests/smoke.rs` and bless the golden with `UPDATE_GOLDENS=1`.

7. **Update `crates/lmprobe/ARCHITECTURE.md` § Sensor Inventory** with the new row.

8. **Document** in the relevant chapter (`durable-memory/03_sensors_search.md` or `durable-memory/04_sensors_graph.md` etc.) and update `durable-memory/index.md` if a new chapter is warranted.

## v1-additive schema rules

Per [08_envelope_schema.md](08_envelope_schema.md), the policy:

- Adding a new optional field with `#[serde(default)]` is fine.
- Adding a new variant to a leaf enum (e.g. extending `change_type` taxonomy on `BlameEntry`) is fine.
- Renaming or removing a field requires `vN+1` and explicit Lead approval.

Phase 4D added several v1-additive fields (`origin_root`, `enclosing_decl`, `worktree_path`, `patch_path`, `audit_log_path`, `applied_files_count`, `follows`) without bumping any envelope to v2.

## Read next

- [10_languages.md](10_languages.md) for the per-language coverage matrix.
- [08_envelope_schema.md](08_envelope_schema.md) for the schema-versioning policy.
- [16_testing.md](16_testing.md) for adding deep-tier verdict rows + smoke goldens.
