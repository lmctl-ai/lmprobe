//! Phase 6D.1 — TC-106 family: in-process integration test for the
//! Scala `callers` call-site grammar. Restores cycle-1 behavior (the
//! 5B.6 extraction preserved an empty `NoCallGrammar` for Scala; the
//! battle-test report flagged this as a regression).
//!
//! Pins the per-crate contract for the locked callers doctrine
//! (`durable-memory/04_sensors_graph.md` ## callers, locked at `0557e9fd`):
//! - M1_BARE_CALL: `foo($$$ARGS)` matches `foo(x, y)` (incl. no-new
//!   case-class apply `Foo(x)` — same `call_expression` AST).
//! - M2_METHOD_CALL: `$OBJ.foo($$$ARGS)` matches `obj.foo(x)` AND
//!   multi-segment `pkg.mod.foo(x)` (greedy `$OBJ`).
//! - M3_CONSTRUCTOR: `new Foo($$$ARGS)` matches `new Foo(x)`; the
//!   bare `Foo($$$ARGS)` pattern handles no-new `Foo(x)`.
//! - M4_NO_FALSEMATCH: `fooBar(x)` / `obj.fooBar(x)` do NOT match
//!   `imports/callers foo`.
//! - M5_HIGHER_ORDER: `xs.map(foo)` matches via `$X.$M(foo)` (canonical
//!   higher-order shape). Bare-arg `bar(foo)` is intentionally
//!   excluded — would over-match every single-arg call.
//!
//! Test format mirrors `crates/lmprobe-lang-go/tests/imports.rs` /
//! `crates/lmprobe-lang-php/tests/imports.rs`: tempdir + Pattern +
//! find_all (avoids dev-dep cycle on `lmprobe-engine`).

use ast_grep_core::matcher::Pattern;
use ast_grep_language::LanguageExt;
use lmprobe_lang_core::langs::{CallGrammar, SgLang};
use lmprobe_lang_scala::ScalaCallGrammar;

/// Total match count across all patterns for `name` in `content`.
/// Mirrors the engine's per-pattern emit loop in `sensors::callers`.
fn count_matches(content: &str, patterns: &[String], lang: SgLang) -> usize {
  let grep = lang.ast_grep(content);
  let root_node = grep.root();
  let mut total = 0;
  for pattern_str in patterns {
    let pat = match Pattern::try_new(pattern_str, lang) {
      Ok(p) => p,
      Err(_) => continue, // silent drop on parse fail (mirrors engine)
    };
    total += root_node.find_all(pat).count();
  }
  total
}

#[test]
fn m1_bare_call() {
  // `foo(1, 2)` — bare function call; same `call_expression` AST
  // as no-new constructor `Foo(args)`.
  let content = "object App { foo(1, 2)\nfoo()\n}\n";
  let p = ScalaCallGrammar.call_patterns("foo");
  assert!(
    !p.is_empty(),
    "patterns non-empty (suppresses NOT_IMPLEMENTED)"
  );
  assert_eq!(count_matches(content, &p, SgLang::Scala), 2);
}

#[test]
fn m1_no_new_constructor_via_bare_call() {
  // Scala's case-class `apply` makes `Foo(x)` AST-identical to a
  // bare call; the bare pattern catches both.
  let content = "object App { val u = User(\"alice\", 30)\n}\n";
  let p = ScalaCallGrammar.call_patterns("User");
  assert_eq!(
    count_matches(content, &p, SgLang::Scala),
    1,
    "no-new ctor `User(...)` matched via bare pattern"
  );
}

#[test]
fn m2_method_call() {
  // `obj.foo(x)` — method call.
  let content = "object App { obj.foo(3)\n}\n";
  let p = ScalaCallGrammar.call_patterns("foo");
  assert_eq!(count_matches(content, &p, SgLang::Scala), 1);
}

#[test]
fn m2_multi_segment_path_method_call() {
  // `pkg.mod.sub.foo(x)` — greedy `$OBJ` captures the whole path.
  // Scala uses `.` (not `::`); no separate path-prefixed pattern
  // needed.
  let content = "object App { pkg.mod.sub.foo(4)\nFoo.bar(5)\n}\n";
  let p_foo = ScalaCallGrammar.call_patterns("foo");
  assert_eq!(count_matches(content, &p_foo, SgLang::Scala), 1);
  let p_bar = ScalaCallGrammar.call_patterns("bar");
  assert_eq!(count_matches(content, &p_bar, SgLang::Scala), 1);
}

#[test]
fn m3_constructor_explicit_new() {
  // `new Foo(x)` — explicit-`new` ctor parses as `instance_expression`,
  // distinct from bare `call_expression`. Needs its own pattern.
  let content = "object App { val u = new User(\"alice\")\n}\n";
  let p = ScalaCallGrammar.call_patterns("User");
  // Bare `User($$$)` won't match `new User(...)` (different AST);
  // the `new User($$$)` pattern catches it.
  assert_eq!(count_matches(content, &p, SgLang::Scala), 1);
}

#[test]
fn m3_constructor_no_new_case_class() {
  // Case-class style: `Foo(args)` (no `new`). Same AST as bare call.
  let content = "object App { val u = User(\"alice\", 30)\nuser.print()\n}\n";
  let p = ScalaCallGrammar.call_patterns("User");
  assert_eq!(count_matches(content, &p, SgLang::Scala), 1);
}

