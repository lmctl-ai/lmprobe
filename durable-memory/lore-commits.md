# Lore commit messages

The first line explains why the change was made. Add context when the diff
cannot explain constraints or an implementation choice. Git-native trailers are
optional; use only those that convey useful decisions or evidence.

```text
Prevent misleading help from hiding a command's write mode

The command previews changes by default but also supports audited apply.

Constraint: Preserve existing invocation and mutation safety semantics
Rejected: Rename the command | breaks established agent recipes
Confidence: high
Scope-risk: narrow
Directive: Keep examples aligned with the actual parser and developer memory
Tested: Help examples, parser regressions, and isolated apply fixtures
Not-tested: Production publishing
```

Useful trailers: Constraint, Rejected, Confidence (low/medium/high), Scope-risk
(narrow/moderate/broad), Reversibility (clean/messy/irreversible), Directive,
Tested, Not-tested, Related. Put trailers after a blank line; use `Key: value`.
