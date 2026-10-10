//! §1.7 — bearer-token middleware integration test.
//!
//! Gated behind `--features serve`. Default `cargo test -p lmprobe`
//! skips this whole file; run via:
//!   cargo test -p lmprobe --features serve --test auth
#![cfg(feature = "serve")]
//!
//! Three cases against `serve --addr 127.0.0.1:<port> --auth-token
//! <token>`:
//!
//! 1. POST without `Authorization` → 401.
//! 2. POST with wrong token        → 401.
//! 3. POST with correct token      → 200.
//!
//! Loopback bind is still chosen (otherwise the bind-time refusal
//! kicks in); the `--auth-token` flag activates the middleware
//! regardless of bind interface.
//!
//! Run with `cargo test -p lmprobe --test auth`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const TOKEN: &str = "test-token-XYZ-1234567890";

fn binary() -> PathBuf {
  if let Ok(p) = std::env::var("LMPROBE_BIN") {
    return PathBuf::from(p);
  }
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

fn pick_port() -> u16 {
  // Bind ephemeral; close; reuse the number.
  let sock = std::net::TcpListener::bind("127.0.0.1:0").expect("ephemeral bind");
  let port = sock.local_addr().unwrap().port();
  drop(sock);
  port
}

struct ServeGuard(Child);
impl Drop for ServeGuard {
  fn drop(&mut self) {
    let _ = self.0.kill();
    let _ = self.0.wait();
  }
}

fn start_server(port: u16) -> ServeGuard {
  let child = Command::new(binary())
    .args([
      "serve",
      "--addr",
      &format!("127.0.0.1:{}", port),
      "--auth-token",
      TOKEN,
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("spawn lmprobe serve");

  // Wait for the server to bind by polling TcpStream connect.
  for _ in 0..50 {
    if TcpStream::connect(("127.0.0.1", port)).is_ok() {
      return ServeGuard(child);
    }
    std::thread::sleep(Duration::from_millis(50));
  }
  panic!("server did not start within 2.5s");
}

/// Tiny hand-rolled HTTP/1.1 POST so this test doesn't need reqwest.
fn http_post_graphql(port: u16, headers: &[(&str, &str)], body: &str) -> u16 {
  let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
  stream
    .set_read_timeout(Some(Duration::from_secs(5)))
    .unwrap();
  let mut req = String::new();
  req.push_str("POST /graphql HTTP/1.1\r\n");
  req.push_str(&format!("Host: 127.0.0.1:{}\r\n", port));
  req.push_str("Content-Type: application/json\r\n");
  for (k, v) in headers {
    req.push_str(&format!("{}: {}\r\n", k, v));
  }
  req.push_str(&format!("Content-Length: {}\r\n", body.len()));
  req.push_str("Connection: close\r\n\r\n");
  req.push_str(body);
  stream.write_all(req.as_bytes()).expect("write");
  stream.flush().expect("flush");

  let mut reader = BufReader::new(&mut stream);
  let mut status_line = String::new();
  reader.read_line(&mut status_line).expect("status line");
  // Drain body so the connection closes cleanly.
  let mut sink = Vec::new();
  let _ = reader.read_to_end(&mut sink);
  // "HTTP/1.1 NNN ..."
  status_line
    .split_whitespace()
    .nth(1)
    .and_then(|s| s.parse().ok())
    .unwrap_or(0)
}

#[test]
fn bearer_token_middleware() {
  let port = pick_port();
  let _guard = start_server(port);

  let body = r#"{"query":"{__typename}"}"#;

  let no_token = http_post_graphql(port, &[], body);
  assert_eq!(no_token, 401, "no-token request must be 401");

  let wrong_token = http_post_graphql(port, &[("Authorization", "Bearer wrong-token-zzz")], body);
  assert_eq!(wrong_token, 401, "wrong-token request must be 401");

  let right_token = http_post_graphql(
    port,
    &[("Authorization", &format!("Bearer {}", TOKEN))],
    body,
  );
  assert_eq!(right_token, 200, "correct-token request must be 200");

  eprintln!(
    "auth: no_token={}  wrong_token={}  right_token={}",
    no_token, wrong_token, right_token
  );
}
