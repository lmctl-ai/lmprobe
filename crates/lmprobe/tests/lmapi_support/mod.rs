//! Shared helpers for the lmapi integration tests: a runner with a hard timeout (no test can
//! hang), and a small deterministic Git fixture with code in Rust and Python.

#![allow(dead_code)]

use serde_json::Value;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const TIMEOUT: Duration = Duration::from_secs(30);

pub struct Run {
  pub code: Option<i32>,
  pub stdout: Vec<u8>,
  pub stderr: Vec<u8>,
  pub elapsed: Duration,
}

impl Run {
  pub fn code(&self) -> i32 {
    self.code.expect("exit code (killed by a signal?)")
  }
  pub fn json(&self) -> Value {
    serde_json::from_slice(&self.stdout).unwrap_or_else(|e| {
      panic!(
        "stdout is not one JSON document ({e}):\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&self.stdout),
        String::from_utf8_lossy(&self.stderr)
      )
    })
  }
  pub fn text(&self) -> String {
    String::from_utf8_lossy(&self.stdout).into_owned()
  }
  pub fn err_text(&self) -> String {
    String::from_utf8_lossy(&self.stderr).into_owned()
  }
}

/// How to run one invocation.
#[derive(Default)]
pub struct Opts<'a> {
  pub env: Vec<(&'a str, String)>,
  pub remove_env: Vec<&'a str>,
  pub stdin: Option<Vec<u8>>,
  pub timeout: Option<Duration>,
}

pub fn bin() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

