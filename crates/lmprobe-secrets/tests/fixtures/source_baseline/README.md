# Source Baseline Snapshots

These fixtures are the v1.2 P0.5 non-lossy migration proof for explicit credential-source flow. The credentials and environment names in these files are synthetic test values, never real secrets.

Fixtures:
- `python/boto3_env_app.py`: AWS env read into `boto3.client(... aws_secret_access_key=...)`, plus Stripe env read into `stripe.api_key`.
- `javascript/stripe_env_app.js`: Stripe env read into `require("stripe")(key)`, plus GitHub token env subscript into `new Octokit({ auth: token })`.
- `go/aws_env_app.go`: AWS env read into `credentials.NewStaticCredentialsProvider`, plus Stripe env read into `stripe.Key`.
- `java/AwsEnvApp.java`: AWS secret env read and AWS access key system-property read into `new BasicAWSCredentials(id, secret)`.

Each `expected.json` is captured from the current shipped CLI with:

```sh
./target/debug/lmprobe secrets --format json --gitleaks-compat --config crates/lmprobe-secrets/tests/fixtures/gitleaks-default.toml <fixture-dir>
```

Normalization: JSON is pretty-printed with deterministic key order from `serde_json`; entropy floats are rounded to 12 decimal places to avoid last-bit emitter drift; no timestamps, durations, or machine-local temporary paths are expected. Raw secrets must never appear in snapshots; findings must remain redacted.

Protocol: changing any `expected.json` is a compatibility decision. Do not regenerate snapshots casually; future source-flow migrations must keep these byte-equal unless Reviewer signoff explicitly accepts changed behavior, such as adding `source_evidence[]` or `confirmed_flows[]`.

## Known gaps the source baseline currently encodes

- Python `source_evidence[]` and `confirmed_flows[]` are CLOSED in v1.2 P2. The Python snapshot was refreshed under the reviewer-signoff protocol to include AWS and Stripe env-read confirmed flows.
- JS/Go/Java `source_evidence[]` and `confirmed_flows[]` are CLOSED in v1.2 P3. Those snapshots were refreshed under the reviewer-signoff protocol to include source-origin flows for Stripe, GitHub, AWS, and Java `System.getProperty(...)`.
- Remaining v1.3 candidates: container aliasing (`const env = process.env;`), secret-manager provider inference from `secret_name_regex`, reassignment chains beyond the current cap, container/object indexing, dynamic dispatch, and runtime-derived key names.
