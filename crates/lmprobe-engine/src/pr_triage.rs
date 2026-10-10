//! GitHub pull-request triage via the `gh` CLI.
//!
//! This first slice is intentionally metadata-only: it normalizes a
//! GitHub PR URL, asks `gh pr view` for status/check/review fields,
//! and returns evidence terms without recommending reviewers or
//! computing code impact.

use anyhow::{anyhow, Result};
use lmprobe_types::schema::{
  Diagnostic, ExitStatus, PrCheckEvidence, PrCheckSummary, PrReviewEvidence, PrTriageResult,
  PrTriageStatus, PrTriageTarget,
};
use serde_json::Value;
use std::ffi::OsString;
use std::process::Command;

const GH_FIELDS: &str = "number,url,title,state,isDraft,mergeable,reviewDecision,author,statusCheckRollup,comments,reviews,reviewThreads";
const GH_FIELDS_NO_REVIEW_THREADS: &str = "number,url,title,state,isDraft,mergeable,reviewDecision,author,statusCheckRollup,comments,reviews";
const ACTION_TERMS: &[&str] = &[
  "changes requested",
  "failed",
  "failing",
  "error",
  "dco",
  "cla",
  "template",
  "nitpick",
  "quick win",
  "does not verify",
  "would still pass",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrTriageSince {
  LastAuthorComment,
  All,
}

impl PrTriageSince {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::LastAuthorComment => "last-author-comment",
      Self::All => "all",
    }
  }
}

#[derive(Debug, Clone, Copy)]
pub struct PrTriageOptions {
  pub since: PrTriageSince,
  pub compact: bool,
}

pub fn pr_triage(url: &str, opts: PrTriageOptions) -> Result<PrTriageResult> {
  let target = parse_github_pr_url(url)?;
  let mut result = PrTriageResult::new(target, opts.since.as_str(), opts.compact);
  let gh = std::env::var_os("LMPROBE_GH").unwrap_or_else(|| OsString::from("gh"));

  let mut include_review_threads = true;
  let mut output = match Command::new(&gh)
    .arg("pr")
    .arg("view")
    .arg(&result.target.url)
    .arg("--json")
    .arg(GH_FIELDS)
    .output()
  {
    Ok(output) => output,
    Err(err) => {
      result.exit_status = ExitStatus::Error;
      result.push_warning(
        Diagnostic::new("GITHUB_CLI_FAILED", "failed to execute GitHub CLI").with_hint(format!(
          "install `gh`, authenticate it, or set LMPROBE_GH; {err}"
        )),
      );
      return Ok(result);
    }
  };
  if !output.status.success() {
    result.exit_status = ExitStatus::Error;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if is_unknown_review_threads_error(&stderr) {
      include_review_threads = false;
      result.push_warning(
        Diagnostic::new(
          "GITHUB_CLI_REVIEW_THREADS_UNSUPPORTED",
          "GitHub CLI does not support reviewThreads",
        )
        .with_hint("continuing without inline review comment evidence"),
      );
      match Command::new(&gh)
        .arg("pr")
        .arg("view")
        .arg(&result.target.url)
        .arg("--json")
        .arg(GH_FIELDS_NO_REVIEW_THREADS)
        .output()
      {
        Ok(retry_output) => {
          if !retry_output.status.success() {
            let retry_stderr = String::from_utf8_lossy(&retry_output.stderr);
            result.push_warning(
              Diagnostic::new("GITHUB_CLI_FAILED", "GitHub CLI returned a non-zero status")
                .with_hint(truncate(retry_stderr.trim(), 240)),
            );
            return Ok(result);
          }
          output = retry_output;
        }
        Err(err) => {
          result.push_warning(
            Diagnostic::new("GITHUB_CLI_FAILED", "failed to execute GitHub CLI").with_hint(
              format!("install `gh`, authenticate it, or set LMPROBE_GH; {err}"),
            ),
          );
          return Ok(result);
        }
      }
    } else {
      result.push_warning(
        Diagnostic::new("GITHUB_CLI_FAILED", "GitHub CLI returned a non-zero status")
          .with_hint(truncate(stderr.trim(), 240)),
      );
      return Ok(result);
    }
  }
  let payload: Value = match serde_json::from_slice(&output.stdout) {
    Ok(payload) => payload,
    Err(err) => {
      result.exit_status = ExitStatus::Error;
      result.push_warning(
        Diagnostic::new("GITHUB_CLI_JSON", "GitHub CLI output was not valid JSON")
          .with_hint(err.to_string()),
      );
      return Ok(result);
    }
  };

  result.status = Some(pr_status(&payload));
  result.checks = checks(&payload);
  result.check_summary = summarize_checks(&result.checks);
  result.evidence = review_evidence(&payload, opts, include_review_threads);
  result.exit_status = ExitStatus::Matched;
  Ok(result)
}