/// Run `bin` with `args` in `cwd`; panics (after killing the child) when it outlives the timeout.
pub fn run_bin<S: AsRef<std::ffi::OsStr>>(bin: &Path, args: &[S], cwd: &Path, opts: Opts) -> Run {
  let mut cmd = Command::new(bin);
  cmd
    .args(args)
    .current_dir(cwd)
    .env_remove("LMPROBE_READONLY")
    .env_remove("LMPROBE_DRYRUN")
    .stdin(if opts.stdin.is_some() {
      Stdio::piped()
    } else {
      Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
  // Audit logs, patches and history caches go under target/tmp, never the user's cache.
  cmd.env(
    "XDG_CACHE_HOME",
    Path::new(env!("CARGO_TARGET_TMPDIR")).join("lmapi-xdg-cache"),
  );
  for k in &opts.remove_env {
    cmd.env_remove(k);
  }
  for (k, v) in &opts.env {
    cmd.env(k, v);
  }
  let start = Instant::now();
  let mut child = cmd.spawn().expect("spawn lmprobe");
  if let Some(input) = opts.stdin {
    let mut pipe = child.stdin.take().unwrap();
    std::thread::spawn(move || {
      let _ = pipe.write_all(&input);
    });
  }
  let mut out = child.stdout.take().unwrap();
  let mut err = child.stderr.take().unwrap();
  let out_t = std::thread::spawn(move || {
    let mut v = Vec::new();
    let _ = out.read_to_end(&mut v);
    v
  });
  let err_t = std::thread::spawn(move || {
    let mut v = Vec::new();
    let _ = err.read_to_end(&mut v);
    v
  });
  let limit = opts.timeout.unwrap_or(TIMEOUT);
  let status = loop {
    if let Some(status) = child.try_wait().unwrap() {
      break status;
    }
    if start.elapsed() > limit {
      let _ = child.kill();
      let _ = child.wait();
      panic!(
        "timeout after {limit:?}: {:?}",
        args
          .iter()
          .map(|a| a.as_ref().to_owned())
          .collect::<Vec<_>>()
      );
    }
    std::thread::sleep(Duration::from_millis(5));
  };
  Run {
    code: status.code(),
    stdout: out_t.join().unwrap(),
    stderr: err_t.join().unwrap(),
    elapsed: start.elapsed(),
  }
}

pub fn run<S: AsRef<std::ffi::OsStr>>(args: &[S], cwd: &Path, opts: Opts) -> Run {
  run_bin(&bin(), args, cwd, opts)
}

pub fn lm(args: &[&str], cwd: &Path) -> Run {
  run(args, cwd, Opts::default())
}

/// `git` in `dir` with a fixed identity, dates and no user config.
pub fn git(dir: &Path, args: &[&str]) {
  let out = Command::new("git")
    .args(args)
    .current_dir(dir)
    .env("GIT_CONFIG_GLOBAL", "/dev/null")
    .env("GIT_CONFIG_NOSYSTEM", "1")
    .env("GIT_AUTHOR_NAME", "Fixture")
    .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
    .env("GIT_COMMITTER_NAME", "Fixture")
    .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
    .env("GIT_AUTHOR_DATE", "2026-01-02T03:04:05Z")
    .env("GIT_COMMITTER_DATE", "2026-01-02T03:04:05Z")
    .output()
    .expect("run git");
  assert!(
    out.status.success(),
    "git {args:?}: {}",
    String::from_utf8_lossy(&out.stderr)
  );
}

pub const LIB_RS_V1: &str = "use std::collections::HashMap;

pub trait Shape {
    fn area(&self) -> f64;
}

pub struct Square {
    pub side: f64,
}

impl Shape for Square {
    fn area(&self) -> f64 {
        self.side * self.side
    }
}

pub struct User {
    pub name: String,
}

pub fn helper(x: i32) -> i32 {
    x + 1
}

pub fn run() -> i32 {
    let m: HashMap<i32, i32> = HashMap::new();
    helper(m.len() as i32)
}
";

pub const APP_PY: &str = r#"import os
import hashlib
import requests
from fastapi import FastAPI

app = FastAPI()


def require_auth(f):
    return f


@app.get("/users")
@require_auth
def list_users():
    token = os.environ.get("API_TOKEN")
    if not authorize(token):
        return []
    digest = hashlib.md5(b"x").hexdigest()
    f = open("notes.txt")
    data = f.read()
    f.close()
    resp = requests.get("https://example.invalid/api", timeout=5)
    delete_user(1)
    return [digest, data, resp]


def authorize(t):
    return bool(t)


def delete_user(i):
    return i


async def worker():
    fetch()


def slice_it(items):
    head = items[:2]
    process(items, head)
"#;

pub const OPENAPI_YAML: &str = "openapi: 3.0.0
info:
  title: Fixture
  version: '1'
paths:
  /users:
    get:
      operationId: listUsers
      responses:
        '200':
          description: ok
";

/// A Git repository with two commits (fixed identity and dates): `src/lib.rs`, `app.py`,
/// `config.yaml`, `openapi.yaml`, `notes.txt` ("hello world"); the second commit adds a TODO and
/// changes `helper`.
pub fn fixture() -> tempfile::TempDir {
  let dir = tempfile::tempdir().unwrap();
  let root = dir.path();
  fs::create_dir_all(root.join("src")).unwrap();
  fs::write(root.join("src/lib.rs"), LIB_RS_V1).unwrap();
  fs::write(root.join("app.py"), APP_PY).unwrap();
  fs::write(root.join("config.yaml"), "api:\n  token_key: API_TOKEN\n").unwrap();
  fs::write(root.join("openapi.yaml"), OPENAPI_YAML).unwrap();
  fs::write(root.join("notes.txt"), "hello world\n").unwrap();
  // A one-rule gitleaks config: the default rule set takes ~40 s to compile in debug builds.
  fs::write(
    root.join("rules.toml"),
    "[[rules]]\nid = \"fixture-token\"\ndescription = \"fixture\"\nregex = '''fixture_[a-z]{8}'''\n",
  )
  .unwrap();
  git(root, &["init", "-q", "-b", "main"]);
  git(root, &["add", "."]);
  git(root, &["commit", "-qm", "init"]);
  let v2 = LIB_RS_V1.replace(
    "    x + 1\n",
    "    // TODO: overflow\n    x.saturating_add(1)\n",
  );
  fs::write(root.join("src/lib.rs"), v2).unwrap();
  git(root, &["commit", "-qam", "saturate helper"]);
  dir
}

/// Files directly in `dir` whose name starts with `prefix`.
pub fn temp_files(dir: &Path, prefix: &str) -> Vec<String> {
  fs::read_dir(dir)
    .map(|it| {
      it.filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(prefix))
        .collect()
    })
    .unwrap_or_default()
}
