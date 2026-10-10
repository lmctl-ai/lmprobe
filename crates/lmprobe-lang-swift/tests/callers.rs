//! Phase 6D.1 — TC-106 family: in-process integration test for the
//! Swift `callers` call-site grammar. Restores cycle-1 behavior (the
//! 5B.7 extraction preserved an empty `NoCallGrammar` for Swift; the
//! battle-test report flagged this as a regression).
//!
//! Pins the per-crate contract for the locked callers doctrine
//! (`durable-memory/04_sensors_graph.md` ## callers, locked at `0557e9fd`):
//! - M1_BARE_CALL: `foo($$$ARGS)` matches `foo(x, y)`.
//! - M2_METHOD_CALL: `$OBJ.foo($$$ARGS)` matches `obj.foo(x)` AND
//!   multi-segment `pkg.mod.foo(x)` (greedy `$OBJ`).
//! - M3_CONSTRUCTOR_NO_NEW: Swift uses no `new` keyword — `Foo(x: 1)`
//!   is the constructor-call form. Same `call_expression` AST as a
//!   bare call, so the bare pattern catches it.
//! - M4_NO_FALSEMATCH: `fooBar(x)` / `obj.fooBar(x)` do NOT match
//!   `callers foo`. Comments and string literals don't match either.
//! - M5_TRAILING_CLOSURE: `foo { x in … }` is matched via the
//!   `{name} { $$$BODY }` pattern. The pattern correctly does NOT
//!   match `if foo { … }` / `while foo { … }` / `for foo in xs { … }`
//!   — tree-sitter-swift differentiates these structurally.
//!
//! Test format mirrors `crates/lmprobe-lang-scala/tests/callers.rs`:
//! tempdir + Pattern + find_all (avoids dev-dep cycle on
//! `lmprobe-engine`).

use ast_grep_core::matcher::Pattern;
use ast_grep_language::LanguageExt;
use lmprobe_lang_core::langs::{CallGrammar, SgLang};
use lmprobe_lang_swift::SwiftCallGrammar;

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
fn patterns_non_empty() {
  let p = SwiftCallGrammar.call_patterns("foo");
  assert!(
    !p.is_empty(),
    "patterns must be non-empty (suppresses NOT_IMPLEMENTED)"
  );
}

#[test]
fn m1_bare_call() {
  // `foo(1, 2)` and `foo()` — bare function calls.
  let content = "func run() { foo(1, 2)\nfoo()\n}\n";
  let p = SwiftCallGrammar.call_patterns("foo");
  assert_eq!(count_matches(content, &p, SgLang::Swift), 2);
}

#[test]
fn m2_method_call() {
  // `obj.foo(x)` — method call.
  let content = "func run() { obj.foo(3)\n}\n";
  let p = SwiftCallGrammar.call_patterns("foo");
  assert_eq!(count_matches(content, &p, SgLang::Swift), 1);
}

#[test]
fn m2_multi_segment_path_method_call() {
  // `pkg.mod.sub.foo(x)` — greedy `$OBJ` captures the whole path.
  let content = "func run() { pkg.mod.sub.foo(4)\nFoo.bar(5)\n}\n";
  let p_foo = SwiftCallGrammar.call_patterns("foo");
  assert_eq!(count_matches(content, &p_foo, SgLang::Swift), 1);
  let p_bar = SwiftCallGrammar.call_patterns("bar");
  assert_eq!(count_matches(content, &p_bar, SgLang::Swift), 1);
}

#[test]
fn m3_constructor_no_new_via_bare_call() {
  // Swift's no-`new` initializer call: `User("alice")` and
  // `User(name: "bob")` are both `call_expression` nodes
  // structurally identical to a bare call. Caught by the bare
  // pattern (no separate `new`-prefixed pattern needed).
  let content = "func run() { let u = User(\"alice\")\nlet v = User(name: \"bob\")\n}\n";
  let p = SwiftCallGrammar.call_patterns("User");
  assert_eq!(count_matches(content, &p, SgLang::Swift), 2);
}

#[test]
fn m3_constructor_capitalized_disambiguated_only_by_convention() {
  // Per doctrine: capitalization is convention; `Foo(args)` is
  // treated as a caller-site for `Foo` regardless of whether it's
  // a constructor or a function-named-the-same-as-a-type.
  let content = "func run() {\n    let a = User(name: \"a\")\n    let b = process(input: 1)\n}\n";
  let p_user = SwiftCallGrammar.call_patterns("User");
  assert_eq!(count_matches(content, &p_user, SgLang::Swift), 1);
  let p_process = SwiftCallGrammar.call_patterns("process");
  assert_eq!(count_matches(content, &p_process, SgLang::Swift), 1);
}

#[test]
fn m4_no_falsematch_on_unrelated_names() {
  // `fooBar` / `barFoo` should NOT match `callers foo`. Identifiers
  // in Swift are matched exactly by ast-grep.
  let content = "func run() {\n    fooBar(1)\n    obj.fooBar(2)\n    barFoo(3)\n    let f = FooBar(x: 1)\n    foo(5)\n}\n";
  let p = SwiftCallGrammar.call_patterns("foo");
  // Only the literal `foo(5)` matches.
  assert_eq!(count_matches(content, &p, SgLang::Swift), 1);
}

#[test]
fn m4_no_falsematch_in_comment_or_string() {
  let content = "func run() {\n    // foo(1)\n    let s = \"foo(2)\"\n    bar()\n}\n";
  let p = SwiftCallGrammar.call_patterns("foo");
  assert_eq!(count_matches(content, &p, SgLang::Swift), 0);
}

