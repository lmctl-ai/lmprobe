# lmprobe edit

Preview and apply common text edits to explicit existing UTF-8 files, including
comments, strings, Markdown, configuration, and source. Use `fix` for AST patterns.

## Use cases

| Case | Invocation |
| --- | --- |
| Literal whole-word preview | `lmprobe edit notes.txt -p oldword -r newword --word --json` |
| Apply literal replacement outside Git or in a dirty tree | `lmprobe edit notes.txt -p old -r new --apply --force` |
| Regex capture replacement | `lmprobe edit notes.txt -p '(old)([0-9]+)' -r '$2-$1' --regex` |
| First occurrence, ignore case | `lmprobe edit notes.txt -p old -r new --first -i` |
| Insert line | `lmprobe edit notes.txt --insert-after 3 --text 'new line'` |
| Insert before line | `lmprobe edit notes.txt --insert-before 1 --text 'header'` |
| Delete range | `lmprobe edit notes.txt --delete-lines 2:4` |
| Replace range | `lmprobe edit notes.txt --replace-lines 2:4 --text 'replacement line'` |
| Exact prefix/suffix | `lmprobe edit notes.txt --append --text 'suffix'` |
| Sequence operations | `lmprobe edit notes.txt --script edits.json --apply --force --outfile result.json` |

## Options

`FILES...` names explicit files. No implicit directory traversal or glob expansion;
the shell may expand filenames. Choose one operation:
`--pattern` (`-p`) plus `--replacement` (`-r`), `--script`, `--insert-before`,
`--insert-after`, `--delete-lines`, `--replace-lines`, `--prepend`, or `--append`.
`--text` supplies insertion/range replacement/prefix/suffix content, including
literal newlines. Empty text is allowed. Replacement supports `--regex`, `--word`,
`--first`, and inherited `--ignore-case` (`-i`). Literal replacements preserve `$`
exactly; regex replacements expand `$1` / `${name}`; `$$` escapes dollars.
`--apply` writes; `--force` requires apply and bypasses Git safety checks only.
`--help` and [common.md](common.md) cover shared transport options.
`--max-filesize` is enforced. Traversal/context/cache/language flags do not select
or filter edit files. `--workspace` is refused, including preview, to avoid ambiguity.

## Scripts and semantics

`--script FILE` reads a JSON array; `--script -` reads stdin. This is distinct from
`--infile`, which remains exclusively a full GraphQL document. Operations run
sequentially on the transformed content, independently in each file. Unknown keys
and empty scripts are refused. Put case sensitivity inside script operations.

```json
[
  {"op":"replace","pattern":"oldword","replacement":"newword","word":true},
  {"op":"insert_after","line":2,"text":"added line"},
  {"op":"delete_lines","start":4,"end":5},
  {"op":"replace_lines","start":1,"end":1,"text":"new header"},
  {"op":"prepend","text":"prefix\n"},
  {"op":"append","text":"\nsuffix"}
]
```

Replacement optional booleans: `regex`, `word`, `ignore_case`, `first`, default false.
All replacements apply globally unless first is true. Rust regex syntax supports
multiline/dotall inline flags (`(?m)`, `(?s)`), captures and Unicode; not sed BRE,
lookaround or pattern backreferences. Line numbers are 1-based, ranges inclusive;
invalid ranges fail the whole preflight. Empty files have no addressable lines;
prepend/append can populate them. Line insertions normalize inserted line endings
to CRLF when the input contains CRLF, otherwise LF, and terminate inserted lines.
Replacement of a final unterminated line preserves its lack of a newline unless
the supplied replacement contains one. Prepend/append concatenate exactly.

## Write safety and result

All selected files must pass regular-file, no-symlink-ancestor, single-hardlink,
UTF-8, size, and operation checks before writing anything. Duplicate canonical
paths are deduplicated after validating each supplied path. Outfile cannot alias
a source or script. Apply requires clean Git unless force; unlike `fix`, edits
operate on current live bytes and support non-Git files with force. Unchanged
inputs produce no_match (exit 1, overridable with --exit-zero-on-no-match).

Before writing, a private mode-0600 recovery JSON containing every before/after
is synced under XDG_CACHE_HOME/lmprobe (fallback ~/.cache/lmprobe). Its path is
reported as audit_path. Each source is rechecked for identity/metadata/content,
then replaced through a synced same-directory temporary preserving permissions.
This is atomic per file, not across the batch; there is no locking guarantee.
A write failure stops later files, returns error (exit 2), per-file applied flags,
mode incomplete and recovery path. Existing successes are not silently rolled back.
The recovery file contains source text; keep it private and remove when no longer needed.

Envelope `lmprobe.edit.v1`: mode, files[{path,before,after,applied}], audit_path,
exit_status, warnings. --json/--outfile use `lmprobe.result.v1`, rows payload;
unchanged files are omitted. CLI-only for now; no GraphQL mutation added.

## Code and verification

- `crates/lmprobe/src/text_edit.rs`: deterministic transformations and unit tests.
- `crates/lmprobe/src/edit_command.rs`: clap args, preflight, recovery, safe writes.
- `crates/lmprobe/src/cli.rs`: Commands::Edit dispatch and format rendering.
- `crates/lmprobe/src/program_output.rs`: versioned rows adapter.
- `crates/lmprobe/tests/text_edit.rs`: temp-root command regressions.

Run `cargo test -p lmprobe --locked --test text_edit` and binary unit tests.
