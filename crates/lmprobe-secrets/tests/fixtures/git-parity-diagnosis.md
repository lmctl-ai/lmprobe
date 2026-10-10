# Git History Parity Diagnosis (Phase 6.1)

Generated against `$LMPROBE_GITLEAKS_REPO` on 2026-05-19 using explicit upstream default config `$LMPROBE_GITLEAKS_REPO/config/gitleaks.toml`.

Commands:

```sh
$LMPROBE_GITLEAKS_REPO/gitleaks git $LMPROBE_GITLEAKS_REPO --config $LMPROBE_GITLEAKS_REPO/config/gitleaks.toml --no-color --redact --report-format json --report-path /tmp/gitleaks-git-config.json
./target/debug/lmprobe --format json secrets --source git --gitleaks-compat --config $LMPROBE_GITLEAKS_REPO/config/gitleaks.toml $LMPROBE_GITLEAKS_REPO > /tmp/lmprobe-git-config.json
```

## Normalized Counts

| Set | Count |
| --- | ---: |
| gitleaks | 273 |
| lmprobe | 50 |
| overlap | 49 |
| gitleaks-only | 224 |
| lmprobe-only | 1 |

Normalized key: full gitleaks fingerprint `commit:path:rule-id:line`.

## Delta Buckets By Rule And Path

| rule_id | path bucket | overlap | gitleaks-only | lmprobe-only |
| --- | --- | ---: | ---: | ---: |
| `aws-access-token` | `detect/` | 0 | 150 | 0 |
| `generic-api-key` | `detect/` | 0 | 20 | 0 |
| `generic-api-key` | `cmd/generate/` | 3 | 18 | 1 |
| `curl-auth-header` | `cmd/generate/` | 0 | 8 | 0 |
| `private-key` | `detect/` | 4 | 5 | 0 |
| `curl-auth-user` | `cmd/generate/` | 0 | 3 | 0 |
| `discord-api-token` | `detect/` | 0 | 3 | 0 |
| `anthropic-admin-api-key` | `cmd/generate/` | 0 | 2 | 0 |
| `anthropic-api-key` | `cmd/generate/` | 0 | 2 | 0 |
| `aws-amazon-bedrock-api-key-short-lived` | `cmd/generate/` | 0 | 2 | 0 |
| `databricks-api-token` | `cmd/generate/` | 0 | 2 | 0 |
| `private-key` | `cmd/generate/` | 0 | 2 | 0 |
| `sourcegraph-access-token` | `cmd/generate/` | 5 | 1 | 0 |
| `cloudflare-origin-ca-key` | `cmd/generate/` | 0 | 1 | 0 |
| `sidekiq-secret` | `detect/` | 0 | 1 | 0 |
| `sidekiq-sensitive-url` | `detect/` | 0 | 1 | 0 |
| `slack-config-access-token` | `cmd/generate/` | 0 | 1 | 0 |
| `slack-legacy-workspace-token` | `cmd/generate/` | 0 | 1 | 0 |
| `slack-user-token` | `cmd/generate/` | 0 | 1 | 0 |
| `openai-api-key` | `cmd/generate/` | 9 | 0 | 0 |
| `1password-secret-key` | `cmd/generate/` | 4 | 0 | 0 |
| `1password-secret-key` | `detect/` | 4 | 0 | 0 |
| `notion-api-token` | `cmd/generate/` | 3 | 0 | 0 |
| `atlassian-api-token` | `cmd/generate/` | 2 | 0 | 0 |
| `aws-access-token` | `testdata/` | 2 | 0 | 0 |
| `heroku-api-key-v2` | `cmd/generate/` | 2 | 0 | 0 |
| `aws-amazon-bedrock-api-key-long-lived` | `cmd/generate/` | 1 | 0 | 0 |
| `clickhouse-cloud-api-secret-key` | `cmd/generate/` | 1 | 0 | 0 |
| `generic-api-key` | `config/` | 1 | 0 | 0 |
| `generic-api-key` | `testdata/` | 1 | 0 | 0 |
| `openshift-user-token` | `cmd/generate/` | 1 | 0 | 0 |
| `perplexity-api-key` | `cmd/generate/` | 1 | 0 | 0 |
| `slack-app-token` | `cmd/generate/` | 1 | 0 | 0 |
| `slack-bot-token` | `cmd/generate/` | 1 | 0 | 0 |
| `slack-config-refresh-token` | `cmd/generate/` | 1 | 0 | 0 |
| `slack-legacy-bot-token` | `cmd/generate/` | 1 | 0 | 0 |
| `slack-legacy-token` | `cmd/generate/` | 1 | 0 | 0 |

