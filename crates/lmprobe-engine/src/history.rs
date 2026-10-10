//! Git-history persistence + cache directory plumbing (D6).
//!
//! Phase 0 §0.10 relocates the cache from `<root>/.lmprobe/history.db`
//! (the working-tree pollution we're paying off) to
//! `$XDG_CACHE_HOME/lmprobe/history.db`, defaulting to
//! `~/.cache/lmprobe/history.db`.
//!
//! Back-compat: `evolution` falls back to the legacy path when the
//! XDG path has no rows for a symbol AND the legacy DB exists; that
//! fallback emits a `DEPRECATED_CACHE_PATH` diagnostic. New writes
//! always go to the XDG path.
//!
//! Phase 3A — D2 cache key extension. Two cache surfaces co-exist:
//!  • Blob-keyed: `(blob_sha, language, sensor_version, engine_version,
//!    schema_version) → result`. Stable forever per blob; only grows.
//!  • Repo-wide: `(repo_root_path, schema_version, head_sha) → row[]`
//!    for commit metadata (author, message, timestamp, files-changed).
//!    Invalidates only when HEAD advances; incremental walk picks up
//!    new commits since last cached HEAD.
//!
//! ARCHITECTURE.md § Cache Schema documents the table shapes.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use lmprobe_types::error::Result;

/// Bump when any cache table's column shape changes. Existing rows
/// at older schema_version stay readable — queries simply hit fresh
/// rows at the new version.
pub const SCHEMA_VERSION: u32 = 2;
/// Bump when sensor-emitted result shape changes for a given input.
/// Phase 6B #3 raised this to 2: `blob_signatures` rows now also
/// carry `line_start` / `line_end` for symbol hits, so callers
/// (notably `history blame`) can populate `BlameEntry.line_start`
/// instead of the prior 0-sentinel. Old (sensor_version=1) rows are
/// invisible to the new query path; they age out as fresh rows
/// land at version 2.
#[allow(dead_code)]
pub const SENSOR_VERSION: u32 = 2;
/// Bump when the underlying ast-grep engine version changes (cargo
/// dep upgrade for `ast_grep_core`). Reserved for Phase 4 alongside
/// `SENSOR_VERSION` — the `blob_signatures` table already has the
/// column; the read path that consumes it is the §3.4 / §3A
/// follow-up that closes the AST-aware blame perf debt.
#[allow(dead_code)]
pub const ENGINE_VERSION: u32 = 1;

/// Resolve `$XDG_CACHE_HOME/lmprobe/`, falling back to
/// `$HOME/.cache/lmprobe/`. Caller is responsible for creating it.
pub fn xdg_cache_dir() -> PathBuf {
  let base = std::env::var("XDG_CACHE_HOME")
    .ok()
    .filter(|v| !v.is_empty())
    .map(PathBuf::from)
    .or_else(|| {
      std::env::var("HOME")
        .ok()
        .filter(|v| !v.is_empty())
        .map(|h| PathBuf::from(h).join(".cache"))
    })
    .unwrap_or_else(|| PathBuf::from(".cache"));
  base.join("lmprobe")
}

/// XDG path of the global history database.
pub fn xdg_history_db_path() -> PathBuf {
  xdg_cache_dir().join("history.db")
}

/// Legacy per-root cache path (`<root>/.lmprobe/history.db`). Read
/// for backward-compat in §0.10; never written to from this build on.
pub fn legacy_history_db_path(root: &Path) -> PathBuf {
  root.join(".lmprobe").join("history.db")
}

/// Path for a `trace --force` audit log entry. One file per trace
/// invocation; pre-instrument file SHAs land here so a stale tree can
/// be reconstructed if rollback fails.
pub fn trace_audit_log_path(timestamp: &str) -> PathBuf {
  xdg_cache_dir().join(format!("trace-{}.log", timestamp))
}