#[test]
fn m3_constructor_both_forms_in_one_file() {
  let content =
    "object App {\n  val a = new User(\"alice\")\n  val b = User(\"bob\")\n  obj.User(\"x\")\n}\n";
  let p = ScalaCallGrammar.call_patterns("User");
  // Expects: 1× new-ctor + 1× no-new ctor + 1× method named `User`.
  // Total = 3 across the bare / method / new-ctor patterns.
  assert_eq!(count_matches(content, &p, SgLang::Scala), 3);
}

#[test]
fn m4_no_falsematch_on_unrelated_names() {
  // `fooBar` / `barFoo` should NOT match `callers foo`. Identifiers
  // in Scala are matched exactly by ast-grep.
  let content =
    "object App {\n  fooBar(1)\n  obj.fooBar(2)\n  barFoo(3)\n  new FooBar(4)\n  foo(5)\n}\n";
  let p = ScalaCallGrammar.call_patterns("foo");
  // Only the literal `foo(5)` matches.
  assert_eq!(count_matches(content, &p, SgLang::Scala), 1);
}

#[test]
fn m4_no_falsematch_in_comment_or_string() {
  let content = "object App {\n  // foo(1)\n  val s = \"foo(2)\"\n  bar()\n}\n";
  let p = ScalaCallGrammar.call_patterns("foo");
  assert_eq!(count_matches(content, &p, SgLang::Scala), 0);
}

#[test]
fn m5_higher_order_method_arg_passing() {
  // `xs.map(foo)` — passing `foo` as a callable IS a caller-site
  // per locked doctrine. Caught by `$X.$M(foo)` pattern.
  let content = "object App {\n  xs.map(foo)\n  xs.foreach(foo)\n  ys.filter(bar)\n}\n";
  let p = ScalaCallGrammar.call_patterns("foo");
  assert_eq!(
    count_matches(content, &p, SgLang::Scala),
    2,
    "xs.map(foo) + xs.foreach(foo)"
  );
}

#[test]
fn m5_higher_order_blocked_for_bare_arg() {
  // DOCUMENTED LIMITATION: bare-arg `bar(foo)` and `Future(foo)` are
  // NOT matched, because adding `$X(foo)` would over-match every
  // single-arg call. The canonical `xs.method(foo)` shape is
  // matched; non-method bare passes are intentionally blocked.
  let content = "object App {\n  bar(foo)\n  Future(foo)\n  println(foo)\n}\n";
  let p = ScalaCallGrammar.call_patterns("foo");
  assert_eq!(
    count_matches(content, &p, SgLang::Scala),
    0,
    "bare-arg passing intentionally not matched"
  );
}

#[test]
fn callee_decls_match_concrete_typed_def() {
  let content = "class C {\n  def parse(s: String): Int = 0\n}\n";
  let p = ScalaCallGrammar.callee_decl_patterns("parse");
  assert!(count_matches(content, &p, SgLang::Scala) >= 1);
}

#[test]
fn callee_decls_match_abstract_typed_def() {
  let content = "trait T {\n  def parse(s: String): Int\n}\n";
  let p = ScalaCallGrammar.callee_decl_patterns("parse");
  assert!(count_matches(content, &p, SgLang::Scala) >= 1);
}

#[test]
fn callee_decls_match_override_def() {
  let content = "class C extends T {\n  override def parse(s: String): Int = 0\n}\n";
  let p = ScalaCallGrammar.callee_decl_patterns("parse");
  assert!(count_matches(content, &p, SgLang::Scala) >= 1);
}

#[test]
fn callee_decls_match_no_paren_def() {
  // `def name: T = expr` — Scala's no-paren val-style def.
  let content = "object O {\n  def parse: Int = 42\n}\n";
  let p = ScalaCallGrammar.callee_decl_patterns("parse");
  assert!(count_matches(content, &p, SgLang::Scala) >= 1);
}

#[test]
fn callee_decls_match_generic_and_curried() {
  let content =
    "object O {\n  def parse[A](x: A): A = x\n  def fold(z: Int)(f: Int => Int): Int = z\n}\n";
  let p_parse = ScalaCallGrammar.callee_decl_patterns("parse");
  let p_fold = ScalaCallGrammar.callee_decl_patterns("fold");
  assert!(count_matches(content, &p_parse, SgLang::Scala) >= 1);
  assert!(count_matches(content, &p_fold, SgLang::Scala) >= 1);
}

#[test]
fn all_shapes_in_one_file() {
  // Synthetic fixture exercising every locked shape — pins the full
  // 5-marker matrix in one file.
  let content = "package myapp\n\nclass User(name: String) {\n  def greet(): String = name\n}\n\nobject App {\n  val u1 = new User(\"alice\")     // M3 explicit-new\n  val u2 = User(\"bob\")           // M3 no-new (case-class apply)\n  u1.greet()                       // M2 method call\n  pkg.mod.greet()                  // M2 multi-segment\n  greet()                          // M1 bare\n  xs.map(greet)                    // M5 higher-order\n  // greet(in_comment)              // M4 no-falsematch\n  val s = \"greet(in_string)\"      // M4 no-falsematch\n  greetBar()                       // M4 no-falsematch (different name)\n}\n";

  let p_user = ScalaCallGrammar.call_patterns("User");
  // 1× new-ctor + 1× no-new ctor = 2 (no User method or HO usage).
  assert_eq!(count_matches(content, &p_user, SgLang::Scala), 2);

  let p_greet = ScalaCallGrammar.call_patterns("greet");
  // u1.greet() + pkg.mod.greet() + greet() + xs.map(greet) = 4.
  // Comment / string forms do NOT match.
  assert_eq!(count_matches(content, &p_greet, SgLang::Scala), 4);
}
