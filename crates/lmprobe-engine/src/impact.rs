//! Phase 3B §3.2 — `lmprobe impact <symbol>`.
//!
//! Recursive callers/callees traversal with depth-banded confidence
//! labels. The DAG is bounded by `--depth` (default 3, hard cap 10).
//!
//! Confidence bands per Lead arbitration:
//!   • depth 1   → "high"
//!   • depth 2-5 → "medium"
//!   • depth >5  → "low"

use std::collections::{HashSet, VecDeque};
use std::path::Path;

use crate::sensors::WalkOpts;
use lmprobe_types::error::Result;
use lmprobe_types::schema::{Diagnostic, ImpactEdge, ImpactNode, ImpactResult, SymbolHit};

/// Direction of `impact` BFS expansion.
///
/// Phase 5A workspace split: lives here in engine since impact is
/// the only sensor that takes it. Binary's clap CLI defines a
/// thin wrapper enum + `clap::ValueEnum` derive + conversion;
/// async-graphql `Enum` derive is here directly since the
/// GraphQL surface uses this type as a resolver arg.
#[derive(Clone, Copy, Debug, PartialEq, Eq, async_graphql::Enum)]
pub enum ImpactDirection {
  /// "Who calls me?" — recurse callers.
  Upstream,
  /// "What do I call?" — recurse callees.
  Downstream,
}

/// Local impact-only file-class filter. This is intentionally not a
/// repo-wide path classifier; it exists to trim impact DAG noise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, async_graphql::Enum)]
pub enum ImpactFileFilter {
  All,
  TestsOnly,
  ExcludeTests,
}

const HARD_DEPTH_CAP: u32 = 10;

fn confidence_for(depth: u32) -> &'static str {
  match depth {
    0 | 1 => "high",
    2..=5 => "medium",
    _ => "low",
  }
}

/// Run callers (upstream) or callees (downstream) at depth N from
/// origin. BFS expands one frontier per hop; cycle protection via a
/// visited set keyed on `name`.
pub fn impact(
  origin: &str,
  root: &Path,
  direction: ImpactDirection,
  depth: u32,
  file_filter: ImpactFileFilter,
  opts: &WalkOpts,
) -> Result<ImpactResult> {
  let max_depth = depth.min(HARD_DEPTH_CAP);

  let mut nodes: Vec<ImpactNode> = Vec::new();
  let mut edges: Vec<ImpactEdge> = Vec::new();
  let mut visited: HashSet<String> = HashSet::new();
  let mut queue: VecDeque<(String, u32)> = VecDeque::new();
  queue.push_back((origin.to_string(), 0));
  visited.insert(origin.to_string());

  while let Some((name, d)) = queue.pop_front() {
    if d >= max_depth {
      continue;
    }
    let next_hits: Vec<SymbolHit> = match direction {
      ImpactDirection::Upstream => crate::sensors::callers(&name, root, opts)?.hits,
      ImpactDirection::Downstream => crate::graph_sensors::callees(&name, root, opts)?.hits,
    };

    for hit in next_hits {
      // Heuristic: extract the surrounding declaration's name as
      // the upstream node. The current `callers` envelope ships
      // `SymbolHit::name = <queried name>` (the called symbol),
      // not the caller's own name. For 3B we approximate by
      // tagging the call-site location and walking on the called
      // text — a faithful caller-name extraction is the
      // §3.2-extension Phase-4 follow-up.
      let target_name = match direction {
        ImpactDirection::Upstream => extract_caller_decl_name(&hit, &name)
          .unwrap_or_else(|| format!("{}@{}:{}", name, hit.file_path, hit.line_start)),
        ImpactDirection::Downstream => hit.name.clone(),
      };
      if visited.insert(target_name.clone()) {
        nodes.push(ImpactNode {
          name: target_name.clone(),
          kind: hit.kind.clone(),
          file_path: hit.file_path.clone(),
          line_start: hit.line_start,
          depth: d + 1,
          confidence: confidence_for(d + 1).to_string(),
        });
        queue.push_back((target_name.clone(), d + 1));
      }
      let (from, to) = match direction {
        ImpactDirection::Upstream => (target_name, name.clone()),
        ImpactDirection::Downstream => (name.clone(), target_name),
      };
      edges.push(ImpactEdge {
        from,
        to,
        edge_type: match direction {
          ImpactDirection::Upstream => "calls".into(),
          ImpactDirection::Downstream => "calls".into(),
        },
      });
    }
  }

  apply_file_filter(origin, file_filter, &mut nodes, &mut edges);

  let direction_str = match direction {
    ImpactDirection::Upstream => "upstream".to_string(),
    ImpactDirection::Downstream => "downstream".to_string(),
  };
  let mut env = ImpactResult::new(origin.to_string(), direction_str, max_depth, nodes, edges);
  if depth > HARD_DEPTH_CAP {
    env.push_warning(
            Diagnostic::new(
                "IMPACT_DEPTH_CAPPED",
                format!(
                    "requested --depth {} clamped to hard cap {}; deeper traversal is bounded for runtime safety",
                    depth, HARD_DEPTH_CAP
                ),
            )
            .with_hint("split the query or pin a smaller --depth"),
        );
  }
  Ok(env)
}