/// Open or create the history sqlite database. Schema is
/// upgrade-tolerant via `CREATE TABLE IF NOT EXISTS`. Creates parent
/// dirs as needed.
///
/// Tables (Phase 3A):
///  • `symbol_milestones` — evolution sensor's per-symbol history
///    (Phase 1.8; back-compat preserved).
///  • `repo_commits` — repo-wide commit metadata, keyed by
///    (repo_root, schema_version). One row per commit. Used by
///    `history commit / diff / churn / co-change / todos`.
///  • `repo_head` — last-cached HEAD per repo for incremental walk.
///  • `blob_signatures` — per (blob_sha, language) AST signature
///    used by the content-hash blame heuristic to filter formatting-
///    only commits.
pub fn init_history_db(path: &Path) -> Result<Connection> {
  if let Some(parent) = path.parent() {
    std::fs::create_dir_all(parent)?;
  }
  let conn = Connection::open(path)?;
  conn.execute(
    "CREATE TABLE IF NOT EXISTS symbol_milestones (\
            symbol_name TEXT, \
            commit_hash TEXT, \
            author TEXT, \
            timestamp TEXT, \
            change_type TEXT, \
            signature TEXT, \
            PRIMARY KEY (symbol_name, commit_hash))",
    [],
  )?;
  conn.execute(
    "CREATE TABLE IF NOT EXISTS repo_commits (\
            repo_root TEXT NOT NULL, \
            schema_version INTEGER NOT NULL, \
            commit_hash TEXT NOT NULL, \
            author TEXT, \
            email TEXT, \
            timestamp INTEGER, \
            message TEXT, \
            files_changed TEXT, \
            PRIMARY KEY (repo_root, schema_version, commit_hash))",
    [],
  )?;
  conn.execute(
    "CREATE INDEX IF NOT EXISTS idx_repo_commits_ts \
            ON repo_commits (repo_root, schema_version, timestamp)",
    [],
  )?;
  conn.execute(
    "CREATE TABLE IF NOT EXISTS repo_head (\
            repo_root TEXT NOT NULL, \
            schema_version INTEGER NOT NULL, \
            head_sha TEXT NOT NULL, \
            updated_at INTEGER NOT NULL, \
            PRIMARY KEY (repo_root, schema_version))",
    [],
  )?;
  conn.execute(
        "CREATE TABLE IF NOT EXISTS blob_signatures (\
            blob_sha TEXT NOT NULL, \
            language TEXT NOT NULL, \
            sensor_version INTEGER NOT NULL, \
            engine_version INTEGER NOT NULL, \
            schema_version INTEGER NOT NULL, \
            symbol_name TEXT NOT NULL, \
            content_hash TEXT NOT NULL, \
            signature TEXT NOT NULL DEFAULT '', \
            line_start INTEGER, \
            line_end INTEGER, \
            PRIMARY KEY (blob_sha, language, sensor_version, engine_version, schema_version, symbol_name))",
        [],
    )?;
  // Upgrade paths. SQLite emits "duplicate column" on a no-op; ignore.
  let _ = conn.execute(
    "ALTER TABLE blob_signatures ADD COLUMN signature TEXT NOT NULL DEFAULT ''",
    [],
  );
  // Phase 6B #3 — line range columns (nullable INTEGER).
  let _ = conn.execute(
    "ALTER TABLE blob_signatures ADD COLUMN line_start INTEGER",
    [],
  );
  let _ = conn.execute(
    "ALTER TABLE blob_signatures ADD COLUMN line_end INTEGER",
    [],
  );
  Ok(conn)
}

/// Stable, normalised path string for cache keys. Canonicalises the
/// root to absorb relative-path noise; falls back to the input on
/// canonicalize failure.
pub fn repo_root_key(root: &Path) -> String {
  std::fs::canonicalize(root)
    .map(|p| p.to_string_lossy().into_owned())
    .unwrap_or_else(|_| root.to_string_lossy().into_owned())
}

