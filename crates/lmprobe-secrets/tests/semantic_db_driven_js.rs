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
    sdk: "aws-sdk-js-v3",
    positive: r#"
import { S3Client } from "@aws-sdk/client-s3";
const secret = "aws_js_secret";
new S3Client({ credentials: { accessKeyId: id, secretAccessKey: secret } });
"#,
    negative: r#"
import { S3Client } from "@aws-sdk/client-s3";
const secret = "aws_js_secret";
new S3Client({ region: "us-east-1" });
"#,
  },
  ProviderCase {
    provider_id: "stripe",
    sdk: "stripe-node-current",
    positive: r#"
const key = "sk_REMOVEMElive_4eC39HqLyjWDarjtT1zdp7dc";
require("stripe")(key);
"#,
    negative: r#"
const key = "sk_REMOVEMElive_4eC39HqLyjWDarjtT1zdp7dc";
const port = 8080;
"#,
  },
  ProviderCase {
    provider_id: "github",
    sdk: "octokit-js-current",
    positive: r#"
import { Octokit } from "octokit";
const token = "ghp_providerdbtoken123456789";
new Octokit({ auth: token });
"#,
    negative: r#"
import { Octokit } from "octokit";
const token = "ghp_providerdbtoken123456789";
new Octokit({ userAgent: "x" });
"#,
  },
  ProviderCase {
    provider_id: "slack",
    sdk: "slack-web-api-js-current",
    positive: r#"
import { WebClient } from "@slack/web-api";
const token = "xoxb-provider-db-token";
new WebClient(token);
"#,
    negative: r#"
import { WebClient } from "@slack/web-api";
const token = "xoxb-provider-db-token";
new WebClient();
"#,
  },
  ProviderCase {
    provider_id: "twilio",
    sdk: "twilio-node-current",
    positive: r#"
const twilio = require("twilio");
const sid = "AC11111111111111111111111111111111";
const auth = "twilio_provider_auth";
twilio(sid, auth);
"#,
    negative: r#"
const twilio = require("twilio");
const auth = "twilio_provider_auth";
twilio();
"#,
  },
  ProviderCase {
    provider_id: "sendgrid",
    sdk: "sendgrid-nodejs-current",
    positive: r#"
const sgMail = require("@sendgrid/mail");
const apiKey = "SG.provider.db.key";
sgMail.setApiKey(apiKey);
"#,
    negative: r#"
const sgMail = require("@sendgrid/mail");
const apiKey = "SG.provider.db.key";
sgMail.setSubstitutionWrappers("{{", "}}");
"#,
  },
];

#[test]
fn javascript_provider_db_positive_shapes_emit_provider_metadata() {
  for case in PROVIDERS {
    let result = analyze_source(
      "case.js",
      &case.positive.replace("REMOVEME", ""),
      SinkEvidenceMode::Off,
    );
    assert!(
      result.symbol_reachability.iter().any(|finding| {
        finding.provider_id.as_deref() == Some(case.provider_id)
          && finding.sdk.as_deref() == Some(case.sdk)
      }),
      "{} positive should emit provider metadata; got {:#?}",
      case.provider_id,
      result.symbol_reachability
    );
  }
}

#[test]
fn typescript_provider_db_uses_javascript_shapes() {
  let result = analyze_source(
    "case.ts",
    r#"
import { WebClient } from "@slack/web-api";
const token = "xoxb-provider-db-token";
new WebClient(token);
"#,
    SinkEvidenceMode::Off,
  );
  assert!(result
    .symbol_reachability
    .iter()
    .any(|finding| finding.provider_id.as_deref() == Some("slack")
      && finding.sdk.as_deref() == Some("slack-web-api-js-current")));
}

#[test]
fn javascript_provider_db_negative_shapes_do_not_emit_that_provider() {
  for case in PROVIDERS {
    let result = analyze_source(
      "case.js",
      &case.negative.replace("REMOVEME", ""),
      SinkEvidenceMode::Off,
    );
    assert!(result
      .symbol_reachability
      .iter()
      .all(|finding| finding.provider_id.as_deref() != Some(case.provider_id)));
  }
}

#[test]
fn javascript_canonical_merge_cross_links_layer1_rule_and_provider_sink() {
  let content = r#"
const key = "sk_REMOVEMElive_4eC39HqLyjWDarjtT1zdp7dc";
require("stripe")(key);
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
    file_path: "case.js",
    content: &content,
    source_mode: "test",
  });
  let semantic = analyze_source("case.js", &content, SinkEvidenceMode::Off);
  let mut result = SecretsResult::with_sink_catalog_version(findings, SINK_CATALOG_VERSION);
  result.semantic_findings = semantic.symbol_reachability;
  result.rebuild_canonical_findings();

  let canonical = result
    .canonical_findings
    .iter()
    .find(|finding| finding.provider_id.as_deref() == Some("stripe"))
    .expect("canonical stripe finding");
  assert_eq!(canonical.detector, "gitleaks-rule");
  assert_eq!(canonical.rule_id.as_deref(), Some("stripe-access-token"));
  assert!(canonical
    .evidence
    .iter()
    .any(|evidence| evidence.detector == "symbol-reachability"));
}

#[test]
fn javascript_provider_sink_evidence_is_opt_in() {
  let content = r#"
import { Octokit } from "octokit";
new Octokit({ auth: tokenFromRuntime });
"#;
  let off = analyze_source("case.js", content, SinkEvidenceMode::Off);
  assert!(off.sink_evidence.is_empty());

  let on = analyze_source("case.js", content, SinkEvidenceMode::On);
  let finding = on
    .sink_evidence
    .iter()
    .find(|finding| finding.provider_id.as_deref() == Some("github"))
    .expect("github provider sink evidence");
  assert_eq!(finding.detector, "provider-sink");
  assert_eq!(finding.confidence, SemanticConfidence::ProviderSinkPattern);
}
