pub use lmprobe_types::schema::Secret;

#[cfg(test)]
mod tests {
  use super::Secret;

  #[test]
  fn secret_formats_and_serializes_redacted_by_default() {
    let secret = Secret::new("AKIAZ3M7Q6YB2N4P5R6S");

    assert_eq!(secret.expose_raw(), "AKIAZ3M7Q6YB2N4P5R6S");
    assert!(!format!("{secret:?}").contains(secret.expose_raw()));
    assert!(!format!("{secret}").contains(secret.expose_raw()));

    let json = serde_json::to_string(&secret).expect("serialize secret");
    assert_eq!(json, "\"REDACTED\"");
    assert!(!json.contains(secret.expose_raw()));
  }
}
