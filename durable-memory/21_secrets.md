# Secret Scanning in lmprobe

`lmprobe secrets <path>` is lmprobe's native secret-scanning capability. It
integrates the gitleaks TOML rule format for regex-based detection and adds
lmprobe's AST-aware symbol graph for semantic evidence that regex-only scanning
cannot connect.

The scanner has two layers:

1. **Gitleaks-format rule layer**: parses gitleaks-style TOML rules and applies keyword prefiltering, regex matching, `secretGroup`/capture extraction, entropy gates, allowlists, and redaction.
2. **lmprobe semantic evidence layer**: AST/symbol reachability for cases a pure regex scanner cannot connect, such as a hardcoded literal assigned in one module and passed to a credential sink through an imported variable.

This verb is not a full taint engine and not a bug-for-bug implementation of
another scanner. It is designed for low-noise evidence with explicit confidence
labels while remaining compatible with gitleaks-format rule corpora.

## Invocation

```sh
lmprobe secrets .
lmprobe --format json secrets src
lmprobe secrets --config gitleaks.toml src
lmprobe secrets --gitleaks-compat /path/to/corpus
lmprobe secrets --source git /path/to/repo
```

The default config is the vendored upstream gitleaks default config. When `--config` is omitted and the source path contains `.gitleaks.toml`, lmprobe loads that file and honors `[extend] useDefault = true` or `[extend] path = "..."` before scanning. `--config` accepts a gitleaks-style TOML config and is treated as explicit.

By default, filesystem walking is lmprobe-native and respects ignore files through the normal walker. `--gitleaks-compat` is for rule-corpus parity checks against gitleaks-style directory scanning: it disables ignore-file behavior and uses gitleaks-style walking assumptions where implemented.

### Locale/i18n bundle noise recipe (opt-in)

Default behavior keeps rule compatibility intact: locale bundles are still scanned
and reported if they match a rule. If your team has recurring locale/i18n noise,
use an explicit config overlay for opt-in suppression.

Example `secrets-locale-noise.gitleaks.toml`:

```toml
title = "secrets-locale-noise"
minVersion = "v8.25.0"

[extend]
useDefault = true

[allowlist]
paths = [
  '''(?i)(^|/)assets/i18n/.*''',
  '''(?i)(^|/)locales/.*''',
  '''(?i)(^|/)translations/.*''',
]
```

Run:

```sh
lmprobe secrets --config secrets-locale-noise.gitleaks.toml --format json .
```

This is opt-in and reversible: keep it off unless the team wants locale bundles
excluded. For teams that leave locale scans enabled, downweight decoded-only
natural-language translation hits in ranking logic instead of suppressing them by
default.

`--source git` scans git history with `git log -p -U0 --full-history --all --diff-filter=tuxdb`. The rule layer scans added patch content only; semantic AST flow does not run over historical blobs.

## GraphQL Recipes

Provider DB entries auto-generate GraphQL recipes against the `callsites` resolver. `callsites` is the supported recipe-execution surface for credential-consuming SDK/API call sites: recipes lower provider `call_shapes` into structured filters such as `argNames`, `positionalIndices`, `structFields`, `assignmentTargets`, and `importHints`. There is no monolithic provider-sink resolver; users can run or adapt the generated GraphQL directly with `lmprobe query`.

## Provider Recipes (v1.1)

`lmprobe secrets-recipes` exposes the provider DB for discovery and inspection.
Recipes are auto-generated from `call_shapes`; contributors edit TOML, not
hand-written GraphQL. Execute recipes through `lmprobe query --recipe`.

```sh
lmprobe secrets-recipes list
lmprobe secrets-recipes list --language python --provider aws --format json
lmprobe secrets-recipes show aws/python
lmprobe secrets-recipes show aws/java/aws-sdk-java-v2
lmprobe query --recipe secrets/aws/python --var path=./src
lmprobe query --recipe secrets/aws/python --all-shapes --var path=./src
```

Recipe keys use `<provider>/<language>[/<sdk>]`. The `sdk` segment is optional only when the provider has a single SDK entry for that language. For example, `aws/java` is ambiguous because the DB ships both `aws-sdk-java-v1` and `aws-sdk-java-v2`, so use `aws/java/aws-sdk-java-v2`.

`secrets-recipes list` enumerates `(provider_id, language, sdk, shape_index)` rows. `show` prints the generated GraphQL recipe text by default, or wraps it with metadata under `--format json`.

`lmprobe query --recipe secrets/<provider>/<language>[/<sdk>]` is the canonical execution path. Pass the target path as a GraphQL variable, for example `--var path=./src`. Without `--all-shapes`, it executes the first shape for the selected key, matching `show`; with `--all-shapes`, it executes every `call_shape` under that provider/language/sdk.