fn is_unknown_review_threads_error(stderr: &str) -> bool {
  stderr.contains(r#"Unknown JSON field: "reviewThreads""#)
}

fn parse_github_pr_url(input: &str) -> Result<PrTriageTarget> {
  let trimmed = input.trim();
  let without_scheme = trimmed
    .strip_prefix("https://")
    .or_else(|| trimmed.strip_prefix("http://"))
    .unwrap_or(trimmed);
  let without_www = without_scheme
    .strip_prefix("www.")
    .unwrap_or(without_scheme);
  let path = without_www.strip_prefix("github.com/").ok_or_else(|| {
    anyhow!("expected GitHub pull request URL: https://github.com/<owner>/<repo>/pull/<number>")
  })?;
  let path = path
    .split(['?', '#'])
    .next()
    .unwrap_or(path)
    .trim_end_matches('/');
  let parts = path.split('/').collect::<Vec<_>>();
  if parts.len() != 4 || parts[2] != "pull" {
    return Err(anyhow!(
      "expected GitHub pull request URL: https://github.com/<owner>/<repo>/pull/<number>"
    ));
  }
  let number = parts[3]
    .parse::<u64>()
    .map_err(|_| anyhow!("pull request number must be an integer"))?;
  Ok(PrTriageTarget {
    owner: parts[0].to_string(),
    repo: parts[1].to_string(),
    number,
    url: format!(
      "https://github.com/{}/{}/pull/{}",
      parts[0], parts[1], number
    ),
  })
}

fn pr_status(payload: &Value) -> PrTriageStatus {
  PrTriageStatus {
    title: string_field(payload, "title"),
    author: payload
      .get("author")
      .and_then(|author| string_field(author, "login")),
    state: string_field(payload, "state"),
    mergeable: string_field(payload, "mergeable"),
    review_decision: string_field(payload, "reviewDecision"),
    draft: payload
      .get("isDraft")
      .and_then(Value::as_bool)
      .unwrap_or(false),
  }
}

fn checks(payload: &Value) -> Vec<PrCheckEvidence> {
  payload
    .get("statusCheckRollup")
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
    .map(|check| {
      let status = string_field(check, "status");
      let conclusion = string_field(check, "conclusion");
      let state = string_field(check, "state");
      let bucket = check_bucket(status.as_deref(), conclusion.as_deref(), state.as_deref());
      PrCheckEvidence {
        name: string_field(check, "name")
          .or_else(|| string_field(check, "context"))
          .unwrap_or_else(|| "check".to_string()),
        status,
        conclusion,
        state,
        bucket,
        url: string_field(check, "detailsUrl").or_else(|| string_field(check, "targetUrl")),
      }
    })
    .collect()
}

fn check_bucket(status: Option<&str>, conclusion: Option<&str>, state: Option<&str>) -> String {
  let conclusion = conclusion.unwrap_or_default().to_ascii_lowercase();
  let status = status.unwrap_or_default().to_ascii_lowercase();
  let state = state.unwrap_or_default().to_ascii_lowercase();
  if matches!(
    conclusion.as_str(),
    "failure" | "failed" | "timed_out" | "timed out" | "action_required" | "cancelled"
  ) || matches!(state.as_str(), "failure" | "error" | "failed")
  {
    "failing".to_string()
  } else if matches!(conclusion.as_str(), "success") || matches!(state.as_str(), "success") {
    "passing".to_string()
  } else if matches!(conclusion.as_str(), "neutral" | "skipped") {
    "skipped_neutral".to_string()
  } else if matches!(
    status.as_str(),
    "queued" | "in_progress" | "requested" | "pending"
  ) || matches!(state.as_str(), "pending" | "expected")
  {
    "pending".to_string()
  } else {
    "pending".to_string()
  }
}

fn summarize_checks(checks: &[PrCheckEvidence]) -> PrCheckSummary {
  let mut summary = PrCheckSummary {
    total: checks.len(),
    ..Default::default()
  };
  for check in checks {
    match check.bucket.as_str() {
      "passing" => summary.passing += 1,
      "failing" => summary.failing += 1,
      "skipped_neutral" => summary.skipped_neutral += 1,
      _ => summary.pending += 1,
    }
  }
  summary
}

fn review_evidence(
  payload: &Value,
  opts: PrTriageOptions,
  include_review_threads: bool,
) -> Vec<PrReviewEvidence> {
  let cutoff = match opts.since {
    PrTriageSince::All => None,
    PrTriageSince::LastAuthorComment => last_author_comment_at(payload),
  };
  let mut events = Vec::new();
  if let Some(comments) = payload.get("comments").and_then(Value::as_array) {
    for comment in comments {
      push_event(
        &mut events,
        "comment",
        comment,
        None,
        opts,
        cutoff.as_deref(),
      );
    }
  }
  if let Some(reviews) = payload.get("reviews").and_then(Value::as_array) {
    for review in reviews {
      push_event(
        &mut events,
        "review",
        review,
        string_field(review, "state"),
        opts,
        cutoff.as_deref(),
      );
    }
  }
  if include_review_threads {
    if let Some(threads) = payload.get("reviewThreads").and_then(Value::as_array) {
      for thread in threads {
        if let Some(comments) = thread.get("comments").and_then(Value::as_array) {
          for comment in comments {
            push_event(
              &mut events,
              "review_comment",
              comment,
              None,
              opts,
              cutoff.as_deref(),
            );
          }
        }
      }
    }
  }
  events.sort_by(|a, b| a.created_at.cmp(&b.created_at));
  events
}

fn push_event(
  events: &mut Vec<PrReviewEvidence>,
  kind: &str,
  item: &Value,
  review_state: Option<String>,
  opts: PrTriageOptions,
  cutoff: Option<&str>,
) {
  let created_at = string_field(item, "createdAt")
    .or_else(|| string_field(item, "submittedAt"))
    .unwrap_or_default();
  if let Some(cutoff) = cutoff {
    if created_at.as_str() <= cutoff {
      return;
    }
  }
  let body = string_field(item, "body").unwrap_or_default();
  let excerpt_len = if opts.compact { 80 } else { 240 };
  events.push(PrReviewEvidence {
    kind: kind.to_string(),
    author: item
      .get("author")
      .and_then(|author| string_field(author, "login"))
      .unwrap_or_else(|| "unknown".to_string()),
    author_association: string_field(item, "authorAssociation"),
    created_at,
    body_excerpt: truncate(&body, excerpt_len),
    action_terms: action_terms(&body),
    review_state,
    path: string_field(item, "path"),
    line: item
      .get("line")
      .and_then(Value::as_u64)
      .and_then(|line| usize::try_from(line).ok()),
    url: string_field(item, "url"),
  });
}

fn last_author_comment_at(payload: &Value) -> Option<String> {
  let author = payload
    .get("author")
    .and_then(|author| string_field(author, "login"))?;
  payload
    .get("comments")
    .and_then(Value::as_array)?
    .iter()
    .filter(|comment| {
      comment
        .get("author")
        .and_then(|author| string_field(author, "login"))
        .is_some_and(|login| login == author)
    })
    .filter_map(|comment| string_field(comment, "createdAt"))
    .max()
}

fn action_terms(body: &str) -> Vec<String> {
  let lowered = body.to_ascii_lowercase();
  ACTION_TERMS
    .iter()
    .filter(|term| lowered.contains(**term))
    .map(|term| (*term).to_string())
    .collect()
}

fn string_field(value: &Value, key: &str) -> Option<String> {
  value
    .get(key)
    .and_then(Value::as_str)
    .map(ToString::to_string)
}

fn truncate(input: &str, limit: usize) -> String {
  let mut out = input.trim().replace('\n', " ");
  if out.len() > limit {
    out.truncate(limit.saturating_sub(3));
    out.push_str("...");
  }
  out
}
