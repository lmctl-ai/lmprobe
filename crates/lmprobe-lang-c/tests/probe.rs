//! Diagnostic probe — temp file, deleted before commit.

use ast_grep_core::matcher::Pattern;
use ast_grep_language::LanguageExt;
use lmprobe_lang_core::langs::SgLang;

fn dump<'a>(n: &ast_grep_core::Node<'a, ast_grep_core::tree_sitter::StrDoc<SgLang>>, depth: usize) {
  let pad = "  ".repeat(depth);
  let kind_owned = n.kind();
  let kind: &str = kind_owned.as_ref();
  let txt = n.text();
  let txt: String = txt.chars().take(60).collect();
  println!("{pad}{kind}: {:?}", txt);
  for c in n.children() {
    dump(&c, depth + 1);
  }
}

#[test]
fn probe_bare_call_pattern_kind() {
  let lang = SgLang::C;
  let pat_str = "decode_array_index_from_pointer($$$ARGS)";
  println!("\n--- pattern as standalone source ---");
  let grep = lang.ast_grep(pat_str);
  dump(&grep.root(), 0);

  println!("\n--- match against call site ---");
  let content = r#"int main(void) {
    if (!decode_array_index_from_pointer(child_pointer, &index)) {
        return 1;
    }
    return 0;
}
"#;
  let grep2 = lang.ast_grep(content);
  dump(&grep2.root(), 0);

  println!("\n--- finding matches with bare pattern ---");
  let pat = Pattern::try_new(pat_str, lang).unwrap();
  let hits: Vec<_> = grep2.root().find_all(pat).collect();
  println!("hits: {}", hits.len());

  println!("\n--- AST of variant with semicolon ---");
  let g = lang.ast_grep("decode_array_index_from_pointer($$$ARGS);");
  dump(&g.root(), 0);

  println!("\n--- variants ---");
  for variant in &[
    "decode_array_index_from_pointer($$$ARGS);",
    "decode_array_index_from_pointer($$$ARGS)\n",
    "!decode_array_index_from_pointer($$$ARGS)",
    "if (decode_array_index_from_pointer($$$ARGS)) { }",
    "$_ = decode_array_index_from_pointer($$$ARGS);",
    "int _x = decode_array_index_from_pointer($$$ARGS);",
  ] {
    let p = match Pattern::try_new(variant, lang) {
      Ok(p) => p,
      Err(e) => {
        println!("variant={variant:?} PARSE FAIL: {:?}", e);
        continue;
      }
    };
    let n: usize = grep2.root().find_all(p).count();
    println!("variant={variant:?}  hits={n}");
  }
}
