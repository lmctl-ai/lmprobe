# 18 — Phase 5B per-language extraction playbook

This chapter is the deep-dive playbook for the Phase 5B fan-out: extracting each language from `lmprobe-lang-core` into its own `lmprobe-lang-<X>` crate. It is the canonical reference Coder Lead sends to every per-language Coder before they start work — read together with [11_extending.md](11_extending.md), it is sufficient to take a single language end-to-end without further dispatch.

The pattern was validated by `CoderRust` (Phase 5A.3, commit `4187ce62`) and `CoderKotlin` (Phase 5B.1, commit `92b560e8`); the `5A.7` follow-up (`065308f1`) closed the routing bug CoderKotlin's STOP signal surfaced and made Step 6 (deleting dead lang-core impls) safe for the whole Phase 5B set.

## What Phase 5B is

Phase 5A established the workspace split (`lmprobe-types` / `lmprobe-lang-core` / `lmprobe-registry` / `lmprobe-engine` / `lmprobe-lang-<X>` / `lmprobe`) and seeded 28 empty `crates/lmprobe-lang-<X>/` directories needed for parallel language extractions. Phase 5B is the parallel sweep: one Coder, one language, one crate, one PR. The per-Coder session spawns with `sessiondir = .../crates/lmprobe-lang-<X>/`. All language crates are now extracted, and the template workflow remains the canonical path for future additions.

Each extraction is **byte-equal** to the pre-extraction behavior — the registry's per-language dispatch arms route the same trait impls through a different module path. The deep-tier `lang_verdicts_match` test enforces this: post-extraction verdicts on the `post_phase_2c` column of `LANG_VERDICTS` must match what lang-core was producing before.

## Workflow checklist

The 8 steps every Coder runs from session start through commit. Steps 1–4 are mechanical; 5 is the bulk of the work; 6–8 are the close-out.

1. **Spawn-time state.** For historical seeded sessions, the workdir opens with `crates/lmprobe-lang-<X>/` containing `.gitkeep`. In the current tree, all seeded dirs are already materialized; this workflow still applies to future language additions. The workspace `[workspace] exclude` list in `/Cargo.toml` lists legacy placeholders only during migration.

2. **Scaffold the crate** (from the workspace root):

   ```bash
   cd $LMPROBE_REPO   # alias is literal-cwd-relative; xtask reads ./xtask/templates/lang/
   cargo xtask new-lang <X>
   ```

   xtask detects the seeded empty dir, removes `.gitkeep`, copies `xtask/templates/lang/` into the crate, inserts 8 dispatch arms into `crates/lmprobe-registry/src/lib.rs` (one per trait), appends a path-dep to `crates/lmprobe-registry/Cargo.toml`, and removes your crate from the workspace `exclude` list. After this step `cargo build -p lmprobe-lang-<X>` should succeed against the stub impls.

3. **Read the lang-core source you're replacing.** Open `crates/lmprobe-lang-core/src/langs.rs` and find every reference to your language: bespoke `*Extractor` struct, `*ImportGrammar`, `*InheritanceGrammar`, `*DecorationGrammar`, `*TraceTemplate`, `*Taxonomy` (declared via the `taxonomy!` macro), and the explicit `SgLang::<X> => &<X>...` arms in the 8 dispatch fns. This is the source of truth for what your crate must produce.

4. **Pick the pattern.** If your language uses `GenericExtractor` + `StandardCallGrammar` and has no special helpers, follow the **simple delegation** pattern (Rust). If lang-core has private `clean` / `first_named_child` helpers + a bespoke `extract` walk for your language, follow the **bespoke** pattern (Kotlin). See [§ Reference patterns](#two-reference-patterns) below.

5. **Fill in the trait impls** in `crates/lmprobe-lang-<X>/src/lib.rs`. Every struct already exists from the template; replace the stub bodies with the impls you copied (and adjusted) from lang-core. Drop fixtures into `fixtures/` (≤5 small files, idiomatic, ≤30 lines each). Replace the template's `tests/smoke.rs` with bespoke per-crate assertions (Rust has 11, Kotlin has 13 — pin the patterns the binary's deep tier will exercise).