#[test]
fn m5_trailing_closure() {
  // `foo { x in print(x) }` — Swift trailing-closure invocation.
  // Caught by the `{name} { $$$BODY }` pattern.
  let content = "func run() {\n    foo { x in print(x) }\n}\n";
  let p = SwiftCallGrammar.call_patterns("foo");
  assert_eq!(count_matches(content, &p, SgLang::Swift), 1);
}

#[test]
fn m5_trailing_closure_no_falsematch_on_control_flow() {
  // `if foo { … }` / `while foo { … }` / `for foo in xs { … }` are
  // NOT trailing-closure invocations — tree-sitter-swift
  // differentiates the structural shapes. The trailing-closure
  // pattern correctly returns 0 hits for these.
  let content = "func run() {\n    if foo {\n        print(\"a\")\n    }\n    while foo {\n        print(\"b\")\n    }\n    for foo in xs {\n        print(foo)\n    }\n}\n";
  let p = SwiftCallGrammar.call_patterns("foo");
  // No legitimate caller-site for `foo` in this fixture.
  assert_eq!(count_matches(content, &p, SgLang::Swift), 0);
}

#[test]
fn m5_higher_order_method_arg_passing() {
  // `xs.map(foo)` — passing `foo` as a callable IS a caller-site
  // per locked doctrine. Caught by `$X.$M(foo)` pattern.
  let content = "func run() {\n    xs.map(foo)\n    xs.forEach(foo)\n    ys.filter(bar)\n}\n";
  let p = SwiftCallGrammar.call_patterns("foo");
  assert_eq!(count_matches(content, &p, SgLang::Swift), 2);
}

#[test]
fn m5_higher_order_blocked_for_bare_arg() {
  // DOCUMENTED LIMITATION: bare-arg `bar(foo)` is NOT matched
  // (would over-match every single-arg call). The canonical
  // `xs.method(foo)` shape is matched; non-method bare passes are
  // intentionally blocked.
  let content = "func run() {\n    bar(foo)\n    Task(foo)\n    println(foo)\n}\n";
  let p = SwiftCallGrammar.call_patterns("foo");
  assert_eq!(count_matches(content, &p, SgLang::Swift), 0);
}

#[test]
fn callee_decls_match_typed_func() {
  let content = "class C {\n    func parse(s: String) -> Int { return 0 }\n}\n";
  let p = SwiftCallGrammar.callee_decl_patterns("parse");
  assert!(count_matches(content, &p, SgLang::Swift) >= 1);
}

#[test]
fn callee_decls_match_bare_func() {
  let content = "class C {\n    func parse() { print(\"hi\") }\n}\n";
  let p = SwiftCallGrammar.callee_decl_patterns("parse");
  assert!(count_matches(content, &p, SgLang::Swift) >= 1);
}

#[test]
fn callee_decls_match_public_and_private() {
  let content = "class C {\n    public func parse(s: String) -> Int { return 0 }\n    private func parse2() { }\n}\n";
  let p1 = SwiftCallGrammar.callee_decl_patterns("parse");
  let p2 = SwiftCallGrammar.callee_decl_patterns("parse2");
  assert!(count_matches(content, &p1, SgLang::Swift) >= 1);
  assert!(count_matches(content, &p2, SgLang::Swift) >= 1);
}

#[test]
fn callee_decls_match_override_and_static() {
  let content =
    "class C {\n    override func update() { }\n    static func make() -> C { return C() }\n}\n";
  let p_update = SwiftCallGrammar.callee_decl_patterns("update");
  let p_make = SwiftCallGrammar.callee_decl_patterns("make");
  assert!(count_matches(content, &p_update, SgLang::Swift) >= 1);
  assert!(count_matches(content, &p_make, SgLang::Swift) >= 1);
}

#[test]
fn callee_decls_match_protocol_abstract() {
  // Protocol method (no body) — `func {name}($$$) -> $RET` and
  // `func {name}($$$)` patterns match.
  let content = "protocol P {\n    func parse(s: String) -> Int\n    func reset()\n}\n";
  let p_parse = SwiftCallGrammar.callee_decl_patterns("parse");
  let p_reset = SwiftCallGrammar.callee_decl_patterns("reset");
  assert!(count_matches(content, &p_parse, SgLang::Swift) >= 1);
  assert!(count_matches(content, &p_reset, SgLang::Swift) >= 1);
}

#[test]
fn all_shapes_in_one_file() {
  // Synthetic fixture exercising every locked shape — pins the
  // full 5-marker matrix in one file.
  let content = "import Foundation\n\nclass User {\n    let name: String\n    init(name: String) { self.name = name }\n    func greet() -> String { return name }\n}\n\nfunc run() {\n    let u = User(name: \"alice\")     // M3 no-new ctor\n    u.greet()                          // M2 method call\n    pkg.mod.greet()                    // M2 multi-segment\n    greet()                            // M1 bare\n    xs.map(greet)                      // M5 higher-order\n    greet { ctx in print(ctx) }        // M5 trailing closure\n    // greet(in_comment)                // M4 no-falsematch\n    let s = \"greet(in_string)\"        // M4 no-falsematch\n    greetBar()                         // M4 no-falsematch (different name)\n    if greet {                         // NOT a caller-site\n        print(\"flag\")\n    }\n}\n";

  let p_user = SwiftCallGrammar.call_patterns("User");
  // 1× no-new ctor (no method named User, no higher-order, no trailing).
  assert_eq!(count_matches(content, &p_user, SgLang::Swift), 1);

  let p_greet = SwiftCallGrammar.call_patterns("greet");
  // u.greet() + pkg.mod.greet() + greet() + xs.map(greet) +
  // greet { … } = 5. Comment / string / `if greet { … }` and
  // `greetBar()` do NOT match.
  assert_eq!(count_matches(content, &p_greet, SgLang::Swift), 5);
}