fn apply_file_filter(
  origin: &str,
  file_filter: ImpactFileFilter,
  nodes: &mut Vec<ImpactNode>,
  edges: &mut Vec<ImpactEdge>,
) {
  if matches!(file_filter, ImpactFileFilter::All) {
    return;
  }
  nodes.retain(|node| match file_filter {
    ImpactFileFilter::All => true,
    ImpactFileFilter::TestsOnly => is_test_path(&node.file_path),
    ImpactFileFilter::ExcludeTests => !is_test_path(&node.file_path),
  });
  let kept: HashSet<String> = nodes.iter().map(|node| node.name.clone()).collect();
  edges.retain(|edge| {
    (edge.from == origin || kept.contains(&edge.from))
      && (edge.to == origin || kept.contains(&edge.to))
  });
}

pub fn is_test_path(path: &str) -> bool {
  let parts: Vec<String> = path
    .split(['/', '\\'])
    .filter(|part| !part.is_empty())
    .map(|part| part.to_ascii_lowercase())
    .collect();
  if parts.iter().any(|part| {
    matches!(
      part.as_str(),
      "test" | "tests" | "__tests__" | "spec" | "specs"
    )
  }) {
    return true;
  }
  let file_name = parts.last().map(String::as_str).unwrap_or("");
  file_name.starts_with("test_")
    || file_name.ends_with("_test.rs")
    || file_name.ends_with("_test.go")
    || file_name.ends_with("_test.py")
    || file_name.ends_with("_test.ts")
    || file_name.ends_with("_test.tsx")
    || file_name.ends_with("_spec.ts")
    || file_name.ends_with("_spec.tsx")
    || file_name.contains(".test.")
    || file_name.contains(".spec.")
}

/// Best-effort: from a call-site hit, peek at the surrounding text to
/// find the enclosing declaration's identifier. The current
/// SymbolHit shape doesn't carry the AST parent — we re-read the
/// file and walk back from the hit line until a `fn`/`def`/etc.
/// declaration is found. Returns `None` if not findable; callers
/// substitute a stable position-based pseudo-name.
fn extract_caller_decl_name(hit: &SymbolHit, _called: &str) -> Option<String> {
  use std::fs;
  let content = fs::read_to_string(&hit.file_path).ok()?;
  let lines: Vec<&str> = content.lines().collect();
  let start = (hit.line_start.saturating_sub(1) as usize).min(lines.len().saturating_sub(1));
  for i in (0..=start).rev() {
    let line = lines[i].trim_start();
    for prefix in &[
      "pub fn ",
      "fn ",
      "async fn ",
      "pub async fn ",
      "def ",
      "async def ",
      "function ",
      "public ",
      "private ",
      "protected ",
      "func ",
    ] {
      if let Some(rest) = line.strip_prefix(*prefix) {
        let nm = rest
          .split('(')
          .next()?
          .split('<')
          .next()?
          .split_whitespace()
          .last()?
          .trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
        if !nm.is_empty() {
          return Some(nm.to_string());
        }
      }
    }
  }
  None
}
