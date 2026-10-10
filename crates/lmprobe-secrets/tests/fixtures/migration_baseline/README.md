# Migration Baseline Snapshots

These fixtures implement durable-memory/008 amendment B2: a non-lossy migration proof for moving the hand-rolled semantic sink recognizer to the provider semantic DB. The credentials in these files are synthetic test values, never real secrets.

Fixtures:
- `python/boto3_app.py`: boto3 credential kwarg, `stripe.api_key` assignment, and generic `client.login(token)`.
- `javascript/stripe_app.js`: Stripe constructor-style call and an Authorization header.
- `go/aws_app.go`: AWS Go SDK static credentials call and `aws.Credentials` struct literal.
- `java/BasicAWSCredentials.java`: Java AWS credentials constructor and constructor-style field flow into `login`.

Each `expected.json` is captured from the current shipped CLI with:

```sh
./target/debug/lmprobe secrets --format json --gitleaks-compat --config crates/lmprobe-secrets/tests/fixtures/gitleaks-default.toml <fixture-dir>
```

Normalization: JSON is pretty-printed with deterministic key order from `serde_json`; entropy floats are rounded to 12 decimal places to avoid last-bit emitter drift; no timestamps, durations, or machine-local temporary paths are expected. Raw secrets must never appear in snapshots; findings must remain redacted.

Protocol: changing any `expected.json` is a compatibility decision. Do not regenerate snapshots casually; future P2/P3 migrations must keep these byte-equal unless Reviewer signoff explicitly accepts changed behavior.

## Known gaps the baseline currently encodes

- CLOSED in v1.1 P3: `javascript/stripe_app.js` now matches the Stripe Node shape `require("stripe")(key)` as `provider_id=stripe, sdk=stripe-node-current`. The JS `expected.json` was refreshed as the planned recall improvement documented here.
- v1.1 P3 also surfaces provider metadata (`provider_id`, `provider_rule_ids`, `credential_kind`, `sdk`, `provider_shape_id`) in semantic and canonical finding JSON. This is a one-time baseline-schema bump for the DB-driven provider migration, not a scanner regression.
- The Go and Java snapshots now include additional DB-driven AWS SDK sink evidence for existing fixture call sites. That is provider-DB recall newly exposed by P3; it is intentionally captured in the refreshed snapshots under this reviewer-signoff protocol.
