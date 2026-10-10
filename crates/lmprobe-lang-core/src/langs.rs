//! Language registry — D5 capability traits.
//!
//! Phase 0 §0.5 installed:
//! - [`NameExtractor`] — per-language identifier extraction from a
//!   declaration node. Closes the QA §2.3 family of failures where
//!   the generic `field("name") → field("key") → child(0)` fallback
//!   returns a keyword (`class`, `interface`, `void`) instead of the
//!   identifier.
//! - [`CallGrammar`] — per-language call-site patterns for the
//!   `callers` verb. Replaces the hard-coded `lang_supports_callers`
//!   list shipped in Chunk 0B with a proper trait dispatch (D5
//!   composition).
//!
//! Phase 1 §1.4 will add `KindTaxonomy` (portable function/method/
//! class names) on top of these. Phase 2 broadens `ImportGrammar` /
//! `DecorationGrammar`. The shape established here is the template
//! all of those follow.

use ast_grep_core::tree_sitter::StrDoc;
use ast_grep_core::Node;

pub use ast_grep_language::SupportLang as SgLang;

/// Convenience alias: the Node type the rest of the crate operates on.
pub type SgNode<'a> = Node<'a, StrDoc<SgLang>>;

// ─────────────────────────── helpers ──────────────────────────────

fn clean(s: &str) -> String {
  s.trim().trim_matches(|c| c == '"' || c == '\'').to_string()
}

// ───────────────────────── NameExtractor ──────────────────────────

/// Extract the symbol name from a declaration node. Returning `None`
/// signals the caller should fall back to a generic strategy (or
/// emit `unknown`).
pub trait NameExtractor: Send + Sync {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String>;
}

/// Generic fallback — the strategy the pre-§0.5 binary used everywhere.
/// Works for languages with a `name` field on declarations (Rust,
/// Java, Python, Go, JS/TS, Json/Yaml/Toml/Xml, Dart, Lua, Bash,
/// Scala, PHP).
pub struct GenericExtractor;
impl NameExtractor for GenericExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    let n = node
      .field("name")
      .or_else(|| node.field("key"))
      .or_else(|| node.child(0))?;
    let s = clean(&n.text());
    if s.is_empty() {
      None
    } else {
      Some(s)
    }
  }
}

pub fn name_extractor_for(lang: SgLang) -> &'static dyn NameExtractor {
  match lang {
    _ => &GenericExtractor,
  }
}

// ───────────────────────── CallGrammar ────────────────────────────

