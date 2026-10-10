use lmprobe_secrets::{
  analyze_source, FileScanInput, Layer1Engine, SecretsResult, SinkEvidenceMode,
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
    sdk: "aws-sdk-go-v2",
    positive: r#"
package main
import "github.com/aws/aws-sdk-go-v2/credentials"
func f() {
  id := "aws_id"
  secret := "aws_go_secret"
  credentials.NewStaticCredentialsProvider(id, secret, "")
}
"#,
    negative: r#"
package main
import "github.com/aws/aws-sdk-go-v2/config"
func f() {
  secret := "aws_go_secret"
  config.LoadDefaultConfig(ctx)
}
"#,
  },
  ProviderCase {
    provider_id: "stripe",
    sdk: "stripe-go-v85",
    positive: r#"
package main
import stripe "github.com/stripe/stripe-go/v85"
func f() {
  key := "sk_REMOVEMElive_4eC39HqLyjWDarjtT1zdp7dc"
  stripe.Key = key
}
"#,
    negative: r#"
package main
import stripe "github.com/stripe/stripe-go/v85"
func f() {
  key := "sk_REMOVEMElive_4eC39HqLyjWDarjtT1zdp7dc"
  stripe.APIVersion = "2024-01-01"
}
"#,
  },
  ProviderCase {
    provider_id: "github",
    sdk: "go-github-current",
    positive: r#"
package main
import "golang.org/x/oauth2"
func f() {
  token := "ghp_providerdbtoken123456789"
  oauth2.StaticTokenSource(&oauth2.Token{AccessToken: token})
}
"#,
    negative: r#"
package main
import "github.com/google/go-github/v66/github"
func f() {
  token := "ghp_providerdbtoken123456789"
  github.NewClient(nil)
}
"#,
  },
  ProviderCase {
    provider_id: "slack",
    sdk: "slack-go-current",
    positive: r#"
package main
import "github.com/slack-go/slack"
func f() {
  token := "xoxb-provider-db-token"
  slack.New(token)
}
"#,
    negative: r#"
package main
import "github.com/slack-go/slack"
func f() {
  token := "xoxb-provider-db-token"
  slack.New()
}
"#,
  },
  ProviderCase {
    provider_id: "twilio",
    sdk: "twilio-go-current",
    positive: r#"
package main
import twilio "github.com/twilio/twilio-go"
func f() {
  sid := "AC11111111111111111111111111111111"
  token := "twilio_provider_auth"
  twilio.NewRestClientWithParams(twilio.ClientParams{Username: sid, Password: token})
}
"#,
    negative: r#"
package main
import twilio "github.com/twilio/twilio-go"
func f() {
  token := "twilio_provider_auth"
  twilio.NewRestClient()
}
"#,
  },
  ProviderCase {
    provider_id: "sendgrid",
    sdk: "sendgrid-go-v4",
    positive: r#"
package main
import "github.com/sendgrid/sendgrid-go/v4"
func f() {
  apiKey := "SG.provider.db.key"
  sendgrid.NewSendClient(apiKey)
}
"#,
    negative: r#"
package main
import "github.com/sendgrid/sendgrid-go/v4"
func f() {
  apiKey := "SG.provider.db.key"
  sendgrid.MakeRequest(request)
}
"#,
  },
];

#[test]
fn go_provider_db_positive_shapes_emit_provider_metadata() {
  for case in PROVIDERS {
    let result = analyze_source(
      "case.go",
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
fn go_provider_db_negative_shapes_do_not_emit_that_provider() {
  for case in PROVIDERS {
    let result = analyze_source(
      "case.go",
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
fn go_canonical_merge_cross_links_layer1_rule_and_provider_sink() {
  let content = r#"
package main
import "github.com/aws/aws-sdk-go-v2/credentials"
func f() {
  id := "id"
  secret := "AKIAZ3M7Q6YB2N4P5R6S"
  credentials.NewStaticCredentialsProvider(id, secret, "")
}
"#;
  let engine = Layer1Engine::from_config_str(
    r#"
title = "test"
[[rules]]
id = "aws-access-token"
description = "aws"
regex = '''AKIA[0-9A-Z]{16}'''
"#,
  )
  .expect("layer1 engine");
  let findings = engine.scan_file(FileScanInput {
    file_path: "case.go",
    content,
    source_mode: "test",
  });
  let semantic = analyze_source("case.go", content, SinkEvidenceMode::Off);
  let mut result = SecretsResult::with_sink_catalog_version(findings, SINK_CATALOG_VERSION);
  result.semantic_findings = semantic.symbol_reachability;
  result.rebuild_canonical_findings();

  let canonical = result
    .canonical_findings
    .iter()
    .find(|finding| finding.provider_id.as_deref() == Some("aws"))
    .expect("canonical aws finding");
  assert_eq!(canonical.detector, "gitleaks-rule");
  assert_eq!(canonical.rule_id.as_deref(), Some("aws-access-token"));
  assert!(canonical
    .evidence
    .iter()
    .any(|evidence| evidence.detector == "symbol-reachability"));
}
