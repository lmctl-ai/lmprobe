//! Phase 6C round-2 — in-process integration test for HCL/Terraform
//! `module "X" { ... }` import patterns. Round-1 anchored on the
//! source path; round-2 re-anchors on the **block name** per PM
//! doctrine. Single pattern → no dedupe.
//!
//! Doctrine: `imports vpc /tree` matches `module "vpc" { source = ...
//! }`. Source is metadata; users querying source strings fall to
//! `grep`/`search`.

use ast_grep_core::matcher::Pattern;
use ast_grep_language::LanguageExt;
use lmprobe_lang_core::langs::{ImportGrammar, SgLang};
use lmprobe_lang_hcl::HclImportGrammar;

fn count_matches(source: &str, pattern_str: &str) -> usize {
  let lang = SgLang::Hcl;
  let pat = match Pattern::try_new(pattern_str, lang) {
    Ok(p) => p,
    Err(_) => return 0,
  };
  let grep = lang.ast_grep(source);
  grep.root().find_all(pat).count()
}

fn count_imports(source: &str, name: &str) -> usize {
  let g = HclImportGrammar;
  let patterns = g.import_patterns(name);
  patterns
    .iter()
    .map(|p| count_matches(source, p))
    .sum::<usize>()
}

#[test]
fn patterns_returns_dual_anchor_round3() {
  // Phase 6C round-3: dual-anchor — block-name AND source-path.
  // 2 patterns; natural disjointness preserves "1 hit per block".
  let g = HclImportGrammar;
  let p = g.import_patterns("vpc");
  assert_eq!(p.len(), 2, "round-3: block-name + source-path");
}

#[test]
fn module_block_anchor_matches_block_name() {
  // imports vpc /tree → matches `module "vpc" { ... }`.
  let src = r#"
module "vpc" {
  source = "./modules/vpc"
  region = "us-east-1"
}
"#;
  assert_eq!(count_imports(src, "vpc"), 1);
}

#[test]
fn module_block_anchor_does_not_match_other_block_names() {
  let src = r#"
module "eks" {
  source = "../modules/eks"
}
"#;
  assert_eq!(count_imports(src, "vpc"), 0);
}

#[test]
fn dedupe_one_hit_per_block_round2() {
  // Round-1 emitted 2 hits per block (block-form + direct-attr
  // patterns both fired). Round-2 ships a single pattern.
  let src = r#"
module "vpc" {
  source = "./modules/vpc"
}
"#;
  let count = count_imports(src, "vpc");
  assert_eq!(
    count, 1,
    "exactly 1 hit per module block — no dedupe overlap"
  );
}

#[test]
fn module_block_with_multi_attrs_matches_via_lenient_first_attr() {
  // tree-sitter-hcl's lenient sibling matching: `module "X" {
  // source = $SRC }` matches a block where `source` is the
  // first attr, regardless of trailing attrs. Terraform
  // convention puts source first; verified across qa1 fixtures.
  let src = r#"
module "vpc" {
  source  = "./modules/vpc"
  region  = "us-east-1"
  cidr    = "10.0.0.0/16"
}
"#;
  assert_eq!(count_imports(src, "vpc"), 1);
}

#[test]
fn module_block_with_git_source_matches_by_block_name() {
  let src = r#"
module "consul" {
  source = "git::https://example.com/vpc.git?ref=v1.2.0"
}
"#;
  assert_eq!(count_imports(src, "consul"), 1);
}

#[test]
fn module_block_with_registry_source_matches_by_block_name() {
  let src = r#"
module "vpc" {
  source = "terraform-aws-modules/vpc/aws"
  version = "5.0.0"
}
"#;
  assert_eq!(count_imports(src, "vpc"), 1);
}

#[test]
fn provider_source_attr_does_not_match_anymore() {
  // Round-1 matched the `source = "hashicorp/aws"` direct-attr
  // pattern even outside module blocks. Round-2 drops that
  // pattern; users query source strings via grep/search.
  let src = r#"
terraform {
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 4.0"
    }
  }
}
"#;
  assert_eq!(
    count_imports(src, "aws"),
    0,
    "round-2 doesn't match nested provider blocks via source attr"
  );
  assert_eq!(
    count_imports(src, "hashicorp/aws"),
    0,
    "round-2 doesn't match by source path either"
  );
}

#[test]
fn real_qa1_fixture_terraform_aws_vpc() {
  // Synthetic excerpt mirroring qa1/repos/hcl/terraform-aws-vpc
  // shape. Multiple module blocks, each with source as first attr.
  let src = r#"
module "flow-log" {
  source = "../../modules/flow-log"
  vpc_id = aws_vpc.this.id
}

module "subnets" {
  source = "../../modules/subnets"
}
"#;
  assert_eq!(count_imports(src, "flow-log"), 1);
  assert_eq!(count_imports(src, "subnets"), 1);
  // Other names don't match.
  assert_eq!(count_imports(src, "this"), 0);
}

#[test]
fn empty_block_does_not_match() {
  // `module "x" { }` with no source attr doesn't match the
  // round-2 pattern (which requires `source = $SRC`).
  let src = r#"
module "empty" {
}
"#;
  assert_eq!(count_imports(src, "empty"), 0);
}

#[test]
fn no_module_blocks_no_hits() {
  let src = r#"
resource "aws_vpc" "main" {
  cidr_block = "10.0.0.0/16"
}
"#;
  assert_eq!(count_imports(src, "main"), 0);
}

#[test]
fn patterns_round3_dual_anchor() {
  // Round-3: 2 patterns interpolate {name} into both block-name
  // and source-path slots.
  let g = HclImportGrammar;
  let p = g.import_patterns("VPC_BLOCK");
  assert_eq!(p.len(), 2);
  assert!(
    p.iter().any(|s| s.contains("\"VPC_BLOCK\" {")),
    "block-name anchor missing: {p:?}"
  );
  assert!(
    p.iter().any(|s| s.contains("source = \"VPC_BLOCK\"")),
    "source-path anchor missing: {p:?}"
  );
}

#[test]
fn source_path_anchor_matches_round3() {
  // Round-3 dual-anchor: querying by source path.
  let src = r#"
module "vpc" {
  source = "./modules/vpc"
}
"#;
  assert_eq!(count_imports(src, "./modules/vpc"), 1);
  assert_eq!(count_imports(src, "../missing/path"), 0);
}