/// Per-language call-site patterns for the `callers` verb.
/// Returning an empty Vec signals "this language doesn't have
/// method/function-call grammar (or we don't know it yet)" — the
/// caller emits a `NOT_IMPLEMENTED` diagnostic.
pub trait CallGrammar: Send + Sync {
  fn call_patterns(&self, name: &str) -> Vec<String>;
  /// §3.3 callees-side: patterns that match a function/method
  /// declaration named `name` whose body we will then walk for
  /// internal calls. Default: the same shape as `call_patterns`'s
  /// declaration-side counterpart (`fn name(...)` / `def name(...)` /
  /// etc.) so the sensor can locate the body and DFS its descendants.
  fn callee_decl_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

/// Languages with `obj.method(args)` and `func(args)` grammars: Rust,
/// Java, JavaScript, TypeScript, TSX, Python, Go, C, C++, C#.
/// Replaces Chunk 0B's hard-coded `lang_supports_callers` list with a
/// trait dispatch.
pub struct StandardCallGrammar;
impl CallGrammar for StandardCallGrammar {
  fn call_patterns(&self, name: &str) -> Vec<String> {
    // QA TC-014: path-prefixed calls (`hs::name(...)`,
    // `crate::a::b::name(...)`, C++ `ns::Foo::name(...)`) are
    // common in Rust and C++ but matched by neither bare nor
    // method-call patterns. The third pattern catches them.
    // Languages without `::` path syntax (Python, JS/TS, Go,
    // Java, C#) silently drop the failed parse.
    vec![
      format!("$OBJ.{}($$$ARGS)", name),
      format!("{}($$$ARGS)", name),
      format!("$$$PATH::{}($$$ARGS)", name),
    ]
  }
  fn callee_decl_patterns(&self, name: &str) -> Vec<String> {
    // Wide net across the 10 standard-call languages; the sensor
    // walks each match's body for descendant call sites. Some
    // patterns will fail to parse on some grammars (e.g. `fn` is
    // Rust-only) — failed-parse falls through cleanly.
    vec![
      // Rust
      format!("fn {}($$$) {{ $$$ }}", name),
      format!("fn {}($$$) -> $RET {{ $$$ }}", name),
      format!("pub fn {}($$$) {{ $$$ }}", name),
      format!("pub fn {}($$$) -> $RET {{ $$$ }}", name),
      format!("async fn {}($$$) -> $RET {{ $$$ }}", name),
      format!("pub async fn {}($$$) -> $RET {{ $$$ }}", name),
      // Python
      format!("def {}($$$): $$$", name),
      format!("async def {}($$$): $$$", name),
      // JavaScript / TypeScript
      format!("function {}($$$) {{ $$$ }}", name),
      format!("export function {}($$$) {{ $$$ }}", name),
      format!("const {} = ($$$) => {{ $$$ }}", name),
      format!("export const {} = ($$$) => {{ $$$ }}", name),
      // Java / C# / C++
      format!("$RET {}($$$) {{ $$$ }}", name),
      format!("public $RET {}($$$) {{ $$$ }}", name),
      format!("private $RET {}($$$) {{ $$$ }}", name),
      // Go
      format!("func {}($$$) {{ $$$ }}", name),
      format!("func {}($$$) $RET {{ $$$ }}", name),
      format!("func ($RECV) {}($$$) {{ $$$ }}", name),
      format!("func ($RECV) {}($$$) $RET {{ $$$ }}", name),
    ]
  }
}

/// Sentinel for languages where call-syntax isn't applicable (XML,
/// YAML, JSON, TOML, HTML, CSS, …) or isn't yet modelled.
pub struct NoCallGrammar;
impl CallGrammar for NoCallGrammar {
  fn call_patterns(&self, _: &str) -> Vec<String> {
    Vec::new()
  }
}

// ────────────────────────── ImportGrammar ─────────────────────────
//
// §2.2 — D5 fourth capability trait. Returns ast-grep pattern
// strings that match a language's import / use / require syntaxes
// for the requested module name. Empty Vec = "this language has no
// import grammar wired" (sensor surfaces NOT_IMPLEMENTED).
//
// Same shape as `CallGrammar`: stateless, takes a `&str name`,
// returns `Vec<String>` patterns.

pub trait ImportGrammar: Send + Sync {
  fn import_patterns(&self, name: &str) -> Vec<String>;
  /// §3.3 exports-side: patterns matching where `name` is *re-
  /// exported* from a module (vs imported into one). Default empty
  /// — languages that ship explicit re-export grammar override.
  fn export_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
  /// Phase 6C round 3 (designer arbitration): some grammars have
  /// atomic AST nodes that pattern-string metavars can't decompose
  /// (tree-sitter-elixir's qualified alias `Foo.Bar.Baz`,
  /// tree-sitter-go's import path string `"encoding/fmt"`).
  /// Bare-root prefix-match in those grammars is impossible at the
  /// pattern layer. When this trait method returns `true`, the
  /// engine's `imports` sensor applies a post-match
  /// `hit.content.contains(name)` filter to eliminate false
  /// positives produced by intentionally-broad patterns.
  ///
  /// Default false — most grammars compose patterns finely enough
  /// to make the filter unnecessary. Override only in grammars
  /// where the filter is the only way to express prefix-match.
  fn needs_prefix_match(&self) -> bool {
    false
  }
  /// Phase 6C round 4 — block-form per-spec extraction. Some
  /// grammars (notably tree-sitter-go) parse multi-import blocks
  /// `import (\n "X"\n "Y"\n)` as a parent `*_list` node containing
  /// child `*_spec` nodes. Pattern strings can't enumerate metavar
  /// positions across the list, so bare-form specs at variable
  /// positions are unreachable via patterns alone. When this method
  /// returns `Some((spec_kind, parent_kind))`, the engine's
  /// `imports` sensor DFS-walks for `spec_kind` nodes whose parent
  /// is `parent_kind` and applies `content.contains(name)` per
  /// spec. Only consulted when `needs_prefix_match() == true`.
  fn block_form_spec_kind(&self) -> Option<(&'static str, &'static str)> {
    None
  }
}

pub struct NoImportGrammar;
impl ImportGrammar for NoImportGrammar {
  fn import_patterns(&self, _: &str) -> Vec<String> {
    Vec::new()
  }
}

pub fn import_grammar_for(lang: SgLang) -> &'static dyn ImportGrammar {
  match lang {
    _ => &NoImportGrammar,
  }
}

// ───────────────────── InheritanceGrammar ─────────────────────────
//
// §3.3 — D5 eighth capability trait. Single trait covers super-/
// sub-type and override grammar because the three are tightly
// coupled in every host language (the same `class X extends Y`
// shape feeds super- and sub-type queries; method override
// detection lives next to it). Empty Vec → NOT_IMPLEMENTED.

