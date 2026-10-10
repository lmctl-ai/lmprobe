# Gitleaks Test Port Coverage

Port status for selected upstream gitleaks tests. Upstream project: https://github.com/gitleaks/gitleaks (MIT).

Status values:

- PASS: ported as a running Rust regression.
- IGNORED: ported as `#[ignore]` with a tracked v1.1 gap.
- SKIPPED-NOT-APPLICABLE: intentionally not ported because lmprobe consumes gitleaks output rather than implementing that subsystem.

Totals: 45 rows; 23 PASS, 21 IGNORED, 1 SKIPPED-NOT-APPLICABLE.

## Master Task #12: v1.1 Gitleaks Port Gaps

- Commit allowlists in git fragments: `config/allowlist_test.go::TestCommitAllowed`.
- Library-level config extension resolver fixtures: `config/config_test.go::TestTranslateExtend`.
- Staged/pre-commit git mode: `detect/detect_test.go::TestFromGitStaged`.
- Archive extraction source mode: `detect/detect_test.go::TestDetectWithArchives`.
- Windows separator parity: `detect/detect_test.go::TestWindowsFileSeparator_RulePath`, `TestWindowsFileSeparator_RuleAllowlistPaths`.
- Baseline CSV/SARIF loading if required beyond JSON: `detect/baseline_test.go::TestFileLoadBaseline`.
- Report byte-equality/template output: `report/*_test.go`.
- SARIF/CSV/JUnit/template structural report parity beyond JSON envelope: `report/{sarif,csv,junit,template}_test.go`.
- Go-specific source internals: `sources/common_test.go::Test_readUntilSafeBoundary`, `sources/git_test.go` channel-level tests.
- Full baseline metadata equivalence beyond fingerprint suppression: `detect/baseline_test.go::TestIsNew`.
- SCM web-link generation: `detect/utils_test.go::Test_createScmLink`.
- Reader/streaming reader source API: `detect/reader_test.go::TestDetectReader`, `TestStreamDetectReader`.

## config/allowlist_test.go

| Go test function | Rust port | Status | Notes |
| --- | --- | --- | --- |
| `TestCommitAllowed` | `gitleaks_ports/allowlist.rs::gitleaks_config_allowlist_test_commit_allowed` | IGNORED | Commit-bearing allowlist evaluation is a v1.1 git-fragment gap. |
| `TestRegexAllowed` | `gitleaks_ports/allowlist.rs::gitleaks_config_allowlist_test_regex_allowed` | PASS | Global regex allowlist suppresses a rule hit. |
| `TestPathAllowed` | `gitleaks_ports/allowlist.rs::gitleaks_config_allowlist_test_path_allowed` | PASS | Global path allowlist suppresses a rule hit. |
| `TestValidate` | `gitleaks_ports/allowlist.rs::gitleaks_config_allowlist_test_validate` | PASS | Empty allowlists error; commits/stopwords normalize and duplicate allowlists collapse. Closed by this commit. |

## config/config_test.go

| Go test function | Rust port | Status | Notes |
| --- | --- | --- | --- |
| `TestTranslate` | `gitleaks_ports/config.rs::gitleaks_config_test_translate_loads_rules_and_keywords` | PASS | Default config loads 222 rules and lowercased keyword set. |
| `TestTranslateAllowlists` | `gitleaks_ports/config.rs::gitleaks_config_test_translate_allowlists` | PASS | Global and rule allowlists parse and suppress findings. |
| `TestTranslateExtend` | `gitleaks_ports/config.rs::gitleaks_config_test_translate_extend` | IGNORED | CLI resolves `[extend]`; library fixture resolver remains v1.1. |
| `TestExtendedRuleKeywordsAreDowncase` | `gitleaks_ports/config.rs::gitleaks_config_test_extended_rule_keywords_are_downcase` | PASS | Rule keywords are normalized to lowercase. |

## detect/detect_test.go

