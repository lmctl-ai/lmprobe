use lmprobe_secrets::{
  load_default_provider_db, load_gitleaks_config_str, DEFAULT_GITLEAKS_CONFIG,
};

#[test]
fn vendored_default_config_still_loads_and_compiles() {
  let config = load_gitleaks_config_str(DEFAULT_GITLEAKS_CONFIG).expect("vendored config loads");
  assert_eq!(config.rules.len(), 222);
  let compiled = config.compile_patterns();
  let stats = compiled.stats;
  assert_eq!(
    stats.fail, 0,
    "pattern compile failures: {:?}",
    compiled.errors
  );
}

#[test]
fn optional_upstream_gitleaks_config_matches_vendored_fixture() {
  let Ok(path) = std::env::var("LMPROBE_GITLEAKS_CONFIG") else {
    eprintln!("skipping upstream config sync check; LMPROBE_GITLEAKS_CONFIG is not set");
    return;
  };
  let Ok(upstream) = std::fs::read_to_string(&path) else {
    eprintln!("skipping upstream config sync check; missing {path}");
    return;
  };
  if upstream != DEFAULT_GITLEAKS_CONFIG {
    eprintln!(
      "upstream config differs from vendored fixture; see crates/lmprobe-secrets/UPSTREAM_SYNC.md"
    );
  }
  let config = load_gitleaks_config_str(&upstream).expect("upstream config loads");
  let compiled = config.compile_patterns();
  let stats = compiled.stats;
  assert_eq!(
    stats.fail, 0,
    "pattern compile failures: {:?}",
    compiled.errors
  );
}

#[test]
fn provider_semantic_db_rule_ids_exist_in_vendored_gitleaks_config() {
  let db = load_default_provider_db().expect("provider DB loads and rule IDs resolve");
  assert_eq!(db.providers.len(), 10);
  let referenced_rules: usize = db
    .providers
    .iter()
    .map(|provider| provider.rule_ids.len())
    .sum();
  assert_eq!(referenced_rules, 29);
}
