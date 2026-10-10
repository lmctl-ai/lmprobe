# Per-language fixture owners

Phase 0 §0.11 / CC2 — skeleton. One row per QA-tested language. The
"Owner" column is intentionally blank: a Phase-1+ task assigns
per-language stewards so fixture rot stops cascading into QA gaps.

The `Fixture` column points at the canonical reference repo used by
`tests/deep.rs`. They live out-of-tree under
`$LMPROBE_QA_CORPUS/<lang>/<repo>/` today; CC2 will commit
minimal in-tree replacements as part of Phase 1+.

| Language    | Fixture                                  | Smoke cases | Owner |
| ----------- | ---------------------------------------- | ----------- | ----- |
| bash        | bash/git-extras                          | 0           |       |
| c           | c/sds                                    | 0           |       |
| cpp         | cpp/stb                                  | 0           |       |
| csharp      | csharp/commandline                       | 0           |       |
| css         | css/normalize.css                        | 0           |       |
| dart        | dart/dart_style                          | 0           |       |
| elixir      | elixir/plug                              | 0           |       |
| go          | go/cobra                                 | 7           |       |
| haskell     | haskell/xmonad                           | 0           |       |
| hcl         | hcl/terraform-aws-vpc                    | 0           |       |
| html        | html/50projects50days                    | 0           |       |
| java        | java/junit5-samples                      | 2           |       |
| javascript  | javascript/JSON-js                       | 0           |       |
| json        | json/JSON-Schema-Test-Suite              | 0           |       |
| kotlin      | kotlin/architecture-templates            | 0           |       |
| lua         | lua/kickstart.nvim                       | 0           |       |
| nix         | nix/nixos-generators                     | 0           |       |
| php         | php/PHP-Parser                           | 0           |       |
| python      | python/markupsafe                        | 5           |       |
| ruby        | ruby/sinatra                             | 0           |       |
| rust        | rust/fd                                  | 9           |       |
| scala       | scala/scopt                              | 0           |       |
| solidity    | solidity/solmate                         | 0           |       |
| swift       | swift/Alamofire                          | 0           |       |
| toml        | toml/toml                                | 0           |       |
| tsx         | tsx/swr                                  | 0           |       |
| typescript  | typescript/zod                           | 0           |       |
| xml         | xml/dom4j                                | 1           |       |
| yaml        | yaml/ansible-examples                    | 0           |       |

Smoke counts as of Chunk 0C (36 total cases incl. cross-cutting).