| Go test function | Rust port | Status | Notes |
| --- | --- | --- | --- |
| `TestDetect` | `gitleaks_ports/detect.rs::gitleaks_detect_test_detect_basic_rule_and_rule_allowlist` | PASS | Basic AWS rule hit and allowlisted negative. |
| `TestFromGit` | `gitleaks_ports/detect.rs::gitleaks_detect_test_from_git` | PASS | Synthetic committed secret is found in git history. |
| `TestFromGitStaged` | `gitleaks_ports/detect.rs::gitleaks_detect_test_from_git_staged` | IGNORED | Staged/pre-commit source mode is not implemented. |
| `TestFromFiles` | `gitleaks_ports/detect.rs::gitleaks_detect_test_from_files` | PASS | Filesystem scan finds a committed-style source file secret. |
| `TestDetectWithArchives` | `gitleaks_ports/detect.rs::gitleaks_detect_test_detect_with_archives` | IGNORED | Archive extraction is not implemented. |
| `TestDetectWithSymlinks` | `gitleaks_ports/detect.rs::gitleaks_detect_test_detect_with_symlinks` | PASS | Default skips symlink, `--follow-symlinks` includes it. |
| `TestDetectRuleAllowlist` | `gitleaks_ports/detect.rs::gitleaks_detect_test_detect_basic_rule_and_rule_allowlist` | PASS | Rule-level secret allowlist suppresses the hit. |
| `TestNormalizeGitleaksIgnorePaths` | `gitleaks_ports/detect.rs::gitleaks_detect_test_normalize_gitleaksignore_paths` | IGNORED | Path normalization corners remain v1.1 polish. |
| `TestWindowsFileSeparator_RulePath` | `gitleaks_ports/detect.rs::gitleaks_detect_test_windows_file_separator_rule_path` | IGNORED | Windows separator matching remains v1.1 polish. |
| `TestWindowsFileSeparator_RuleAllowlistPaths` | `gitleaks_ports/detect.rs::gitleaks_detect_test_windows_file_separator_rule_allowlist_paths` | IGNORED | Windows separator allowlist matching remains v1.1 polish. |

## detect/baseline_test.go

| Go test function | Rust port | Status | Notes |
| --- | --- | --- | --- |
| `TestIsNew` | `gitleaks_ports/baseline.rs::gitleaks_detect_baseline_test_is_new` | IGNORED | lmprobe suppresses by fingerprint rather than full finding metadata. |
| `TestFileLoadBaseline` | `gitleaks_ports/baseline.rs::gitleaks_detect_baseline_test_file_load_baseline_json` | PASS | JSON baseline path is accepted. |
| `TestFileLoadBaseline` CSV/SARIF rows | `gitleaks_ports/baseline.rs::gitleaks_detect_baseline_test_file_load_baseline_csv_sarif_errors` | IGNORED | Non-JSON baseline handling is not implemented. |
| `TestIgnoreIssuesInBaseline` | `gitleaks_ports/baseline.rs::gitleaks_detect_baseline_test_ignore_issues_in_baseline` | PASS | Fingerprint baseline suppresses a finding. |

## detect/codec/decoder_test.go

| Go test function | Rust port | Status | Notes |
| --- | --- | --- | --- |
| `TestDecode` | `gitleaks_ports/decoder.rs::gitleaks_codec_decoder_test_decode_base64_percent_hex_unicode` | PASS | Recursive scanner decode path covers base64, percent, hex, and unicode. |
| `TestDecode` invalid base64 row | `gitleaks_ports/decoder.rs::gitleaks_codec_decoder_test_decode_invalid_base64_is_left_alone` | PASS | Invalid base64 does not produce a finding. |

## detect/utils_test.go

| Go test function | Rust port | Status | Notes |
| --- | --- | --- | --- |
| `Test_createScmLink` | `gitleaks_ports/utils.rs::gitleaks_detect_utils_test_create_scm_link` | IGNORED | lmprobe secrets does not generate SCM web links. |
| entropy helper coverage requested by migration task | `gitleaks_ports/utils.rs::gitleaks_detect_utils_entropy_gate_math` | PASS | Shannon entropy matches gitleaks math for a known AWS example. |
| fingerprint helper coverage requested by migration task | `gitleaks_ports/utils.rs::gitleaks_detect_utils_fingerprint_format` | PASS | Filesystem fingerprint format is `path:rule:line`. |

## detect/location_test.go

