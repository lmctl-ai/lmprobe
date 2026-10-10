use lmprobe_secrets::{load_default_provider_db, load_default_source_db};
use std::collections::HashSet;

#[test]
fn source_db_provider_ids_exist_in_default_provider_db() {
  let providers = load_default_provider_db().expect("provider DB loads");
  let provider_ids: HashSet<_> = providers
    .providers
    .iter()
    .map(|provider| provider.id.as_str())
    .collect();
  let sources = load_default_source_db().expect("source DB loads");
  for source in &sources.sources {
    if let Some(provider_id) = source.provider_id.as_deref() {
      assert!(
        provider_ids.contains(provider_id),
        "{} references unknown provider {}",
        source.id,
        provider_id
      );
    }
  }
}
