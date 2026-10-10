//! `lmprobe-lang-go` — D5 capability impls for Go.
//!
//! Phase 5B.9 extraction. Mirrors the trait impls that previously
//! lived in `lmprobe-lang-core::langs`. Each impl struct is reachable
//! from the registry's per-language dispatch arms
//! (`SgLang::Go => &lmprobe_lang_go::Go*`).
//!
//! Structs exported (per the registry's expected naming):
//! - `GoExtractor` — delegates to `GenericExtractor` (Go's
//!   `function_declaration` / `method_declaration` / `type_spec`
//!   nodes carry a `name` field).
//! - `GoCallGrammar` — delegates to `StandardCallGrammar`
//!   (`name(args)` / `obj.name(args)` / `Type::name(args)`).
//! - `GoImportGrammar` — `import "name"` plus capitalised-identifier
//!   export forms (`func`, `type … struct`, `type … interface`,
//!   `var`, `const`).
//! - `GoInheritanceGrammar` — empty (Go has no `class extends`;
//!   composition is via struct embedding + interface satisfaction is
//!   structural, neither expressible as a single pattern). Preserved
//!   as `NoInheritanceGrammar` for byte-equality.
//! - `GoDecorationGrammar` — empty (Go has no annotation syntax;
//!   build tags + `//go:` directives are line-comment based and
//!   handled outside the decoration trait).
//! - `GoTraceTemplate` — `fmt.Fprintln(os.Stderr, "[TRACE] var =",
//!   $var)`.
//! - `GoKindTaxonomy` — portable-kind ↔ Go tree-sitter-kind map.
//!   Notably reuses `type_declaration` for Struct, Interface, and
//!   TypeAlias (the tree-sitter grammar wraps all three under the
//!   same outer kind; the inner `type_spec` distinguishes them).
//! - `GoTopLevelKinds` — empty (Go isn't a data lang).
//!
//! Trait API constraint (W4): structs implement only the
//! `lmprobe-lang-core` traits + `ast-grep-core` types. No
//! `lmprobe-types` envelope refs — keeps async-graphql out of
//! per-language builds.

#![allow(dead_code)]

use lmprobe_lang_core::langs::{
  CallGrammar, DecorationGrammar, GenericExtractor, ImportGrammar, InheritanceGrammar,
  KindTaxonomy, NameExtractor, PortableKind, SgNode, StandardCallGrammar, TopLevelKinds,
  TraceTemplate,
};

// ──────────────────────────── name extractor ──────────────────────

/// Go uses the generic `field("name") → field("key") → child(0)`
/// strategy — `func Foo()`, `type Bar struct`, `type Baz interface`
/// all expose the identifier under the `name` field.
pub struct GoExtractor;
impl NameExtractor for GoExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    GenericExtractor.extract(node)
  }
}

// ──────────────────────────── call grammar ────────────────────────

/// Go uses standard `name(args)` / `obj.name(args)` shapes; defer
/// to `StandardCallGrammar`. (Go has no `Type::method` form, but
/// `StandardCallGrammar` emits it harmlessly — a `Type::name` pattern
/// won't match Go ASTs.)
pub struct GoCallGrammar;
impl CallGrammar for GoCallGrammar {
  fn call_patterns(&self, name: &str) -> Vec<String> {
    StandardCallGrammar.call_patterns(name)
  }
  fn callee_decl_patterns(&self, name: &str) -> Vec<String> {
    StandardCallGrammar.callee_decl_patterns(name)
  }
}

// ──────────────────────────── import grammar ──────────────────────