## Delta Buckets By Commit Recency

| commit recency | overlap | gitleaks-only | lmprobe-only |
| --- | ---: | ---: | ---: |
| `181-365d` | 11 | 144 | 1 |
| `1-2y` | 38 | 80 | 0 |

## Diagnosis

- `lmprobe` gitleaks-only misses matching an exact `.gitleaksignore` entry: 0/224.
- `lmprobe` gitleaks-only misses matching only the same `(file, rule)` pair as some `.gitleaksignore` entry: 217/224.
- Root cause for the dominant bucket: lmprobe suppresses any finding whose `(file_path, rule_id)` matches a `.gitleaksignore` entry, while gitleaks suppresses only the exact global fingerprint `path:rule:line` or exact commit fingerprint `commit:path:rule:line`. This over-suppresses historical findings in `detect/detect_test.go` and generated rule files.
- Remaining same-config misses after excluding broad `.gitleaksignore` suppression are 7 findings: 2 Amazon Bedrock short-lived, 4 Anthropic, and 1 Sourcegraph token in generated rule source. These need separate rule/match parity inspection after the suppression fix.
- The single lmprobe-only finding is `87d96295d65a52f2bbeb6123a755b7d5f076f937:cmd/generate/config/rules/aws.go:generic-api-key:99`; it is not caused by `.gitleaksignore`.

## Non-Broad-Suppression Misses

| fingerprint | rule_id | path | line |
| --- | --- | --- | ---: |
| `72977e41b60063f547c2fa2324365805ad43ac3e:cmd/generate/config/rules/anthropic.go:anthropic-admin-api-key:34` | `anthropic-admin-api-key` | `cmd/generate/config/rules/anthropic.go` | 34 |
| `72977e41b60063f547c2fa2324365805ad43ac3e:cmd/generate/config/rules/anthropic.go:anthropic-admin-api-key:54` | `anthropic-admin-api-key` | `cmd/generate/config/rules/anthropic.go` | 54 |
| `72977e41b60063f547c2fa2324365805ad43ac3e:cmd/generate/config/rules/anthropic.go:anthropic-api-key:65` | `anthropic-api-key` | `cmd/generate/config/rules/anthropic.go` | 65 |
| `72977e41b60063f547c2fa2324365805ad43ac3e:cmd/generate/config/rules/anthropic.go:anthropic-api-key:23` | `anthropic-api-key` | `cmd/generate/config/rules/anthropic.go` | 23 |
| `87d96295d65a52f2bbeb6123a755b7d5f076f937:cmd/generate/config/rules/aws.go:aws-amazon-bedrock-api-key-short-lived:91` | `aws-amazon-bedrock-api-key-short-lived` | `cmd/generate/config/rules/aws.go` | 91 |
| `c6424a6fc3775e77fb75d843bf8cddda9cb19e61:cmd/generate/config/rules/sourcegraph.go:sourcegraph-access-token:31` | `sourcegraph-access-token` | `cmd/generate/config/rules/sourcegraph.go` | 31 |
| `87d96295d65a52f2bbeb6123a755b7d5f076f937:cmd/generate/config/rules/aws.go:aws-amazon-bedrock-api-key-short-lived:99` | `aws-amazon-bedrock-api-key-short-lived` | `cmd/generate/config/rules/aws.go` | 99 |

## Lmprobe-Only Findings

| fingerprint | rule_id | path | line |
| --- | --- | --- | ---: |
| `87d96295d65a52f2bbeb6123a755b7d5f076f937:cmd/generate/config/rules/aws.go:generic-api-key:99` | `generic-api-key` | `cmd/generate/config/rules/aws.go` | 99 |