Recipe output redacts callsite argument values by default. The generated `callsites` recipe uses `redactArgValues: true`, and the response includes `argValuesRedacted` so automation can tell whether any argument text was replaced with `REDACTED`. Power users can run custom GraphQL with `redactArgValues: false` when they need raw local evidence.

These recipes follow the scanner-recipe doctrine from `durable-memory/19_scanner_recipes.md`: they return AST callsite evidence, not vulnerability verdicts. The secrets scanner may use the same provider DB to enrich findings, but recipe execution itself does not claim a secret exists.

## Provider Source Recipes (v1.2)

`lmprobe secrets-sources` exposes the source DB for discovery and inspection.
Recipes are auto-generated from `read_shapes`; contributors edit TOML, not
hand-written GraphQL. Execute recipes through `lmprobe query --recipe`.

```sh
lmprobe secrets-sources list
lmprobe secrets-sources list --language python --source env-aws --format json
lmprobe secrets-sources list --category env --provider aws
lmprobe secrets-sources show env-aws/python
lmprobe query --recipe secrets-source/env-aws/python --var path=./src
lmprobe query --recipe secrets-source/env-aws/python --all-shapes --var path=./src
```

Recipe keys use `<source>/<language>[/<sdk>]`. The `sdk` segment is optional only when the source has a single SDK entry for that language; if a future source ships multiple SDK entries for a language, include the SDK segment shown by `secrets-sources list`.

`secrets-sources list` enumerates `(source_id, language, sdk, read_shape_index)` rows. `show` prints the generated GraphQL recipe text by default, or wraps it with metadata under `--format json`.

`lmprobe query --recipe secrets-source/<source>/<language>[/<sdk>]` is the canonical execution path. Pass the target path as a GraphQL variable, for example `--var path=./src`. Without `--all-shapes`, it executes the first shape for the selected key, matching `show`; with `--all-shapes`, it executes every `read_shape` under that source/language/sdk.

Source recipe output redacts sensitive payloads by default. Generated `sourceReads` recipes use `redactArgValues: true`, and the response includes `argValuesRedacted` so automation can tell whether any source text was replaced with `REDACTED`. Environment variable names such as `AWS_SECRET_ACCESS_KEY` are public taxonomy and are not redacted; sensitive paths and identifiers such as Vault secret paths are redacted. Power users can run custom GraphQL with `redactArgValues: false` when they need raw local evidence.

Source recipes follow the scanner-recipe doctrine from `durable-memory/19_scanner_recipes.md`: they return AST source-read evidence, not vulnerability verdicts. A source read shows how code obtains credentials; it does not prove a secret leak or a confirmed source-to-sink flow by itself.

### Source Coverage

Regenerate this table with `cargo xtask source-coverage`; CI checks it with `cargo xtask source-coverage --check`.

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

### Provider Coverage

Regenerate this table with `cargo xtask provider-coverage`; CI checks it with `cargo xtask provider-coverage --check`.

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

## Modes

- `--ast-fp-cut=off|annotate|suppress`
  - `annotate` is the default. It keeps Layer-1 findings and adds `ast_context`.
  - `suppress` moves conservative non-code contexts into `suppressed`.
  - `string-doc` is annotated but not suppressed because real secrets can live in docstrings.
- `--sink-evidence=off|on`
  - Default is `off`.
  - When enabled, lmprobe reports syntax-only credential sink evidence even when no literal or rule hit backs it. This is intentionally low-confidence and can be noisy.

## Output

The JSON envelope schema is `lmprobe.secrets.v1`. The main arrays are:

- `findings`: gitleaks-rule Layer-1 findings.
- `semantic_findings`: bounded symbol-reachability evidence.
- `sink_evidence`: syntax-only sink evidence when `--sink-evidence=on`.
- `canonical_findings`: de-duplicated logical findings with strongest detector priority.
- `suppressed`: findings moved out of active results by `--ast-fp-cut=suppress`.
- `warnings`: diagnostics such as unresolved imports or cross-file depth-cap skips.
- `partial`: emitted as `true` when filesystem coverage was incomplete because a selected file's metadata was unreadable, the file was too large, the file was unreadable, or the walk entry failed.

Semantic findings may include `flow[]` steps:

```json
[
  {"kind": "definition", "file_path": "a.js", "line": 1, "symbol": "SECRET"},
  {"kind": "export", "file_path": "a.js", "line": 1, "symbol": "SECRET"},
  {"kind": "import", "file_path": "b.js", "line": 1, "symbol": "SECRET"},
  {"kind": "sink", "file_path": "b.js", "line": 2, "symbol": "SECRET"}
]
```

v1.2 reserves an additive `source` flow-step kind for explicit credential-source
evidence such as environment reads and secret-manager calls. Scanner emission for
source steps lands with the source-flow integration phase; existing v1.1 flows
continue to use `definition`, `export`, `import`, and `sink`.

Secrets are redacted by default in display, debug, and serialized output. Raw secret values are not written to lmprobe's cache.

