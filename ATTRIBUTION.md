# Attribution

lmprobe is a Rust CLI distributed as packaged binaries through npm. Its
implementation is not included in this public documentation repository.

The private lmprobe implementation is a hard fork of
[ast-grep](https://github.com/ast-grep/ast-grep), forked on 2026-05-02 at
upstream commit `b61e95f4`.

The following implementation components retain ast-grep lineage in the private
source tree:

| Component | Upstream origin | Purpose |
|---|---|---|
| `ast-grep-core` lineage | [ast-grep `crates/core`](https://github.com/ast-grep/ast-grep/tree/main/crates/core) | AST pattern engine |
| `ast-grep-config` lineage | [ast-grep `crates/config`](https://github.com/ast-grep/ast-grep/tree/main/crates/config) | Rule and scan configuration types |
| `ast-grep-language` lineage | [ast-grep `crates/language`](https://github.com/ast-grep/ast-grep/tree/main/crates/language) | Tree-sitter language registry |

Original ast-grep authors retain copyright on pre-fork code. lmprobe-original
work is copyright 2026 lmctl-ai / Mike Ma.

This repository contains documentation and static site assets only.