/// One cached commit. `files_changed` is JSON-serialised on disk.
#[derive(Debug, Clone)]
pub struct CommitRow {
  pub commit_hash: String,
  pub author: String,
  pub email: String,
  pub timestamp: i64,
  pub message: String,
  pub files_changed: Vec<String>,
}

/// One cached `blob_signatures` row — the AST signature + content
/// hash for a (blob_sha, language, symbol_name) tuple. Phase 4A
/// populates this so `blame_symbol`'s per-commit reparse becomes a
/// SQLite read on warm-cache.
///
/// Phase 6B #3 added `line_start` / `line_end`: the symbol's 1-based
/// line range in the blob. `None` for both means "unrecoverable for
/// this entry" (e.g. cross-rename-hop where we know the commit but
/// not the historical line range). The commit/signature/hash are
/// still real even when lines are null.
#[derive(Debug, Clone)]
pub struct BlobSignature {
  pub content_hash: String,
  pub signature: String,
  pub line_start: Option<u32>,
  pub line_end: Option<u32>,
}

/// Read a cached blob signature. None = cache miss; caller parses
/// the blob and writes the row via `insert_blob_signature`.
pub fn read_blob_signature(
  conn: &Connection,
  blob_sha: &str,
  language: &str,
  symbol_name: &str,
) -> Result<Option<BlobSignature>> {
  let mut stmt = conn.prepare(
    "SELECT content_hash, signature, line_start, line_end FROM blob_signatures \
         WHERE blob_sha = ?1 AND language = ?2 \
            AND sensor_version = ?3 AND engine_version = ?4 \
            AND schema_version = ?5 AND symbol_name = ?6",
  )?;
  let mut rows = stmt.query(rusqlite::params![
    blob_sha,
    language,
    SENSOR_VERSION,
    ENGINE_VERSION,
    SCHEMA_VERSION,
    symbol_name
  ])?;
  if let Some(r) = rows.next()? {
    // line_start / line_end stored as nullable INTEGER; map 0 →
    // None (older sensor_version=1 rows would have stored 0; new
    // rows store actual line numbers or NULL).
    let ls: Option<u32> = r.get::<_, Option<u32>>(2)?.filter(|v| *v > 0);
    let le: Option<u32> = r.get::<_, Option<u32>>(3)?.filter(|v| *v > 0);
    Ok(Some(BlobSignature {
      content_hash: r.get(0)?,
      signature: r.get(1)?,
      line_start: ls,
      line_end: le,
    }))
  } else {
    Ok(None)
  }
}

/// Persist a blob signature. Called on cache miss after parsing.
/// Empty signature is a sentinel for "symbol absent in this blob"
/// — cached so subsequent walks skip the parse.
pub fn insert_blob_signature(
  conn: &Connection,
  blob_sha: &str,
  language: &str,
  symbol_name: &str,
  sig: &BlobSignature,
) -> Result<()> {
  conn.execute(
        "INSERT OR REPLACE INTO blob_signatures \
         (blob_sha, language, sensor_version, engine_version, schema_version, symbol_name, content_hash, signature, line_start, line_end) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            blob_sha,
            language,
            SENSOR_VERSION,
            ENGINE_VERSION,
            SCHEMA_VERSION,
            symbol_name,
            sig.content_hash,
            sig.signature,
            sig.line_start,
            sig.line_end,
        ],
    )?;
  Ok(())
}

/// Read the last-cached HEAD for this repo at the current schema
/// version. Returns `None` on cache miss.
pub fn last_cached_head(conn: &Connection, repo_root: &str) -> Result<Option<String>> {
  let mut stmt = conn.prepare(
    "SELECT head_sha FROM repo_head \
         WHERE repo_root = ?1 AND schema_version = ?2",
  )?;
  let mut rows = stmt.query(rusqlite::params![repo_root, SCHEMA_VERSION])?;
  if let Some(r) = rows.next()? {
    Ok(Some(r.get(0)?))
  } else {
    Ok(None)
  }
}

