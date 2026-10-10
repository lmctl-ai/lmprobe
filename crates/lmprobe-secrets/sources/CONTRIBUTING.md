# Source DB Contributions

The credential source DB lives in `crates/lmprobe-secrets/sources/default.toml`.
It is the source of truth for credential-obtaining call sites and environment
reads. Provider sink recipes live in `providers/default.toml`; generic sinks
and source-flow semantics stay in Rust.

## Required Source Fields

Each `[[sources]]` entry must include:

- `id`: stable lowercase source id.
- `description`: human-readable source description.
- `category`: one of `env`, `vault`, `secret-manager`, `dotenv`, or
  `config-file`.
- `provider_id`: optional provider DB id when the source is provider-specific.
- `credential_kind`: credential shape such as `api-key`, `bearer-token`, or
  `external-secret`.
- `env_var_names`: required for `category = "env"`; omit or leave empty for
  non-env sources.
- `secret_name_regex`: reserved forward hook for secret-manager sources. Omit
  this field for v1.2 secret-manager sources. Do not write
  `secret_name_regex = ""`; the field is reserved for v1.3 string-literal-based
  provider inference and should be set explicitly only when that matcher ships.
- `source_urls`: documentation URLs used to justify the read shapes.
- `fast_path_tokens`: high-signal substrings used before AST parsing. Use
  curated environment variable names or SDK identifiers only. Do not use generic
  env APIs such as `os.environ`, `process.env`, `System.getenv`, or
  `os.Getenv`.

## Required Language Fields

Each `[[sources.languages]]` entry must include:

- `language`: one of the v1.2 supported languages: `python`, `javascript`,
  `go`, or `java`.
- `sdk`: stable SDK id, including major version where useful.
- `source_node_kinds`: AST node kinds this entry expects to match.
- `positive_test`: a short snippet that must match at least one `read_shape`.
- `negative_test`: a short snippet that must not match this source.
- At least one `[[sources.languages.read_shapes]]` entry.

Do not fabricate SDK entries. If a source has no clear mainstream SDK shape in
a language, omit that language.

## Read Shape Kinds

- `env_read`: explicit environment or Java system-property reads, for example
  `os.environ["AWS_SECRET_ACCESS_KEY"]`, `process.env.OPENAI_API_KEY`,
  `os.Getenv("OPENAI_API_KEY")`, or `System.getProperty("OPENAI_API_KEY")`.
- `kv_get`: Vault-style key/value reads where the configured key/path argument
  is part of the source evidence.
- `sdk_call`: secret-manager SDK calls such as `get_secret_value`,
  `accessSecretVersion`, or `getSecret`.
- `dotenv_load`: dotenv side-effect loaders such as `load_dotenv()` or
  `require("dotenv").config()`. These emit `source_evidence` only and never
  enter confirmed-flow binding because the call does not return a credential
  value.
- `config_lookup`: explicit config object lookups by credential-bearing key.

## Validation And CI

CI enforces these rules:

- Every non-empty `provider_id` must exist in the provider DB.
- The TOML schema uses `deny_unknown_fields`; typos are errors.
- Every source must have at least one `fast_path_tokens` item, and every token
  must be non-whitespace and at least 3 characters.
- Generic env API fast-path tokens are rejected; use curated env names or SDK
  identifiers instead.
- Every language entry must have at least one `read_shape`.
- `sdk` must be non-empty and unique for `(source, language, sdk)`.
- `positive_test` must match at least one shape.
- `negative_test` must not match any shape.
- The committed source coverage table must match the generated table.

## Diagnostics

- `SOURCE_DB_SCHEMA_MISMATCH`: schema version mismatch, missing required
  fields, invalid uniqueness, invalid fast-path tokens, invalid snippets, or
  orphaned provider IDs.
- `SOURCE_DB_UNKNOWN_FIELD`: the TOML contains a field not present in the
  schema.
- `SOURCE_DB_DEPRECATED_FIELD`: a known old field was detected and should be
  removed or migrated.