| Go test function | Rust port | Status | Notes |
| --- | --- | --- | --- |
| `TestGetLocation` | `gitleaks_ports/location.rs::gitleaks_detect_location_test_get_location` | PASS | Byte-offset matches are reported with 1-based line/column locations matching gitleaks cases. |

## detect/reader_test.go

| Go test function | Rust port | Status | Notes |
| --- | --- | --- | --- |
| `TestDetectReader` | `gitleaks_ports/reader.rs::gitleaks_detect_reader_test_detect_reader` | IGNORED | lmprobe has no public reader source API matching gitleaks `DetectReader`. |
| `TestStreamDetectReader` | `gitleaks_ports/reader.rs::gitleaks_detect_reader_test_stream_detect_reader` | IGNORED | Streaming reader chunking/error propagation is a v1.1 source-mode gap. |

## sources/git_test.go

| Go test function | Rust port | Status | Notes |
| --- | --- | --- | --- |
| commented `TestGitLog` | `gitleaks_ports/git_source.rs::gitleaks_sources_git_test_git_log` | IGNORED | go-gitdiff channel internals are not part of lmprobe. |
| commented `TestGitDiff` | `gitleaks_ports/git_source.rs::gitleaks_sources_git_test_git_diff` | IGNORED | Staged/git-diff source mode is a v1.1 gap. |
| git history committed-and-removed behavior | `gitleaks_ports/git_source.rs::gitleaks_sources_git_test_history_finds_committed_and_removed_secret` | PASS | Historical additions are scanned even after later removal. |

## sources/common_test.go

| Go test function | Rust port | Status | Notes |
| --- | --- | --- | --- |
| `Test_readUntilSafeBoundary` | `gitleaks_ports/source_common.rs::gitleaks_sources_common_test_read_until_safe_boundary` | IGNORED | gitleaks chunk-boundary reader has no direct lmprobe source API equivalent. |

## report/*_test.go

| Go test function | Rust port | Status | Notes |
| --- | --- | --- | --- |
| `json_test.go::TestWriteJSON` | `gitleaks_ports/report.rs::gitleaks_report_json_test_write_json_structural_envelope` | PASS | Structural check for lmprobe JSON envelope and redacted finding. |
| `finding_test.go::TestRedact` | `gitleaks_ports/report.rs::gitleaks_report_finding_redaction_tests` | PASS | Secret type serializes and formats as redacted. |
| `finding_test.go::TestMask` | `gitleaks_ports/report.rs::gitleaks_report_finding_redaction_tests` | PASS | Mapped to lmprobe's always-redacted default boundary. |
| `finding_test.go::TestMaskSecret` | `gitleaks_ports/report.rs::gitleaks_report_finding_redaction_tests` | PASS | Mapped to lmprobe's always-redacted default boundary. |
| `sarif_test.go::TestWriteSarif` | `gitleaks_ports/report.rs::gitleaks_report_sarif_test_write_sarif` | IGNORED | SARIF structural parity remains v1.1. |
| `report_test.go::TestWriteStdout` | `gitleaks_ports/report.rs::gitleaks_report_report_test_write_stdout` | IGNORED | Byte-for-byte stdout parity is not a lmprobe goal. |
| `junit_test.go::TestWriteJunit` | `gitleaks_ports/report.rs::gitleaks_report_junit_test_write_junit` | IGNORED | JUnit report format is not implemented. |
| `template_test.go::TestWriteTemplate` | `gitleaks_ports/report.rs::gitleaks_report_template_test_write_template` | IGNORED | Template report output is not implemented. |
| `template_test.go::TestTemplateDangerousFunctions` | `gitleaks_ports/report.rs::gitleaks_report_template_test_template_dangerous_functions` | IGNORED | Template report output is not implemented. |
| `csv_test.go::TestWriteCSV` | `gitleaks_ports/report.rs::gitleaks_report_csv_test_write_csv` | IGNORED | CSV structural parity remains v1.1. |

## cmd/generate/config/*_test.go

| Go test function | Rust port | Status | Notes |
| --- | --- | --- | --- |
| all rule-generator tests | none | SKIPPED-NOT-APPLICABLE | lmprobe consumes generated `config/gitleaks.toml`; it does not implement the Go rule generator. |