/// Persist the latest HEAD sha so the next refresh can walk only
/// commits introduced since.
pub fn set_cached_head(conn: &Connection, repo_root: &str, head_sha: &str) -> Result<()> {
  conn.execute(
    "INSERT OR REPLACE INTO repo_head \
         (repo_root, schema_version, head_sha, updated_at) \
         VALUES (?1, ?2, ?3, ?4)",
    rusqlite::params![
      repo_root,
      SCHEMA_VERSION,
      head_sha,
      chrono::Utc::now().timestamp()
    ],
  )?;
  Ok(())
}

/// Insert one commit row. `files_changed` is encoded as JSON.
pub fn insert_commit_row(conn: &Connection, repo_root: &str, row: &CommitRow) -> Result<()> {
  let files_json = serde_json::to_string(&row.files_changed).unwrap_or_else(|_| "[]".to_string());
  conn.execute(
        "INSERT OR REPLACE INTO repo_commits \
         (repo_root, schema_version, commit_hash, author, email, timestamp, message, files_changed) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            repo_root,
            SCHEMA_VERSION,
            row.commit_hash,
            row.author,
            row.email,
            row.timestamp,
            row.message,
            files_json
        ],
    )?;
  Ok(())
}

/// Query cached commits, newest-first. Optional filters: minimum
/// timestamp (UNIX seconds) and case-insensitive author substring.
pub fn query_commits(
  conn: &Connection,
  repo_root: &str,
  since_ts: Option<i64>,
  author_substring: Option<&str>,
) -> Result<Vec<CommitRow>> {
  let mut sql = String::from(
    "SELECT commit_hash, author, email, timestamp, message, files_changed \
         FROM repo_commits \
         WHERE repo_root = ?1 AND schema_version = ?2",
  );
  if since_ts.is_some() {
    sql.push_str(" AND timestamp >= ?3");
  }
  if author_substring.is_some() {
    sql.push_str(if since_ts.is_some() {
      " AND LOWER(author) LIKE ?4"
    } else {
      " AND LOWER(author) LIKE ?3"
    });
  }
  sql.push_str(" ORDER BY timestamp DESC, commit_hash ASC");

  let mut stmt = conn.prepare(&sql)?;

  let lowered_author = author_substring.map(|s| format!("%{}%", s.to_lowercase()));
  let make_row = |r: &rusqlite::Row<'_>| -> rusqlite::Result<CommitRow> {
    let files_json: String = r.get(5)?;
    let files: Vec<String> = serde_json::from_str(&files_json).unwrap_or_default();
    Ok(CommitRow {
      commit_hash: r.get(0)?,
      author: r.get(1)?,
      email: r.get(2)?,
      timestamp: r.get(3)?,
      message: r.get(4)?,
      files_changed: files,
    })
  };

  let rows: Vec<CommitRow> = match (since_ts, lowered_author.as_deref()) {
    (None, None) => stmt
      .query_map(rusqlite::params![repo_root, SCHEMA_VERSION], make_row)?
      .collect::<std::result::Result<Vec<_>, _>>()?,
    (Some(ts), None) => stmt
      .query_map(rusqlite::params![repo_root, SCHEMA_VERSION, ts], make_row)?
      .collect::<std::result::Result<Vec<_>, _>>()?,
    (None, Some(a)) => stmt
      .query_map(rusqlite::params![repo_root, SCHEMA_VERSION, a], make_row)?
      .collect::<std::result::Result<Vec<_>, _>>()?,
    (Some(ts), Some(a)) => stmt
      .query_map(
        rusqlite::params![repo_root, SCHEMA_VERSION, ts, a],
        make_row,
      )?
      .collect::<std::result::Result<Vec<_>, _>>()?,
  };
  Ok(rows)
}