- `SOURCE_DB_GENERIC_FAST_PATH_TOKEN`: `fast_path_tokens` contains a generic env
  API rather than a curated source token.

## Workflow

1. Edit `crates/lmprobe-secrets/sources/default.toml`.
2. Run `cargo test -p lmprobe-secrets`.
3. Run `cargo xtask source-coverage --check`.
4. Fix any CI diagnostics.
5. Open a PR with SDK documentation links for new or changed read shapes.

## Finding SDK Read Shapes

Prefer official SDK documentation or README examples. Good read shapes are
places credentials or secret material are explicitly obtained, for example:

- Python env reads such as `os.environ["OPENAI_API_KEY"]`.
- JavaScript secret-manager calls such as `client.accessSecretVersion(...)`.
- Go secret-manager calls such as `client.AccessSecretVersion(...)`.
- Java calls such as `secretClient.getSecret(...)`.

Avoid implicit runtime credential chains. For example, `boto3.client("s3")`
without explicit credential arguments is intentionally invisible to structural
source analysis.

## Current Coverage

Regenerate this table with `cargo xtask source-coverage`; CI checks it with
`cargo xtask source-coverage --check`.

<!-- source-coverage:start -->

| Source | Category | Languages | SDKs | Provider | Env vars |
| --- | --- | --- | --- | --- | --- |
| `env-aws` | `env` | `go`, `java`, `javascript`, `python` | `python/stdlib-env`, `javascript/node-env`, `go/stdlib-env`, `java/stdlib-env` | `aws` | `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN` |
| `env-stripe` | `env` | `go`, `java`, `javascript`, `python` | `python/stdlib-env`, `javascript/node-env`, `go/stdlib-env`, `java/stdlib-env` | `stripe` | `STRIPE_API_KEY`, `STRIPE_SECRET_KEY` |
| `env-github` | `env` | `go`, `java`, `javascript`, `python` | `python/stdlib-env`, `javascript/node-env`, `go/stdlib-env`, `java/stdlib-env` | `github` | `GITHUB_TOKEN`, `GH_TOKEN`, `GITHUB_PAT` |
| `hashicorp-vault-kv` | `vault` | `go`, `java`, `javascript`, `python` | `python/hvac-v1`, `javascript/node-vault-current`, `go/vault-api-v1`, `java/vault-java-current` | - | - |
| `aws-secrets-manager` | `secret-manager` | `go`, `java`, `javascript`, `python` | `python/boto3-secretsmanager-v1`, `javascript/aws-sdk-js-v3-secretsmanager`, `go/aws-sdk-go-v2-secretsmanager`, `java/aws-sdk-java-v2-secretsmanager` | - | - |
| `dotenv-loader` | `dotenv` | `go`, `java`, `javascript`, `python` | `python/python-dotenv-current`, `javascript/dotenv-node-current`, `go/godotenv-current`, `java/java-dotenv-current` | - | - |
| `gcp-secret-manager` | `secret-manager` | `go`, `java`, `javascript`, `python` | `python/google-cloud-secret-manager-python-current`, `javascript/google-cloud-secret-manager-node-current`, `go/google-cloud-secretmanager-go-current`, `java/google-cloud-secretmanager-java-current` | - | - |
| `azure-key-vault` | `secret-manager` | `go`, `java`, `javascript`, `python` | `python/azure-keyvault-secrets-python-current`, `javascript/azure-keyvault-secrets-js-current`, `go/azure-sdk-for-go-azsecrets-current`, `java/azure-security-keyvault-secrets-java-current` | - | - |
| `kubernetes-secret` | `secret-manager` | `go`, `java`, `javascript`, `python` | `python/kubernetes-python-current`, `javascript/k8s-client-node-current`, `go/client-go-current`, `java/kubernetes-java-client-current` | - | - |
| `env-openai` | `env` | `go`, `java`, `javascript`, `python` | `python/stdlib-env`, `javascript/node-env`, `go/stdlib-env`, `java/stdlib-env` | `openai` | `OPENAI_API_KEY` |

<!-- source-coverage:end -->