pub trait InheritanceGrammar: Send + Sync {
  /// Patterns matching the *declaration* of `name` whose body
  /// reveals its parents. Sensor finds these declarations and
  /// extracts the parent identifier(s) from the matched node.
  fn supertype_decl_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
  /// Patterns where some other type extends `name`. Direct hits
  /// in the workspace are subtypes of `name`.
  fn subtype_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
  /// Patterns where method `name` is overridden in a subclass.
  fn override_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

pub struct NoInheritanceGrammar;
impl InheritanceGrammar for NoInheritanceGrammar {}

pub fn inheritance_grammar_for(lang: SgLang) -> &'static dyn InheritanceGrammar {
  match lang {
    _ => &NoInheritanceGrammar,
  }
}

// ─────────────────────── DecorationGrammar ────────────────────────
//
// §2.3 — D5 fifth capability trait. Returns ast-grep pattern strings
// that match a language's annotation / attribute / decorator
// syntaxes for the requested name. Empty Vec = "this language has
// no decoration grammar wired" (sensor surfaces NOT_IMPLEMENTED).
//
// Same shape as ImportGrammar: stateless, takes `&str name`, returns
// `Vec<String>` patterns. Phase 0 / 1 used a hard-coded match arm
// inside `sensors::decorations`; the trait moves that taxonomy here
// so each language is plugged in alongside its other capabilities.

pub trait DecorationGrammar: Send + Sync {
  fn decoration_patterns(&self, name: &str) -> Vec<String>;
}

pub struct NoDecorationGrammar;
impl DecorationGrammar for NoDecorationGrammar {
  fn decoration_patterns(&self, _: &str) -> Vec<String> {
    Vec::new()
  }
}

pub fn decoration_grammar_for(lang: SgLang) -> &'static dyn DecorationGrammar {
  match lang {
    _ => &NoDecorationGrammar,
  }
}

// ───────────────────────── TraceTemplate ──────────────────────────
//
// §2.4 — D5 sixth capability trait. Emits the language-appropriate
// stderr-tagged template snippet for `trace`'s default substitution.
// Returns `None` when no sensible default exists; the sensor surfaces
// NO_TRACE_TEMPLATE so the caller knows to pass `--template`.
//
// `var` is the metavar name (without `$`) the template should
// reference. The current contract used by `sensors::trace` is to
// pass `"VAR"`, so users write queries like `let $VAR = $RHS` and the
// template substitutes `$VAR` with the bound site.

pub trait TraceTemplate: Send + Sync {
  fn template_for(&self, var: &str) -> Option<String>;
}

pub struct NoTraceTemplate;
impl TraceTemplate for NoTraceTemplate {
  fn template_for(&self, _: &str) -> Option<String> {
    None
  }
}

pub fn trace_template_for(lang: SgLang) -> &'static dyn TraceTemplate {
  match lang {
    _ => &NoTraceTemplate,
  }
}

pub fn call_grammar_for(lang: SgLang) -> &'static dyn CallGrammar {
  match lang {
    _ => &NoCallGrammar,
  }
}

// ───────────────────────── KindTaxonomy ───────────────────────────
//
// Phase 1 §1.4 — D5 third trait. Maps a small **portable** kind set
// (function / method / class / struct / interface / trait / enum /
// type_alias / const / macro / import / route) to language-local
// tree-sitter kind strings. CLI surface:
//   • `--kind <portable>`  — portable lookup (default; new path)
//   • `--raw-kind <ts>`    — tree-sitter literal escape hatch
//   • `--list-kinds <lang>`— enumerate the registry
//
// The taxonomy supersedes Phase 0 §0.5's `declaration_kinds()` —
// `all_declaration_kinds()` rolls up every non-empty portable
// mapping for a language, giving the search walker its default
// kind set without per-language duplication.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PortableKind {
  Function,
  Method,
  Class,
  Struct,
  Interface,
  Trait,
  Enum,
  TypeAlias,
  Const,
  Macro,
  Import,
  Route,
}

impl PortableKind {
  pub const ALL: &'static [PortableKind] = &[
    PortableKind::Function,
    PortableKind::Method,
    PortableKind::Class,
    PortableKind::Struct,
    PortableKind::Interface,
    PortableKind::Trait,
    PortableKind::Enum,
    PortableKind::TypeAlias,
    PortableKind::Const,
    PortableKind::Macro,
    PortableKind::Import,
    PortableKind::Route,
  ];
  pub fn name(self) -> &'static str {
    use PortableKind::*;
    match self {
      Function => "function",
      Method => "method",
      Class => "class",
      Struct => "struct",
      Interface => "interface",
      Trait => "trait",
      Enum => "enum",
      TypeAlias => "type_alias",
      Const => "const",
      Macro => "macro",
      Import => "import",
      Route => "route",
    }
  }
  pub fn parse(s: &str) -> Option<PortableKind> {
    use PortableKind::*;
    Some(match s {
      "function" => Function,
      "method" => Method,
      "class" => Class,
      "struct" => Struct,
      "interface" => Interface,
      "trait" => Trait,
      "enum" => Enum,
      "type_alias" | "type" => TypeAlias,
      "const" | "constant" => Const,
      "macro" => Macro,
      "import" => Import,
      "route" => Route,
      _ => return None,
    })
  }
}

