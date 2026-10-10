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
    sdk: "aws-sdk-java-v2",
    positive: r#"
import software.amazon.awssdk.auth.credentials.AwsBasicCredentials;
class Case {
  void f() {
    String secret = "aws_java_v2_secret";
    AwsBasicCredentials.create(id, secret);
  }
}
"#,
    negative: r#"
import software.amazon.awssdk.auth.credentials.DefaultCredentialsProvider;
class Case { void f() { DefaultCredentialsProvider.create(); } }
"#,
  },
  ProviderCase {
    provider_id: "aws",
    sdk: "aws-sdk-java-v1",
    positive: r#"
import com.amazonaws.auth.BasicAWSCredentials;
class Case {
  void f() {
    String secret = "aws_java_v1_secret";
    new BasicAWSCredentials(id, secret);
  }
}
"#,
    negative: r#"
import com.amazonaws.auth.DefaultAWSCredentialsProviderChain;
class Case { void f() { new DefaultAWSCredentialsProviderChain(); } }
"#,
  },
  ProviderCase {
    provider_id: "stripe",
    sdk: "stripe-java-current",
    positive: r#"
import com.stripe.Stripe;
class Case {
  void f() {
    String key = "sk_REMOVEMElive_4eC39HqLyjWDarjtT1zdp7dc";
    Stripe.apiKey = key;
  }
}
"#,
    negative: r#"
import com.stripe.Stripe;
class Case {
  void f() {
    String key = "sk_REMOVEMElive_4eC39HqLyjWDarjtT1zdp7dc";
    Stripe.apiVersion = "2024-01-01";
  }
}
"#,
  },
  ProviderCase {
    provider_id: "github",
    sdk: "github-api-java-current",
    positive: r#"
import org.kohsuke.github.GitHubBuilder;
class Case {
  void f() {
    String token = "ghp_providerdbtoken123456789";
    new GitHubBuilder().withOAuthToken(token).build();
  }
}
"#,
    negative: r#"
import org.kohsuke.github.GitHubBuilder;
class Case { void f() { new GitHubBuilder().build(); } }
"#,
  },
  ProviderCase {
    provider_id: "slack",
    sdk: "slack-api-client-java-current",
    positive: r#"
import com.slack.api.Slack;
class Case {
  void f() {
    String token = "xoxb-provider-db-token";
    Slack.getInstance().methods(token);
  }
}
"#,
    negative: r#"
import com.slack.api.Slack;
class Case { void f() { Slack.getInstance(); } }
"#,
  },
  ProviderCase {
    provider_id: "twilio",
    sdk: "twilio-java-current",
    positive: r#"
import com.twilio.Twilio;
class Case {
  void f() {
    String token = "twilio_provider_auth";
    Twilio.init(accountSid, token);
  }
}
"#,
    negative: r#"
import com.twilio.Twilio;
class Case { void f() { Twilio.destroy(); } }
"#,
  },
  ProviderCase {
    provider_id: "sendgrid",
    sdk: "sendgrid-java-current",
    positive: r#"
import com.sendgrid.SendGrid;
class Case {
  void f() {
    String apiKey = "SG.provider.db.key";
    new SendGrid(apiKey);
  }
}
"#,
    negative: r#"
import com.sendgrid.Email;
class Case { void f() { new Email("a@example.com"); } }
"#,
  },
];

#[test]
fn java_provider_db_positive_shapes_emit_provider_metadata() {
  for case in PROVIDERS {
    let result = analyze_source(
      "Case.java",
      &case.positive.replace("REMOVEME", ""),
      SinkEvidenceMode::Off,
    );
    assert!(
      result.symbol_reachability.iter().any(|finding| {
        finding.provider_id.as_deref() == Some(case.provider_id)
          && finding.sdk.as_deref() == Some(case.sdk)
      }),
      "{} {} positive should emit provider metadata; got {:#?}",
      case.provider_id,
      case.sdk,
      result.symbol_reachability
    );
  }
}

#[test]
fn java_provider_db_negative_shapes_do_not_emit_that_provider() {
  for case in PROVIDERS {
    let result = analyze_source(
      "Case.java",
      &case.negative.replace("REMOVEME", ""),
      SinkEvidenceMode::Off,
    );
    assert!(result
      .symbol_reachability
      .iter()
      .all(
        |finding| finding.provider_id.as_deref() != Some(case.provider_id)
          || finding.sdk.as_deref() != Some(case.sdk)
      ));
  }
}

#[test]
fn java_canonical_merge_cross_links_layer1_rule_and_provider_sink() {
  let content = r#"
import com.stripe.Stripe;
class Case {
  void f() {
    String key = "sk_REMOVEMElive_4eC39HqLyjWDarjtT1zdp7dc";
    Stripe.apiKey = key;
  }
}
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
    file_path: "Case.java",
    content: &content,
    source_mode: "test",
  });
  let semantic = analyze_source("Case.java", &content, SinkEvidenceMode::Off);
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
