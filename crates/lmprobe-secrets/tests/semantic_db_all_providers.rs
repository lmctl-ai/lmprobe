use lmprobe_secrets::{
  analyze_source, load_default_provider_db, ProviderLanguage, ProviderLanguageId, SinkEvidenceMode,
};

#[test]
fn python_provider_entries_emit_and_reject_from_db_snippets() {
  assert_provider_entries_for_language(ProviderLanguageId::Python, "case.py");
}

#[test]
fn javascript_provider_entries_emit_and_reject_from_db_snippets() {
  assert_provider_entries_for_language(ProviderLanguageId::JavaScript, "case.js");
}

#[test]
fn go_provider_entries_emit_and_reject_from_db_snippets() {
  assert_provider_entries_for_language(ProviderLanguageId::Go, "case.go");
}

#[test]
fn java_provider_entries_emit_and_reject_from_db_snippets() {
  assert_provider_entries_for_language(ProviderLanguageId::Java, "Case.java");
}

fn assert_provider_entries_for_language(language_id: ProviderLanguageId, path: &str) {
  let db = load_default_provider_db().expect("default provider DB loads");
  let mut seen = 0;
  for provider in &db.providers {
    for language in provider
      .languages
      .iter()
      .filter(|entry| entry.language == language_id)
    {
      seen += 1;
      let source = positive_source(language);
      let result = analyze_source(path, &source, SinkEvidenceMode::Off);
      assert!(
        result.symbol_reachability.iter().any(|finding| {
          finding.provider_id.as_deref() == Some(provider.id.as_str())
            && finding.sdk.as_deref() == Some(language.sdk.as_str())
        }),
        "{} {} positive should emit provider metadata; source:\n{}\nfindings:\n{:#?}",
        provider.id,
        language.sdk,
        source,
        result.symbol_reachability
      );

      let source = negative_source(language);
      let result = analyze_source(path, &source, SinkEvidenceMode::Off);
      assert!(
        result
          .symbol_reachability
          .iter()
          .all(|finding| finding.provider_id.as_deref() != Some(provider.id.as_str())),
        "{} {} negative should not emit provider metadata; source:\n{}\nfindings:\n{:#?}",
        provider.id,
        language.sdk,
        source,
        result.symbol_reachability
      );
    }
  }
  assert!(seen > 0, "{language_id} should have provider entries");
}

fn positive_source(language: &ProviderLanguage) -> String {
  wrap_source(
    language.language,
    &language
      .call_shapes
      .iter()
      .flat_map(|shape| shape.import_hints.iter())
      .cloned()
      .collect::<Vec<_>>()
      .join(" "),
    &language.positive_test,
  )
}

fn negative_source(language: &ProviderLanguage) -> String {
  wrap_source(
    language.language,
    &language
      .call_shapes
      .iter()
      .flat_map(|shape| shape.import_hints.iter())
      .cloned()
      .collect::<Vec<_>>()
      .join(" "),
    &language.negative_test,
  )
}

fn wrap_source(language: ProviderLanguageId, hints: &str, expression: &str) -> String {
  match language {
    ProviderLanguageId::Python => {
      format!("# {hints}\n{}\n{expression}\n", common_bindings(language))
    }
    ProviderLanguageId::JavaScript | ProviderLanguageId::TypeScript => {
      format!("// {hints}\n{}\n{expression};\n", common_bindings(language))
    }
    ProviderLanguageId::Go => {
      format!(
        "package main\n// {hints}\nfunc main() {{\n{}\n  {expression}\n}}\n",
        common_bindings(language)
      )
    }
    ProviderLanguageId::Java => {
      format!(
        "// {hints}\nclass Case {{ void test() {{ {} {expression} }} }}\n",
        common_bindings(language)
      )
    }
  }
}

fn common_bindings(language: ProviderLanguageId) -> &'static str {
  match language {
    ProviderLanguageId::Python => {
      r#"secret = "provider_db_secret"
token = "provider_db_token"
api_key = "provider_db_api_key"
apiKey = "provider_db_api_key"
key = "provider_db_key"
auth = "provider_db_auth"
auth_token = "provider_db_auth_token"
account_sid = "provider_db_account_sid"
sid = "provider_db_sid"
id = "provider_db_id"
url = "https://example.test"
clientID = "provider_db_client_id""#
    }
    ProviderLanguageId::JavaScript | ProviderLanguageId::TypeScript => {
      r#"const secret = "provider_db_secret";
const token = "provider_db_token";
const api_key = "provider_db_api_key";
const apiKey = "provider_db_api_key";
const key = "provider_db_key";
const auth = "provider_db_auth";
const auth_token = "provider_db_auth_token";
const authToken = "provider_db_auth_token";
const accountSid = "provider_db_account_sid";
const sid = "provider_db_sid";
const id = "provider_db_id";
const url = "https://example.test";
const clientID = "provider_db_client_id";"#
    }
    ProviderLanguageId::Go => {
      r#"  secret := "provider_db_secret"
  token := "provider_db_token"
  api_key := "provider_db_api_key"
  apiKey := "provider_db_api_key"
  key := "provider_db_key"
  auth := "provider_db_auth"
  auth_token := "provider_db_auth_token"
  authToken := "provider_db_auth_token"
  accountSid := "provider_db_account_sid"
  sid := "provider_db_sid"
  id := "provider_db_id"
  url := "https://example.test"
  clientID := "provider_db_client_id""#
    }
    ProviderLanguageId::Java => {
      r#"String secret = "provider_db_secret"; String token = "provider_db_token"; String api_key = "provider_db_api_key"; String apiKey = "provider_db_api_key"; String key = "provider_db_key"; String auth = "provider_db_auth"; String auth_token = "provider_db_auth_token"; String authToken = "provider_db_auth_token"; String accountSid = "provider_db_account_sid"; String sid = "provider_db_sid"; String id = "provider_db_id"; String url = "https://example.test"; String clientID = "provider_db_client_id";"#
    }
  }
}