/// Sentinel `KindTaxonomy` for languages whose taxonomy lives in a
/// per-lang crate (`lmprobe-lang-<X>`). Phase 5B fan-out: when a
/// language's `KotlinTaxonomy` etc. moves out of lang-core into
/// its own crate, `lmprobe-registry`'s per-language arm intercepts
/// the dispatch before lang-core's `taxonomy_for` is reached. The
/// catch-all `_ => &NoKindTaxonomy` arm here exists only to keep
/// lang-core's match exhaustive at compile time — it should never
/// fire at runtime if the registry is wired correctly.
pub struct NoKindTaxonomy;
impl KindTaxonomy for NoKindTaxonomy {
  fn kind_for(&self, _portable: PortableKind) -> &'static [&'static str] {
    &[]
  }
}

pub trait KindTaxonomy: Send + Sync {
  /// Map a portable kind to language-local tree-sitter kinds.
  /// Empty = "this language doesn't have this concept".
  fn kind_for(&self, portable: PortableKind) -> &'static [&'static str];

  /// Roll-up of every non-empty portable mapping. Backs the
  /// search walker's `--kind`-less default (replaces §0.5's
  /// `NameExtractor::declaration_kinds()` indirection).
  fn all_declaration_kinds(&self) -> Vec<&'static str> {
    let mut v: Vec<&'static str> = Vec::new();
    for p in PortableKind::ALL {
      for k in self.kind_for(*p) {
        if !v.contains(k) {
          v.push(*k);
        }
      }
    }
    v
  }
}

pub fn taxonomy_for(lang: SgLang) -> &'static dyn KindTaxonomy {
  match lang {
    // Phase 5B fan-out: per-language extractions hit the
    // `lmprobe-registry` per-lang arm BEFORE this dispatch is
    // called. Catch-all keeps the match exhaustive when those
    // langs leave lang-core; should not fire at runtime.
    _ => &NoKindTaxonomy,
  }
}

/// Language tier — used by tier-aware verdict-tracking and the
/// `def --scope code` filter. `Code` = call/import/declaration
/// semantics apply; `Data` = structural-only (search/has-child/
/// outline). JSON/YAML/TOML/XML/HTML/CSS are Data; everything else
/// is Code. Phase 2C §verdict-criteria-refinement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LangTier {
  Code,
  Data,
}

pub fn lang_tier(lang: SgLang) -> LangTier {
  match lang {
    SgLang::Json | SgLang::Yaml | SgLang::Toml | SgLang::Xml | SgLang::Html | SgLang::Css => {
      LangTier::Data
    }
    _ => LangTier::Code,
  }
}

/// Programming-language predicate — used by `def --scope code`.
/// True for languages where "definition" means "code declaration";
/// false for data/markup formats where the concept doesn't fit
/// cleanly. JSON/YAML/TOML/XML/HTML/CSS are data-shaped.
pub fn is_code_lang(lang: SgLang) -> bool {
  matches!(lang_tier(lang), LangTier::Code)
}

// ───────────────────────── TopLevelKinds ──────────────────────────
//
// §2.5 — D5 seventh capability trait. Returns the per-language
// tree-sitter kind names that count as direct-child entries inside a
// structural container (pairs / attributes / child elements). Used
// by `--has-child` to gate the filter to language-meaningful nodes
// instead of arbitrary text containment.
//
// The companion "container kinds" (object/table/element/etc.) are
// intentionally NOT a separate method — they already live in
// `KindTaxonomy::kind_for(PortableKind::Class)` for data languages,
// so users select containers via `--kind class` rather than going
// through this trait directly.

pub trait TopLevelKinds: Send + Sync {
  /// Direct-child entry kinds — what counts as "a child" inside a
  /// container (pairs, attributes, child elements).
  fn child_kinds(&self) -> &'static [&'static str] {
    &[]
  }
}

pub struct NoTopLevelKinds;
impl TopLevelKinds for NoTopLevelKinds {}

pub fn top_level_kinds_for(lang: SgLang) -> &'static dyn TopLevelKinds {
  match lang {
    _ => &NoTopLevelKinds,
  }
}