6. **Delete the dead lang-core impls.** Now safe post-5A.7. See [§ Step 6](#step-6-delete-dead-lang-core-impls) for the exact list.

7. **Gate sweep.** Run all 9 gates from [§ Gate sweep](#gate-sweep--verdict-match-check). Every one must be green before you commit. The non-negotiable gate is `lang_verdicts_match` — a mismatch means your crate is producing different output than lang-core was, and the registry will refuse to ship.

8. **Commit + report.** Single commit per language, conventional shape:

   ```
   feat(lang-<X>): 5B.<n> — extract <X> to lmprobe-lang-<X>
   ```

   Don't push. Reply to Coder Lead per [§ Reporting back](#reporting-back-to-coder-lead). Coder Lead reviews, lands, and dispatches the next Coder.

## The xtask scaffold

`cargo xtask new-lang <X>` reads `xtask/templates/lang/` and writes the rendered files into `crates/lmprobe-lang-<X>/`. Substitution: plain `str::replace` on `{{lang}}` (lowercase), `{{Lang}}` (PascalCase), `{{LANG}}` (uppercase), `{{tree_sitter_dep}}`. Files written:

```
crates/lmprobe-lang-<X>/
  Cargo.toml          # workspace deps + tree-sitter-<X>
  README.md           # per-crate quick-reference
  src/lib.rs          # 8 unit structs with stub impls
  tests/smoke.rs      # asserts every struct constructs
  fixtures/           # empty; you populate
```

Registry edits:

- `crates/lmprobe-registry/src/lib.rs` — one new arm per dispatch fn (8 total), inserted inside the `xtask:per-lang:<fn>:start/:end` marker regions. Each arm reads `SgLang::<Lang> => &lmprobe_lang_<lang>::<Lang><TraitSuffix>`.
- `crates/lmprobe-registry/Cargo.toml` — appends a path-dep on `lmprobe-lang-<lang>` after the `lmprobe-lang-core = …` anchor line.
- `Cargo.toml` (workspace root) — removes your crate from the `[workspace] exclude` list (only fires when xtask detected the seeded-empty-dir case).

xtask is **idempotent** — re-running on a wired crate skips arms that already exist (matched by exact line content). The `cd $LMPROBE_REPO` step is mandatory: xtask reads `xtask/templates/lang/` relative to its cwd and `crates/lmprobe-registry/src/lib.rs` likewise.

## Trait API constraint W4

W4 is a **method-signature** rule, not a Cargo dependency rule. Per-language crates do depend on `lmprobe-types` transitively (via `lmprobe-lang-core`'s manifest, see `crates/lmprobe-lang-rust/Cargo.toml`), so async-graphql is in every per-language build's dep graph regardless. The constraint is on what types appear in your `impl` blocks' method signatures:

- `lmprobe-lang-core` trait associated types: `SgNode`, `PortableKind`, `SgLang`.
- `ast-grep-core` types reached through `SgNode` (children, fields, kind names).
- Rust primitives + `String` / `Vec<String>` / `Option<String>`.

**Why it matters**: keeps the trait API decoupled from the D4 envelope schema. A v2 envelope shape (e.g. renaming a field on `SymbolHit`, or restructuring `BlameResult`) doesn't force every per-language crate to re-impl its traits — the envelope changes only ripple through the engine, where envelope construction happens. The 29 per-language crates stay v1-additive-immune. Side benefit: keeps trait impls thin (per-language crates produce patterns + extract names; they never assemble result envelopes), which preserves the engine-as-orchestrator boundary.

The template enforces this by importing only from `lmprobe_lang_core::langs::*` — keep that import block minimal as you fill in impls. A `use lmprobe_types::SymbolHit;` (or any `lmprobe-types` import) inside a per-language crate is a W4 violation even though it'd compile.

## Struct-naming gotcha

The xtask-inserted dispatch arms expect **specific struct names**. Lang-core's `taxonomy!` macro generates a `<Lang>Taxonomy` symbol (no `Kind` infix) — but the registry's dispatch arm for `taxonomy_for` inserts `<Lang>KindTaxonomy` (with `Kind` infix). You must export `<Lang>KindTaxonomy` from your crate, not `<Lang>Taxonomy`, or the registry won't compile.

The 8 names the registry expects (substitute your PascalCase lang for `<Lang>`):

| Trait | Struct name expected by registry |
|-------|----------------------------------|
| `NameExtractor`      | `<Lang>Extractor` |
| `CallGrammar`        | `<Lang>CallGrammar` |
| `ImportGrammar`      | `<Lang>ImportGrammar` |
| `InheritanceGrammar` | `<Lang>InheritanceGrammar` |
| `DecorationGrammar`  | `<Lang>DecorationGrammar` |
| `TraceTemplate`      | `<Lang>TraceTemplate` |
| `KindTaxonomy`       | `<Lang>KindTaxonomy` |
| `TopLevelKinds`      | `<Lang>TopLevelKinds` |

The template scaffolds all 8 with these exact names; if you rename any of them you'll break the registry's dispatch arms. Don't.

**PascalCase mapping.** xtask's `pascal_case` fn maps your workspace dir name (`csharp` / `javascript` / `typescript` / `rust` / …) to the `SgLang::<Variant>` PascalCase form. Three SgLang variants have non-trivial casing — `csharp → CSharp`, `javascript → JavaScript`, `typescript → TypeScript` — and xtask hard-codes them (see `pascal_case` in `xtask/src/new_lang.rs` plus its `pascal_case_special_cases` test). All other 25 langs naive-title-case correctly (`tsx → Tsx`, `cpp → Cpp`, etc.). Sanity-check after scaffold: `grep "SgLang::<expected>" crates/lmprobe-registry/src/lib.rs` should return 8 hits (one per dispatch arm). If you see `SgLang::Csharp` or `SgLang::Javascript` instead, your xtask is pre-Phase-5B-fixup — pull the latest `main`.

## Two reference patterns

### Simple delegation (Rust)

For languages whose declaration nodes carry a `name` field and whose call shapes are `name(args)` / `obj.name(args)` / `Type::name(args)`. Most curly-brace languages fit this mould. The trait impl just delegates to lang-core's generic helpers:

```rust
use lmprobe_lang_core::langs::{
    CallGrammar, GenericExtractor, KindTaxonomy, NameExtractor,
    PortableKind, SgNode, StandardCallGrammar,
};

pub struct RustExtractor;
impl NameExtractor for RustExtractor {
    fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
        GenericExtractor.extract(node)
    }
}

pub struct RustCallGrammar;
impl CallGrammar for RustCallGrammar {
    fn call_patterns(&self, name: &str) -> Vec<String> {
        StandardCallGrammar.call_patterns(name)
    }
    fn callee_decl_patterns(&self, name: &str) -> Vec<String> {
        StandardCallGrammar.callee_decl_patterns(name)
    }
}

pub struct RustKindTaxonomy;
impl KindTaxonomy for RustKindTaxonomy {
    fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
        match p {
            PortableKind::Function => &["function_item"],
            PortableKind::Struct   => &["struct_item"],
            PortableKind::Trait    => &["trait_item"],
            // …
            _ => &[],
        }
    }
}
```

Reference: `crates/lmprobe-lang-rust/src/lib.rs`.

### Bespoke (Kotlin)

For languages whose declaration nodes need a custom walk — e.g. `class_declaration` whose `child(0)` is the `class` keyword rather than the identifier, or whose call grammar isn't standard. Replicate the helpers privately inside your crate (do **not** expand lang-core's public API for them):

```rust
use lmprobe_lang_core::langs::{NameExtractor, SgNode};

fn clean(s: &str) -> String { s.trim().trim_matches(|c| c == '"' || c == '\'').to_string() }

fn first_named_child<'a>(node: &SgNode<'a>, kinds: &[&str]) -> Option<SgNode<'a>> {
    node.children().find(|c| kinds.contains(&c.kind().as_ref()))
}

fn is_keyword_kotlin(s: &str) -> bool {
    matches!(s, "class" | "interface" | "fun" | "object" | "suspend" | "override" | "open" | "data")
}

pub struct KotlinExtractor;
impl NameExtractor for KotlinExtractor {
    fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
        if let Some(name) = node.field("name") {
            let s = clean(&name.text());
            if !s.is_empty() && !is_keyword_kotlin(&s) { return Some(s); }
        }
        first_named_child(node, &["simple_identifier", "type_identifier"])
            .map(|n| clean(&n.text()))
    }
}
```

Reference: `crates/lmprobe-lang-kotlin/src/lib.rs`. The 2-line helpers are intentionally duplicated rather than promoted to lang-core — every bespoke extractor would otherwise inflate lang-core's public surface.

## Step 6: delete dead lang-core impls

Now safe post-5A.7 (`065308f1`). Pre-5A.7 the engine bypassed `lmprobe-registry` and called `lmprobe_lang_core::langs::*_for` directly from 4 source files — deleting lang-core impls would have silently regressed your language to `GenericExtractor` (verdict Ok → Partial). 5A.7 fixed every engine import to route through `lmprobe-registry`; deleting is now byte-equal.

What to remove from `crates/lmprobe-lang-core/src/langs.rs` for your `<Lang>`:

- The bespoke `<Lang>Extractor` struct + impl (if your lang has one — many use `GenericExtractor` and have nothing to remove here).
- The `<Lang>ImportGrammar` struct + impl (if present).
- The `<Lang>InheritanceGrammar` struct + impl (if present).
- The `<Lang>DecorationGrammar` struct + impl (if present).
- The `<Lang>TraceTemplate` struct + impl (if present).
- The `taxonomy!(<Lang>Taxonomy { … })` macro invocation.
- All `SgLang::<Lang>` mentions in lang-core's dispatch fns. Of the 8 dispatch fns, **up to 7** can carry an explicit per-lang mention (`top_level_kinds_for` only mentions data langs — Json/Yaml/Toml/Xml/Html/Css). Worked example: Rust is wired through 6 sites in lang-core — five exclusive arms (`SgLang::Rust => &RustImportGrammar` in `import_grammar_for`, and similarly in `inheritance_grammar_for`, `decoration_grammar_for`, `trace_template_for`, `taxonomy_for`) plus one `|`-group membership in `call_grammar_for` (`SgLang::Rust | SgLang::Java | SgLang::JavaScript | … => &StandardCallGrammar`). For the `|`-group case, remove just `SgLang::<Lang> |` from the alternation, leaving the other languages intact. The `|`-group hit is easy to miss with a naive grep — `grep "SgLang::<Lang>" crates/lmprobe-lang-core/src/langs.rs` is the canonical check (Rust returns 6 hits; Kotlin pre-5A.7 returned 5).
- Any private helper used **only** by your lang's deleted impl (e.g. `is_keyword_kotlin`).

The exhaustive-match check: `taxonomy_for` was historically exhaustive on `SgLang`. 5A.7 added `pub struct NoKindTaxonomy` + `_ => &NoKindTaxonomy` catch-all. Removing your language's arm should compile cleanly. If you see an `exhaustive-match` error here, the catch-all is missing — file a bug, don't paper over it.

CoderKotlin **deferred** Step 6 (correctly — pre-5A.7 it was unsafe). Every Coder from 5B.2 onward must do it. Verify by re-running `lang_verdicts_match` after the deletion: green = your registry arm is the sole serving path, red = the engine still has a stray `lmprobe_lang_core::langs::*_for` import that bypassed the registry.

## Gate sweep + verdict-match check

The full 12-gate sweep that must pass before commit. Mirrors the gate block recent commit messages quote (see `92b560e8` and `065308f1`); covers every test tier from [16_testing.md](16_testing.md) plus the GraphQL surface-equality regression guard documented in [17_graphql.md](17_graphql.md):

```bash
# 1. Per-crate build.
cargo build -p lmprobe-lang-<X>

# 2. Per-crate smoke (your bespoke assertions).
cargo test -p lmprobe-lang-<X>

# 3. Per-crate drift lint.
cargo xtask check-lang <X>

# 4. Binary smoke (golden byte-equality across 48 cases).
cargo test -p lmprobe --release --test smoke

# 5. Verdict-match (THE gate that proves byte-equality).
cargo test -p lmprobe --release --test deep -- --ignored lang_verdicts_match --nocapture

# 6. Perf gate (core sensors).
cargo test -p lmprobe --release --test perf -- --ignored

# 7. Perf gate (history).
cargo test -p lmprobe --release --test history_perf -- --ignored

# 8. Blob-cache invariants (only-grows / content-addressable).
cargo test -p lmprobe --release --test blob_cache

# 9. GraphQL surface equality (CLI ↔ GraphQL byte-equal on 5 read sensors + D7 diagnostic equivalence).
cargo test -p lmprobe --release --test graphql_surface_equality

# 10. Auth middleware (serve feature only — gates the daemon's bearer-token path).
cargo test -p lmprobe --features serve --test auth

# 11. Build clean — default features.
cargo build --release

# 12. Build clean — serve features.
cargo build --release --features serve
```

`lang_verdicts_match` (gate 5) is the load-bearing gate. It re-scores every language against `LANG_VERDICTS` (in `crates/lmprobe/tests/deep.rs`) on the `post_phase_2c` column and asserts byte-equality with the recorded verdict. A regression here means your impl drifted from lang-core's behavior — usually a missing pattern in `import_patterns` / `decoration_patterns`, or a `KindTaxonomy` arm that returned the wrong tree-sitter literal.

Gates 8–10 are the surface-regression guards. The per-language extraction shouldn't touch the cache layer, the GraphQL resolvers, or the auth middleware — but a dispatch-fn import that accidentally moved from `lmprobe_registry::*` back to `lmprobe_lang_core::langs::*_for` will surface here as well as in gate 5. Run them; they're <5s each.

Builds 11 + 12 must produce **0 warnings** across the whole workspace; CI enforces this. Optionally add `cargo build --release --all-features` (a third 0-warning gate Coder Lead reports in the commit body) — useful when your lang's stub touched anything feature-gated.

## Troubleshooting

### `error: missing trait method 'kind_for'`

The template ships `<Lang>KindTaxonomy` with a default `kind_for` body returning `&[]`. If you deleted the body while restructuring, re-add:

```rust
fn kind_for(&self, _: PortableKind) -> &'static [&'static str] { &[] }
```

`KindTaxonomy::kind_for` has no default — every impl must provide one.

### `lang_verdicts_match` regression after Step 6

The registry isn't actually routing to your crate; the engine still has a stray `lmprobe_lang_core::langs::*_for` import that bypasses it. Check the four files 5A.7 fixed:

- `crates/lmprobe-engine/src/sensors.rs`
- `crates/lmprobe-engine/src/ast_diff_sensor.rs`
- `crates/lmprobe-engine/src/history_sensors.rs`
- `crates/lmprobe-engine/src/graph_sensors.rs`

Every dispatch-fn import (`name_extractor_for`, `taxonomy_for`, `call_grammar_for`, `import_grammar_for`, `inheritance_grammar_for`, `decoration_grammar_for`, `trace_template_for`, `top_level_kinds_for`) must come from `lmprobe_registry::*`, never `lmprobe_lang_core::langs::*`. The `SgLang` type itself does still come from lang-core — that's correct.

If a new bypass surfaces in a sensor 5A.7 didn't touch, that's a STOP signal: file it back to Coder Lead, don't paper over it by re-adding the lang-core impl.

### `cargo build` fails: `could not load Cargo.toml` for your crate

The workspace `[workspace] exclude` list in `/Cargo.toml` still lists your crate. xtask should remove it during scaffold (it only does so on the seeded-empty-dir path). If you scaffolded into a non-seeded dir (or xtask was run from an unexpected cwd), edit `/Cargo.toml` manually:

```toml
exclude = [
    # remove the line for your lang
    …
]
```

### `taxonomy_for` exhaustive-match fail when deleting your arm

Lang-core's `taxonomy_for` already has `_ => &NoKindTaxonomy` catch-all post-5A.7. If you see a non-exhaustive-match error after removing your `SgLang::<Lang> =>` arm, the catch-all is missing — that's a regression in lang-core, file it back to Coder Lead. Don't paper over by re-adding the deleted arm.

### Perf gate flaky on your dev machine

Known hardware-variance pattern. The 4E-D and 5A.7 runs both bumped thresholds for `ref` (260→360ms), `context_fn` (250→280→440ms) on different developer machines. Inline comments in `crates/lmprobe/tests/perf.rs` document the bump pattern: `max(observed) × 1.10`, rounded. If your machine pushes a sensor over its threshold with no functional change, follow the same pattern in your commit (and call it out in the report-back so Coder Lead can sanity-check the bump isn't masking a real regression).

### STOP-and-surface heuristic

When something doesn't fit — a sensor's import bypasses the registry, a verdict drift you can't explain, a perf jump that correlates with your changes — **stop and surface the signal** rather than fudging the gates green. CoderKotlin's STOP signal on Step 6 triggered the entire 5A.7 routing fix; without it the next 26 extractions would have silently regressed. Coder Lead values surfacing arch surprises far more than completed-but-wrong work; a deferred Step 6 with a clear explanation is a successful outcome.

## Reporting back to Coder Lead

Final reply should include, in this order:

1. **Commit SHA + one-line summary.** E.g. `feat(lang-<X>): 5B.<n> — extract <X> to lmprobe-lang-<X>` at SHA `<short>`.
2. **Per-gate one-liner.** Result of each of the 12 gates from § Gate sweep:

   ```
   - cargo build -p lmprobe-lang-<X>                              → ok
   - cargo test  -p lmprobe-lang-<X>                              → N passed
   - cargo xtask check-lang <X>                                   → clean (1 verified)
   - cargo test  -p lmprobe --release --test smoke                → 48 byte-equal
   - cargo test  -p lmprobe --release --test deep --ignored lang_verdicts_match → green
   - cargo test  -p lmprobe --release --test perf --ignored       → green
   - cargo test  -p lmprobe --release --test history_perf --ignored → green
   - cargo test  -p lmprobe --release --test blob_cache           → 4 passed
   - cargo test  -p lmprobe --release --test graphql_surface_equality → 3 passed
   - cargo test  -p lmprobe --features serve --test auth          → ok
   - cargo build --release                                        → 0 warnings
   - cargo build --release --features serve                       → 0 warnings
   ```

3. **Verdict-match row before/after.** Pull `<X>`'s row from `LANG_VERDICTS` and confirm the live `post_phase_2c` verdict matches.
4. **Any STOP signal.** If you deferred Step 6 or hit something that doesn't fit, say so explicitly and explain why. The arbitration cost is far lower than a silent regression.
5. **Perf-bump justification, if any.** If gate 6 or 7 needed a threshold bump, paste the 5-run distribution and the new ceiling.

Coder Lead reviews, dispatches the next per-language Coder.

## Read next

- [11_extending.md](11_extending.md) — D5 capability traits + the high-level "add a new language" workflow this playbook deepens for 5B.
- [10_languages.md](10_languages.md) — per-language coverage matrix; cross-check your `LANG_VERDICTS` row.
- [16_testing.md](16_testing.md) — test-tier definitions + how `lang_verdicts_match` fits the regression matrix.
- [15_perf.md](15_perf.md) — three-tier perf budget; context for any threshold bump.