/// Go: 8 doctrinal `import` shapes (Phase 6C #11). Doctrine: `{name}`
/// is the import path string (e.g. `fmt`, `github.com/spf13/pflag`),
/// not the Go-side alias. Aliases (`alias "x"`), blank-id (`_ "x"`),
/// and dot-imports (`. "x"`) are metadata; the path is the anchor.
/// One hit per `import_spec` regardless of single-form vs block form.
///
/// AST SHAPE (tree-sitter-go):
/// - Single-form `import "fmt"`:
///   `import_declaration > import_spec > path: "fmt"`
/// - Block form `import ( "fmt" "os" )`:
///   `import_declaration > import_spec_list > import_spec*`
///
/// THE STRUCTURAL CHALLENGE — multi-spec block, target in non-first
/// position. ast-grep patterns parsed via `Pattern::try_new` cannot
/// represent "any number of leading import_spec siblings followed by
/// the target spec": a leading `$$$` always parses as the NAME slot
/// of the next `import_spec` (because Go's `import_spec` grammar is
/// `optional($._package_name) field('path', $.string)` — the metavar
/// satisfies the optional name slot). Verified empirically: every
/// shape of leading-multi-cap (`$$$`, `$$IMP`, `$X $Y`) collapses
/// into the next spec's name. Same root cause as the Nix
/// `inherit` precision caveat (`crates/lmprobe-lang-nix/src/lib.rs`).
///
/// THE TWO-METAVAR + STRING-PLACEHOLDER WORKAROUND:
/// - `$$$A "$P"` parses as TWO separate `import_spec`s when followed
///   by another spec — the multi-cap `$$$A` lives inside the first
///   `import_spec`'s name slot (matches both bare and aliased leading
///   shapes via $$$'s zero-or-more semantics) and `"$P"` is a string
///   literal whose CONTENTS (the path) are metavarized by `$P`. This
///   keeps each leading spec as one structural placeholder. Stacking
///   N of these covers position N+1.
/// - For aliased target (`alias "{name}"`, `_ "{name}"`, `. "{name}"`),
///   the bare pattern `$X "{name}"` parses as ERROR(identifier, string)
///   but `find_all` still matches it against any `import_spec` whose
///   children are name+path (covers all 3 alias kinds × any position
///   × top-level-or-block). Verified non-falsematch on `const`,
///   string literals in code, struct tags, function args.
///
/// PATTERN SET (for query name `X`):
/// - `import "X"`                                      single-form bare
/// - `$X "X"`                                          aliased ANY pos
/// - `import ("X"\n$$$)`                               block bare pos 1
/// - `import ($$$A1 "$P1"\n"X"\n$$$)`                  block bare pos 2
/// - `import ($$$A1 "$P1"\n$$$A2 "$P2"\n"X"\n$$$)`     block bare pos 3
/// - … positions 4–8 (covers up to 8-import blocks; cobra's max is 12,
///   real-world 95th percentile is ≤8). Position 9+ is a doctrinal
///   under-match — same shape as Nix's documented under-match
///   (`Pattern::contextual` extension to `ImportGrammar` would close
///   this; out of scope for Phase 6C #11).
///
/// Doctrine calls (designer-set, all honored):
/// - Wildcards: `import . "X"` is the closest analogue → captured
///   via the aliased pattern; module = anchor (path).
/// - Aliasing: capture the imported PATH (`X`); the alias name is
///   metadata.
/// - Multi-symbol per stmt: each `import_spec` IS one statement. One
///   hit per matched spec. Bare patterns anchor on `import_declaration`
///   (whole block) for structural reasons; aliased pattern anchors on
///   the spec node directly.
///
/// Phase 6C round-3 (TC-105) — basename-on-last-segment via the
/// engine's `needs_prefix_match` filter (designer-arbitrated path-A
/// waiver). tree-sitter-go's import path string is atomic from the
/// pattern matcher (round-2 blocker: `import "$$$/fmt"` → 0 hits).
/// We keep the round-1 9-pattern set (exact-match across block
/// positions 1-8) and rely on the post-match `content.contains(name)`
/// filter on hit text to enable basename-on-last-segment.
///
/// `imports fmt /tree` now matches:
///   - `import "fmt"`              (exact)
///   - `import "encoding/fmt"`     (basename — broad pattern + filter)
///
/// **Coder pick (designer-asked)**: simple `content.contains(name)`
/// over path-aware regex. Justification: (1) simpler implementation
/// (no regex compile per query); (2) consistent with the same engine
/// path Elixir uses; (3) the false-positive class (`imports fmt`
/// matching `import "fmtutil"`) is acceptable — Go authors typically
/// query the package basename and `fmtutil`-style overlaps are rare.
/// Path-aware regex `(^|/)<name>($|"|')` is more precise but adds an
/// engine-side dependency; deferred to a future round if false-
/// positive rate proves problematic in QA.
pub struct GoImportGrammar;
impl ImportGrammar for GoImportGrammar {
  fn import_patterns(&self, name: &str) -> Vec<String> {
    // Phase 6C round-4: position-specific block-form patterns
    // dropped — engine's `block_form_spec_kind` DFS now handles
    // bare AND aliased block-form specs at any position via
    // per-spec content filtering. Pattern set reduces to:
    //   - exact-match single-form (bare and aliased)
    //   - broad single-form for basename match
    //   - broad aliased for cross-form aliased matching
    vec![
      format!("import \"{name}\""), // exact bare single-form
      format!("$X \"{name}\""),     // exact aliased (single+block)
      "$X \"$P\"".to_string(),      // broad aliased + filter
      "import \"$P\"".to_string(),  // broad single + filter
    ]
  }
  fn needs_prefix_match(&self) -> bool {
    true
  }
  fn block_form_spec_kind(&self) -> Option<(&'static str, &'static str)> {
    // tree-sitter-go: block-form `import (\n "X"\n "Y"\n)` parses
    // as `import_declaration > import_spec_list > import_spec*`.
    // The engine's DFS picks up bare specs at any position.
    Some(("import_spec", "import_spec_list"))
  }
  fn export_patterns(&self, name: &str) -> Vec<String> {
    // Go: capitalised top-level identifier is the export signal.
    vec![
      format!("func {name}($$$) {{ $$$ }}"),
      format!("func {name}($$$) $RET {{ $$$ }}"),
      format!("type {name} struct {{ $$$ }}"),
      format!("type {name} interface {{ $$$ }}"),
      format!("var {name} = $RHS"),
      format!("const {name} = $RHS"),
    ]
  }
}

