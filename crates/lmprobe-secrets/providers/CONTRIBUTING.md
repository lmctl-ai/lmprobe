# Provider DB Contributions

The provider semantic DB lives in `crates/lmprobe-secrets/providers/default.toml`.
It is the source of truth for provider-specific credential-consuming SDK/API
call sites. Generic sinks such as `login`, `connect`, `authenticate`, and
`Authorization` header handling stay hardcoded in the semantic scanner and do
not belong in this DB.

## Required Provider Fields

Each `[[providers]]` entry must include:

- `id`: stable lowercase provider id.
- `description`: human-readable provider name.
- `rule_ids`: gitleaks rule IDs that join this provider to the regex layer.
- `credential_kind`: credential shape such as `api-key`, `bearer-token`, or
  `key-secret-pair`.
- `sdk_urls`: SDK or API documentation URLs used to justify the call shapes.
- `fast_path_tokens`: high-signal substrings found in source files that use the
  provider SDK. Each token must be non-empty and at least 3 characters.

This follows the provider DB schema in durable-memory design `008` section B.

## Required Language Fields

Each `[[providers.languages]]` entry must include:

- `language`: one of the v1.1 supported languages: `python`, `javascript`,
  `go`, or `java`.
- `sdk`: stable SDK id, including major version where useful.
- `sink_node_kinds`: AST node kinds this entry expects to match.
- `positive_test`: a short snippet that must match at least one `call_shape`.
- `negative_test`: a short snippet that must not match this provider.
- At least one `[[providers.languages.call_shapes]]` entry.

Do not fabricate SDK entries. If a provider has no clear mainstream SDK shape
in a language, omit that language.

## Validation And CI

CI enforces these rules:

- Every `rule_id` must exist in the vendored gitleaks config.
- The TOML schema uses `deny_unknown_fields`; typos are errors.
- Every provider must have at least one `fast_path_tokens` item, and every token
  must be non-whitespace and at least 3 characters.
- Every language entry must have at least one `call_shape`.
- `sdk` must be non-empty and unique for `(provider, language, sdk)`.
- `positive_test` must match at least one shape.
- `negative_test` must not match that provider.
- The committed provider coverage table must match the generated table.

## Diagnostics

- `PROVIDER_DB_SCHEMA_MISMATCH`: schema version mismatch, missing required
  fields, invalid uniqueness, invalid fast-path tokens, invalid snippets, or
  orphaned rule IDs.
- `PROVIDER_DB_UNKNOWN_FIELD`: the TOML contains a field not present in the
  schema.
- `PROVIDER_DB_DEPRECATED_FIELD`: a known old field was detected and should be
  removed or migrated.

## Workflow

1. Edit `crates/lmprobe-secrets/providers/default.toml`.
2. Run `cargo test -p lmprobe-secrets`.
3. Run `cargo xtask provider-coverage --check`.
4. Fix any CI diagnostics.
5. Open a PR with SDK documentation links for new or changed call shapes.

## Finding SDK Call Shapes

Prefer official SDK documentation or README examples. Good call shapes are the
places credentials are consumed by a provider SDK, for example:

- Python keyword arguments such as `boto3.client(..., aws_secret_access_key=...)`.
- JavaScript constructors such as `new Octokit({ auth: token })`.
- Go constructors or struct literals such as `sendgrid.NewSendClient(apiKey)`.
- Java builders or constructors such as `AwsBasicCredentials.create(id, secret)`.

Avoid source-like behavior such as environment-variable reads, vault clients, or
configuration loaders. Provider DB v1.1 is sinks-only; credential sources are a
future phase.

## Current Coverage

Regenerate this table with `cargo xtask provider-coverage`; CI checks it with
`cargo xtask provider-coverage --check`.

<!-- provider-coverage:start -->

| Provider | Languages | SDKs | Gitleaks rule IDs |
| --- | --- | --- | --- |
| `aws` | `go`, `java`, `javascript`, `python` | `python/boto3-v1`, `javascript/aws-sdk-js-v3`, `go/aws-sdk-go-v2`, `java/aws-sdk-java-v2`, `java/aws-sdk-java-v1` | `aws-access-token`, `aws-amazon-bedrock-api-key-long-lived`, `aws-amazon-bedrock-api-key-short-lived` |
| `stripe` | `go`, `java`, `javascript`, `python` | `python/stripe-python-current`, `javascript/stripe-node-current`, `go/stripe-go-v85`, `java/stripe-java-current` | `stripe-access-token` |
| `github` | `go`, `java`, `javascript`, `python` | `python/pygithub-current`, `javascript/octokit-js-current`, `go/go-github-current`, `java/github-api-java-current` | `github-app-token`, `github-fine-grained-pat`, `github-oauth`, `github-pat`, `github-refresh-token` |
| `slack` | `go`, `java`, `javascript`, `python` | `python/slack-sdk-python-v3`, `javascript/slack-web-api-js-current`, `go/slack-go-current`, `java/slack-api-client-java-current` | `slack-app-token`, `slack-bot-token`, `slack-config-access-token`, `slack-config-refresh-token`, `slack-legacy-bot-token`, `slack-legacy-token`, `slack-legacy-workspace-token`, `slack-user-token`, `slack-webhook-url` |
| `twilio` | `go`, `java`, `javascript`, `python` | `python/twilio-python-v9`, `javascript/twilio-node-current`, `go/twilio-go-current`, `java/twilio-java-current` | `twilio-api-key` |
| `sendgrid` | `go`, `java`, `javascript`, `python` | `python/sendgrid-python-current`, `javascript/sendgrid-nodejs-current`, `go/sendgrid-go-v4`, `java/sendgrid-java-current` | `sendgrid-api-token` |
| `openai` | `go`, `java`, `javascript`, `python` | `python/openai-python-v1`, `javascript/openai-node-v4`, `go/openai-go-current`, `java/openai-java-current` | `openai-api-key` |
| `gitlab` | `go`, `java`, `javascript`, `python` | `python/python-gitlab-current`, `javascript/gitbeaker-rest-current`, `go/go-gitlab-current`, `java/gitlab4j-api-current` | `gitlab-deploy-token`, `gitlab-oauth-app-secret`, `gitlab-pat`, `gitlab-pat-routable` |
| `cloudflare` | `go`, `javascript`, `python` | `python/cloudflare-python-current`, `javascript/cloudflare-typescript-current`, `go/cloudflare-go-current` | `cloudflare-api-key`, `cloudflare-global-api-key`, `cloudflare-origin-ca-key` |
| `datadog` | `go`, `java`, `javascript`, `python` | `python/datadog-api-client-python-current`, `javascript/datadog-api-client-typescript-current`, `go/datadog-api-client-go-v2`, `java/datadog-api-client-java-current` | `datadog-access-token` |

<!-- provider-coverage:end -->
