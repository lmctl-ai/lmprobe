//! Phase 6C round-3 — in-process integration tests for Go `import`
//! patterns. Round-3 collapsed the 9 round-1 position-specific
//! patterns to 2 broad patterns (`$X "$P"` + `import "$P"`) plus the
//! engine's `needs_prefix_match` filter (designer-arbitrated). Tests
//! mimic the engine path: apply patterns + filter by
//! `content.contains(name)`.

use ast_grep_core::matcher::Pattern;
use ast_grep_language::LanguageExt;
use lmprobe_lang_core::langs::{ImportGrammar, SgLang};
use lmprobe_lang_go::GoImportGrammar;

/// Engine-equivalent helper: apply patterns + post-match content
/// filter + per-file `(line, content)` dedup + Phase 6C R4 block-form
/// per-spec DFS. Mirrors `crates/lmprobe-engine/src/sensors.rs::imports`
/// when `needs_prefix_match() == true`.
fn count_with_filter(content: &str, name: &str) -> usize {
  use ast_grep_core::Node as _;
  use std::collections::HashSet;
  let lang = SgLang::Go;
  let g = GoImportGrammar;
  let patterns = g.import_patterns(name);
  let needs_filter = g.needs_prefix_match();
  let grep = lang.ast_grep(content);
  let root_node = grep.root();
  let mut seen: HashSet<(u32, String)> = HashSet::new();
  let mut total = 0;
  for pattern_str in &patterns {
    let pat = match Pattern::try_new(pattern_str, lang) {
      Ok(p) => p,
      Err(_) => continue,
    };
    for m in root_node.find_all(pat) {
      let text = m.text().to_string();
      if needs_filter && !text.contains(name) {
        continue;
      }
      let line = (m.start_pos().line() + 1) as u32;
      if seen.insert((line, text)) {
        total += 1;
      }
    }
  }
  // R4 — block-form per-spec DFS.
  if needs_filter {
    if let Some((spec_kind, parent_kind)) = g.block_form_spec_kind() {
      for n in root_node.dfs() {
        if n.kind() != spec_kind {
          continue;
        }
        if n.parent().map(|p| p.kind().to_string()) != Some(parent_kind.to_string()) {
          continue;
        }
        let text = n.text().to_string();
        if !text.contains(name) {
          continue;
        }
        let line = (n.start_pos().line() + 1) as u32;
        if seen.insert((line, text)) {
          total += 1;
        }
      }
    }
  }
  total
}

#[test]
fn patterns_non_empty() {
  // Suppresses NOT_IMPLEMENTED in the engine sensor.
  let p = GoImportGrammar.import_patterns("fmt");
  assert!(!p.is_empty());
}

#[test]
fn needs_prefix_match_is_true_round3() {
  assert!(GoImportGrammar.needs_prefix_match());
}

#[test]
fn round4_pattern_set_simplified() {
  // Phase 6C R4: position-specific block-form patterns dropped —
  // engine's `block_form_spec_kind` DFS replaces them. Pattern set
  // reduces to 4: 2 exact-match (single bare + aliased) + 2 broad.
  let p = GoImportGrammar.import_patterns("fmt");
  assert_eq!(p.len(), 4);
  assert!(p.iter().any(|s| s == "$X \"$P\""));
  assert!(p.iter().any(|s| s == "import \"$P\""));
  assert!(p.iter().any(|s| s == "import \"fmt\""));
}

#[test]
fn block_form_spec_kind_round4() {
  assert_eq!(
    GoImportGrammar.block_form_spec_kind(),
    Some(("import_spec", "import_spec_list"))
  );
}

#[test]
fn block_form_prefix_match_round4() {
  // R4 PASS bar M2_PFX: `imports github.com/spf13` matches
  // `"github.com/spf13/pflag"` inside a block-form list.
  let content = r#"package x
import (
	"github.com/spf13/pflag"
	"fmt"
)
"#;
  assert_eq!(count_with_filter(content, "github.com/spf13"), 1);
  assert_eq!(count_with_filter(content, "github.com/spf13/pflag"), 1);
}

