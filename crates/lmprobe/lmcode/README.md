# lmprobe lmcode host

`host.schema.json` is the host signature set used to compile lmcode programs.
It declares one `probe_<operation>(string) -> string` callback per supported
operation. The string argument is a JSON object in the operation shape listed
in `operations.json`; the string result is a JSON serialization of the
`lmprobe.result.v1` operation result. In lmcode, parse that result with
`json_parse` before returning it through the invocation sink.

Example source (the schema path points to `host.schema.json`):

```lms
program = _lmcode_ schema="/path/to/host.schema.json";
$return = json_parse(probe_git_status("{}"));
end _lmcode_
r = run_lmcode(program, {command:[ARGV[0], "--lmcode", "$DSLFILE"]});
println(json_stringify(r));
```

The host treats dry mode as invocation metadata. Its dry callback skips the
operation and supplies the JSON null result; real mode uses the existing
lmprobe CLI implementation and returns its structured result document.
