use lmprobe_secrets::scanner::{
  SecretDiagnostic, SecretFinding, SecretsExitStatus, SecretsResult, SuppressedSecretFinding,
};

#[test]
fn scanner_module_reexports_public_secrets_envelope_types() {
  let names = [
    std::any::type_name::<SecretDiagnostic>(),
    std::any::type_name::<SecretFinding>(),
    std::any::type_name::<SecretsExitStatus>(),
    std::any::type_name::<SecretsResult>(),
    std::any::type_name::<SuppressedSecretFinding>(),
  ];
  assert!(names.iter().all(|name| name.contains("lmprobe_types")));
}