#[test]
fn block_form_basename_match_round4() {
  // R4 PASS bar M3_BARE: `imports pflag` matches
  // `"github.com/spf13/pflag"` via basename substring.
  let content = r#"package x
import (
	"github.com/spf13/pflag"
)
"#;
  assert_eq!(count_with_filter(content, "pflag"), 1);
}

#[test]
fn single_form_exact_match() {
  let content = "package x\n\nimport \"fmt\"\n";
  assert_eq!(count_with_filter(content, "fmt"), 1);
}

#[test]
fn single_form_basename_match_round3() {
  // Round-3: `imports fmt` matches `import "encoding/fmt"` via
  // post-match `content.contains("fmt")`.
  let content = "package x\nimport \"encoding/fmt\"\n";
  assert_eq!(count_with_filter(content, "fmt"), 1);
}

#[test]
fn block_form_single_spec_matches() {
  let content = "package x\nimport (\n\t\"fmt\"\n)\n";
  assert_eq!(count_with_filter(content, "fmt"), 1);
}

#[test]
fn block_form_multi_spec_matches_target_at_any_position() {
  // Position 1, 2, 3, …, last — all should match the broad pattern.
  let content = r#"package x
import (
	"bytes"
	"context"
	"fmt"
	"io"
	"os"
)
"#;
  // `imports fmt` should hit position 3.
  assert_eq!(count_with_filter(content, "fmt"), 1);
  // `imports os` should hit position 5.
  assert_eq!(count_with_filter(content, "os"), 1);
  // `imports bytes` should hit position 1.
  assert_eq!(count_with_filter(content, "bytes"), 1);
}

#[test]
fn block_form_aliased_target_matches() {
  // Single-form aliased: `import _ "x"`, `import . "x"`,
  // `import alias "x"`. All should match.
  let content = r#"package x
import (
	_ "github.com/foo/bar"
	. "math"
	myalias "fmt"
)
"#;
  assert_eq!(count_with_filter(content, "github.com/foo/bar"), 1);
  assert_eq!(count_with_filter(content, "math"), 1);
  assert_eq!(count_with_filter(content, "fmt"), 1);
}

#[test]
fn no_falsematch_unrelated_name() {
  let content = r#"package x
import (
	"fmt"
	"os"
)
"#;
  assert_eq!(count_with_filter(content, "missing"), 0);
}

#[test]
fn substring_false_positive_documented() {
  // Round-3 doctrine call: simple `content.contains(name)`.
  // `imports fmt` matches `import "fmtutil"` because "fmtutil"
  // contains "fmt". This is the documented tradeoff vs the more
  // precise but more complex path-aware regex approach.
  let content = "package x\nimport \"fmtutil\"\n";
  assert_eq!(count_with_filter(content, "fmt"), 1);
}

#[test]
fn deep_path_matches_via_basename() {
  // `imports fmt` matches deep paths ending with `/fmt` via the
  // round-3 substring filter.
  let content = "package x\nimport \"deep/path/to/fmt\"\n";
  assert_eq!(count_with_filter(content, "fmt"), 1);
}

#[test]
fn cobra_style_block_realistic() {
  // Excerpt of cobra's command.go import block (12 specs).
  let content = r#"package cobra
import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"reflect"
	"sort"
	"strconv"
	"strings"

	flag "github.com/spf13/pflag"
)
"#;
  // Each unique name should hit exactly once (no double-counting
  // from the 2 round-3 patterns; the broad `$X "$P"` matches
  // every import_spec, but the single-form `import "$P"` doesn't
  // match block-form specs — they live in different parents).
  assert_eq!(count_with_filter(content, "fmt"), 1);
  assert_eq!(count_with_filter(content, "github.com/spf13/pflag"), 1);
  assert_eq!(count_with_filter(content, "os"), 1);
}

#[test]
fn no_falsematch_string_outside_import() {
  // String literals that aren't import specs shouldn't match.
  let content = r#"package x
import "fmt"
const greeting = "hello fmt world"
func f() string { return "fmt is great" }
"#;
  // Only the actual `import "fmt"` should match.
  assert_eq!(count_with_filter(content, "fmt"), 1);
}
