use lmprobe_secrets::{
  analyze_source, FileScanInput, Layer1Engine, SecretsResult, SemanticConfidence, SinkEvidenceMode,
  SINK_CATALOG_VERSION,
};

struct ProviderCase {
  provider_id: &'static str,
  sdk: &'static str,
  positive: &'static str,
  negative: &'static str,
}

const PROVIDERS: &[ProviderCase] = &[
  ProviderCase {
    provider_id: "aws",
    sdk: "boto3-v1",
    positive: r#"
import boto3
secret = "aws_db_secret_value"
boto3.client("s3", aws_secret_access_key=secret)
"#,
    negative: r#"
import boto3
secret = "aws_db_secret_value"
boto3.client("s3")
"#,
  },
  ProviderCase {
    provider_id: "stripe",
    sdk: "stripe-python-current",
    positive: r#"
import stripe
key = "sk_REMOVEMElive_4eC39HqLyjWDarjtT1zdp7dc"
stripe.api_key = key
"#,
    negative: r#"
import stripe
key = "sk_REMOVEMElive_4eC39HqLyjWDarjtT1zdp7dc"
stripe.api_version = "2024-01-01"
"#,
  },
  ProviderCase {
    provider_id: "github",
    sdk: "pygithub-current",
    positive: r#"
from github import Github
token = "ghp_FAKEproviderdbtoken123456789"
Github(token)
"#,
    negative: r#"
from github import Github
token = "ghp_FAKEproviderdbtoken123456789"
Github()
"#,
  },
  ProviderCase {
    provider_id: "slack",
    sdk: "slack-sdk-python-v3",
    positive: r#"
from slack_sdk import WebClient
token = "xoxb-provider-db-token-123"
WebClient(token=token)
"#,
    negative: r#"
from slack_sdk import WebClient
token = "xoxb-provider-db-token-123"
WebClient(timeout=10)
"#,
  },
  ProviderCase {
    provider_id: "twilio",
    sdk: "twilio-python-v9",
    positive: r#"
from twilio.rest import Client
sid = "AC11111111111111111111111111111111"
auth = "twilio_provider_db_auth"
Client(sid, auth)
"#,
    negative: r#"
from twilio.rest import Client
sid = "AC11111111111111111111111111111111"
auth = "twilio_provider_db_auth"
Client()
"#,
  },
  ProviderCase {
    provider_id: "sendgrid",
    sdk: "sendgrid-python-current",
    positive: r#"
from sendgrid import SendGridAPIClient
api_key = "SG.providerdb.fakekey"
SendGridAPIClient(api_key)
"#,
    negative: r#"
from sendgrid import SendGridAPIClient
api_key = "SG.providerdb.fakekey"
SendGridAPIClient()
"#,
  },
];

#[test]
fn python_provider_db_positive_shapes_emit_provider_metadata() {
  for case in PROVIDERS {
    let result = analyze_source(
      "case.py",
      &case.positive.replace("REMOVEME", ""),
      SinkEvidenceMode::Off,
    );
    assert!(
      result.symbol_reachability.iter().any(|finding| {
        finding.provider_id.as_deref() == Some(case.provider_id)
          && finding.sdk.as_deref() == Some(case.sdk)
          && finding
            .provider_shape_id
            .as_deref()
            .is_some_and(|shape_id| {
              shape_id.starts_with(&format!("{}/python/{}/", case.provider_id, case.sdk))
            })
          && finding
            .provider_rule_ids
            .iter()
            .any(|rule_id| !rule_id.is_empty())
      }),
      "{} positive should emit provider metadata; got {:#?}",
      case.provider_id,
      result.symbol_reachability
    );
  }
}

#[test]
fn python_provider_db_negative_shapes_do_not_emit_that_provider() {
  for case in PROVIDERS {
    let result = analyze_source(
      "case.py",
      &case.negative.replace("REMOVEME", ""),
      SinkEvidenceMode::Off,
    );
    assert!(
      result
        .symbol_reachability
        .iter()
        .all(|finding| finding.provider_id.as_deref() != Some(case.provider_id)),
      "{} negative should not emit provider metadata; got {:#?}",
      case.provider_id,
      result.symbol_reachability
    );
  }
}

#[test]
fn canonical_merge_cross_links_layer1_rule_and_provider_sink() {
  let content = r#"
import stripe
key = "sk_REMOVEMElive_4eC39HqLyjWDarjtT1zdp7dc"
stripe.api_key = key
"#
  .replace("REMOVEME", "");
  let engine = Layer1Engine::from_config_str(
    r#"
title = "test"
[[rules]]
id = "stripe-access-token"
description = "stripe"
regex = '''\b(sk_live_[a-zA-Z0-9]{10,99})'''
keywords = ["sk_live"]
"#,
  )
  .expect("layer1 engine");
  let findings = engine.scan_file(FileScanInput {
    file_path: "case.py",
    content: &content,
    source_mode: "test",
  });
  assert!(findings
    .iter()
    .any(|finding| finding.rule_id == "stripe-access-token"));

  let semantic = analyze_source("case.py", &content, SinkEvidenceMode::Off);
  assert!(semantic
    .symbol_reachability
    .iter()
    .any(|finding| finding.provider_id.as_deref() == Some("stripe")));

  let mut result = SecretsResult::with_sink_catalog_version(findings, SINK_CATALOG_VERSION);
  result.semantic_findings = semantic.symbol_reachability;
  result.sink_evidence = semantic.sink_evidence;
  result.rebuild_canonical_findings();

  let canonical = result
    .canonical_findings
    .iter()
    .find(|finding| finding.provider_id.as_deref() == Some("stripe"))
    .expect("canonical stripe finding");
  assert_eq!(canonical.detector, "gitleaks-rule");
  assert_eq!(canonical.rule_id.as_deref(), Some("stripe-access-token"));
  assert_eq!(canonical.provider_id.as_deref(), Some("stripe"));
  assert!(canonical
    .evidence
    .iter()
    .any(|evidence| evidence.detector == "symbol-reachability"));
  assert!(canonical
    .evidence
    .iter()
    .any(|evidence| evidence.detector == "gitleaks-rule"));
}

#[test]
fn provider_sink_evidence_is_opt_in_and_uses_provider_confidence() {
  let content = r#"
import boto3
boto3.client("s3", aws_secret_access_key=user_input)
"#;
  let off = analyze_source("case.py", content, SinkEvidenceMode::Off);
  assert!(off.sink_evidence.is_empty());

  let on = analyze_source("case.py", content, SinkEvidenceMode::On);
  let finding = on
    .sink_evidence
    .iter()
    .find(|finding| finding.provider_id.as_deref() == Some("aws"))
    .expect("aws provider sink evidence");
  assert_eq!(finding.detector, "provider-sink");
  assert_eq!(finding.confidence, SemanticConfidence::ProviderSinkPattern);
  assert_eq!(finding.sdk.as_deref(), Some("boto3-v1"));
}