## Exit Codes

The verb follows lmprobe exit semantics:

- matched evidence: `0`
- no match: `1`
- command/runtime error: `2`
- partial filesystem coverage: `3` on the normal lmprobe output path; `exit_status` remains `matched` or `no_match` and the envelope includes `partial: true`

Use the global `--exit-zero-on-no-match` behavior where supported by the top-level CLI.

## Semantic Layer

The semantic layer is **bounded symbol-graph reachability**, not taint analysis. It handles direct literal-to-variable-to-sink flow and one import/export hop for Python, JS/TS, Go, and Java.

The cross-file graph is capped at one hop. Re-export chains produce a warning and no finding.

Resolver assumptions:

- Python: relative/same-directory `from module import name`.
- JS/TS: relative imports such as `./a.js`, `./a.ts`, and simple named imports.
- Go: relative sub-folder imports used in source fixtures; no stdlib, module-path, GOPATH, or `go.mod` resolution.
- Java: best-effort class-name/path suffix matching; no classpath, build-tool, inheritance, or reflection resolution.

## Known v1 Limits

- No f-string, string concat, or template-literal value evaluation.
- No container/object/index flow such as `d["k"]`, object properties, maps, arrays, or struct field propagation beyond the explicit Java field cases already modeled.
- No dynamic dispatch, reflection, `eval`, `getattr`, `obj[name](...)`, or Java reflection.
- JS destructured imports and destructured local bindings are conservative: destructured names are declared, but not value-resolved.
- Go resolver is relative-subfolder only; it does not resolve standard library names, module paths, GOPATH, vendor trees, or `go.mod`.
- Java resolver is best-effort by source path/class name and does not model full classpaths.
- Git-history mode supports gitleaks-format rules but is not bug-for-bug identical to gitleaks patch-fragment handling. On 2026-05-19, same-config parity on `$LMPROBE_GITLEAKS_REPO` after exact `.gitleaksignore` suppression was: gitleaks 273, lmprobe 272 raw findings (267 unique fingerprints), strict fingerprint overlap 257, gitleaks-only 16, lmprobe-only 10. Remaining differences are concentrated in multi-line patch fragments and generated-rule test literals where gitleaks scans the full added text fragment and lmprobe currently scans added lines.
- `--ast-fp-cut=suppress` can suppress real secrets in test/generated paths because `test-or-generated-path` is a path heuristic. Default `annotate` does not remove those findings.
- Runtime indirection such as `key = readFile("key.txt")` cannot reveal the secret value statically unless the external file itself is scanned by the rule layer.

## Cache Safety

The secret type redacts in `Debug`, `Display`, and `Serialize`. Raw access is restricted to in-memory detector logic. The semantic graph stores file paths, symbols, line/column evidence, and redacted literals; it does not store AST nodes or whole source files in persistent cache.

## Surface Design Rationale (Kept-Both)

This section records the post-v1.2 surface audit. The consolidation rule is:
when two surfaces answer the same question and return byte-equivalent results,
keep the simpler/general surface. The removed recipe `run` subcommands fell into
that category; `lmprobe query --recipe ... --var path=...` is now the only recipe
execution path. The surfaces below remain because they answer different questions
or return different shapes.

- `callers` vs `callsites`: `callers` is symbol-centric: "who calls this symbol?"
  It participates in caller/callee graph workflows. `callsites` is
  filter-centric: "which call/new/struct/assignment sites match this structured
  argument shape?" It powers provider recipes and returns `lmprobe.callsites.v1`.
- `Query.secrets` vs `Query.callsites`: `Query.secrets` runs the full secret
  scanner and returns the same secrets envelope as the CLI path, including rule
  findings, semantic findings, canonical findings, suppressions, and scanner
  warnings. `Query.callsites` returns AST callsite evidence only; it does not
  claim a secret exists.
- `Query.secrets` vs `Query.sourceReads`: `Query.secrets` reports sink/value
  findings and scanner-level evidence. `Query.sourceReads` reports credential
  source-read evidence such as env reads, Vault paths, or secret-manager calls.
  Sources and sinks are different evidence shapes.
- `lmprobe secrets` CLI vs `lmprobe query '{ secrets(...) }'`: both call the same
  scanner engine, but they serve different users. The CLI is the ergonomic
  command-line scanning surface with normal output formats and exit-code
  behavior; GraphQL is the composable resolver surface for agents and tools.
- `secrets-recipes list/show` vs `lmprobe query --recipe secrets/...`: list/show
  answer discovery and inspection questions: "what provider recipes exist?" and
  "what GraphQL will this recipe generate?" `query --recipe` executes a selected
  recipe and returns evidence.
- `secrets-sources list/show` vs `lmprobe query --recipe secrets-source/...`:
  same split as provider recipes, but for source-read recipes. Discovery and
  inspection stay in `secrets-sources`; execution stays in `query --recipe`.
