# Upstream gitleaks Rule Sync

`lmprobe-secrets` vendors the upstream gitleaks default TOML config as
`tests/fixtures/gitleaks-default.toml`. The scanner intentionally consumes the
gitleaks TOML rule schema, but implements rule logic in Rust.

## Sync Procedure

1. Update the local gitleaks checkout.
2. Copy the upstream default config:

   ```sh
   cp /path/to/gitleaks/config/gitleaks.toml \
     crates/lmprobe-secrets/tests/fixtures/gitleaks-default.toml
   ```

3. Run the rule/schema checks:

   ```sh
   LMPROBE_GITLEAKS_CONFIG=/path/to/gitleaks/config/gitleaks.toml \
     cargo test -p lmprobe-secrets --test upstream_config_sync -- --nocapture
   ```

4. Run the parity gate:

   ```sh
   cargo test -p lmprobe-secrets --test gitleaks_parity -- --nocapture
   ```

5. Run the normal crate gates:

   ```sh
   cargo test -p lmprobe-secrets -- --nocapture
   cargo clippy -p lmprobe-secrets --all-targets -- -D warnings
   cargo fmt --check
   cargo build
   ```

## Expected Outcomes

- The config loads with the gitleaks TOML schema model.
- All rule regex/path/allowlist patterns compile through the RE2-to-Rust regex
  compatibility layer.
- Every `rule_id` referenced by `providers/default.toml` exists in the vendored
  gitleaks config. If this fails after an upstream sync, update the provider
  semantic DB join keys in the same change as the gitleaks fixture refresh.
- Every `provider_id` referenced by `sources/default.toml` exists in
  `providers/default.toml`. If this fails after a provider DB sync, update the
  source DB join keys in the same change.
- The differential parity test remains at the documented threshold.

If a future upstream rule fails pattern compilation, extend the compatibility
shim in `src/regex_compat.rs` with a general RE2-compatible normalization. Do
not add one-off line-number patches for individual upstream rules.

If parity changes, classify each delta before changing engine behavior:

- upstream rule corpus changed
- deferred scope such as decoder coordinate mapping or richer baseline matching
- lmprobe logic regression

Only the last category should block a routine config refresh.