// ─────────────────────────── inheritance grammar ──────────────────

/// Go has no `class extends` syntax — composition is via struct
/// embedding (an unnamed field whose type is the embedded type)
/// and interface satisfaction is structural (no `implements`
/// keyword). Neither shape reduces to a single ast-grep pattern,
/// so lang-core fell through to `NoInheritanceGrammar`. Preserve
/// the empty shape for byte-equality.
pub struct GoInheritanceGrammar;
impl InheritanceGrammar for GoInheritanceGrammar {}

// ─────────────────────────── decoration grammar ───────────────────

/// Go has no annotation/attribute syntax. Build tags
/// (`//go:build linux`) and pragmas (`//go:noinline`) are
/// line-comment based and handled outside the decoration trait.
/// Preserve the empty `NoDecorationGrammar` shape.
pub struct GoDecorationGrammar;
impl DecorationGrammar for GoDecorationGrammar {
  fn decoration_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ─────────────────────────── trace template ───────────────────────

pub struct GoTraceTemplate;
impl TraceTemplate for GoTraceTemplate {
  fn template_for(&self, var: &str) -> Option<String> {
    Some(format!(
      "fmt.Fprintln(os.Stderr, \"[TRACE] {var} =\", ${var})"
    ))
  }
}

// ─────────────────────────── kind taxonomy ────────────────────────

pub struct GoKindTaxonomy;
impl KindTaxonomy for GoKindTaxonomy {
  fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
    match p {
      PortableKind::Function => &["function_declaration"],
      PortableKind::Method => &["method_declaration"],
      PortableKind::Class => &[],
      // Go folds struct, interface, and type-alias declarations
      // under the same outer `type_declaration` kind. The inner
      // `type_spec` child carries the actual shape.
      PortableKind::Struct => &["type_declaration"],
      PortableKind::Interface => &["type_declaration"],
      PortableKind::Trait => &[],
      PortableKind::Enum => &[],
      PortableKind::TypeAlias => &["type_declaration"],
      PortableKind::Const => &["const_declaration", "var_declaration"],
      PortableKind::Macro => &[],
      PortableKind::Import => &["import_declaration"],
      PortableKind::Route => &[],
    }
  }
}

// ─────────────────────────── top-level kinds ──────────────────────

/// Go isn't a data language — empty impl. The trait defaults
/// fire (no child kinds, no top-level enumeration).
pub struct GoTopLevelKinds;
impl TopLevelKinds for GoTopLevelKinds {}
