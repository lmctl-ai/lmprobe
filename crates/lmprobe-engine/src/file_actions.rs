//! Filesystem snapshots, age filters, and preview-first regular-file deletion.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use lmprobe_types::schema::{Diagnostic, ExitStatus, FileDeleteEntry, FileDeleteResult, FileEntry};

/// Positive integer duration; checked arithmetic also rejects overflow.
pub fn parse_age(value: &str) -> Result<Duration, String> {
  let value = value.trim();
  if value.len() < 2 || !value.is_ascii() {
    return Err("use a positive duration such as 1w, 7d, 24h, 30m, or 60s".into());
  }
  let (number, suffix) = value.split_at(value.len() - 1);
  let multiplier = match suffix {
    "s" => 1,
    "m" => 60,
    "h" => 3600,
    "d" => 86400,
    "w" => 604800,
    _ => return Err("use s, m, h, d, or w (for example 1w)".into()),
  };
  let seconds = number
    .parse::<u64>()
    .ok()
    .and_then(|n| n.checked_mul(multiplier))
    .filter(|n| *n > 0)
    .ok_or_else(|| "duration must be a positive integer without overflow".to_string())?;
  let duration = Duration::from_secs(seconds);
  SystemTime::now()
    .checked_sub(duration)
    .ok_or_else(|| "duration is out of range".to_string())?;
  Ok(duration)
}

pub fn snapshot(path: &Path) -> std::io::Result<FileEntry> {
  // Resolve only the parent: the final symlink must stay a symlink, never its target.
  let original = if path.is_absolute() {
    path.to_path_buf()
  } else {
    std::env::current_dir()?.join(path)
  };
  let mut has_symlink_parent = false;
  for parent in original.ancestors().skip(1) {
    has_symlink_parent |= fs::symlink_metadata(parent)?.file_type().is_symlink();
  }
  let absolute = match (original.parent(), original.file_name()) {
    (Some(parent), Some(name)) => parent.canonicalize()?.join(name),
    _ => original.canonicalize()?,
  };
  let metadata = fs::symlink_metadata(&absolute)?;
  let kind = if metadata.is_file() {
    "file"
  } else if metadata.is_dir() {
    "directory"
  } else if metadata.file_type().is_symlink() {
    "symlink"
  } else {
    "other"
  };
  let absolute_path = absolute
    .to_str()
    .ok_or_else(|| {
      std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "file action paths must be valid UTF-8",
      )
    })?
    .to_string();
  #[cfg(unix)]
  let (device, inode, changed_at) = {
    use std::os::unix::fs::MetadataExt;
    (
      Some(metadata.dev()),
      Some(metadata.ino()),
      DateTime::from_timestamp(metadata.ctime(), metadata.ctime_nsec() as u32),
    )
  };
  #[cfg(not(unix))]
  let (device, inode, changed_at) = (
    None,
    None,
    metadata.created().ok().map(DateTime::<Utc>::from),
  );
  Ok(FileEntry {
    path: path.to_string_lossy().into_owned(),
    absolute_path,
    file_type: kind.into(),
    has_symlink_parent,
    size_bytes: metadata.len(),
    modified_at: metadata.modified().ok().map(DateTime::<Utc>::from),
    changed_at,
    device,
    inode,
  })
}

pub fn older_than(file: &FileEntry, age: Duration, now: SystemTime) -> bool {
  file.modified_at.is_some_and(|modified| {
    let modified: SystemTime = modified.into();
    now
      .duration_since(modified)
      .is_ok_and(|elapsed| elapsed > age)
  })
}

/// All input is decoded before any filesystem mutation can be attempted.
pub fn read_selection(input: &str, null: bool) -> Result<Vec<FileEntry>, String> {
  if null {
    return raw_paths(input.split('\0').filter(|p| !p.is_empty()));
  }
  let trimmed = input.trim();
  if trimmed.is_empty() {
    return Ok(Vec::new());
  }
  if trimmed.starts_with('{') || trimmed.starts_with('[') || trimmed.starts_with('"') {
    let values = serde_json::Deserializer::from_str(input)
      .into_iter::<serde_json::Value>()
      .collect::<Result<Vec<_>, _>>()
      .map_err(|e| format!("invalid JSON file selection: {e}"))?;
    if values.len() == 1
      && values[0].get("schema").and_then(serde_json::Value::as_str) == Some("lmprobe.result.v1")
    {
      let wrapper = &values[0];
      if wrapper
        .get("source_schema")
        .and_then(serde_json::Value::as_str)
        != Some("lmprobe.find.v1")
        || wrapper
          .get("payload_format")
          .and_then(serde_json::Value::as_str)
          != Some("rows")
      {
        return Err("expected program JSON from find with typed file rows".into());
      }
      let status = match wrapper.get("status").and_then(serde_json::Value::as_str) {
        Some("ok") => "matched",
        Some("no_match") => "no_match",
        _ => return Err("refusing failed/partial program selection".into()),
      };
      let files = wrapper
        .pointer("/payload/rows")
        .cloned()
        .ok_or("program selection is missing rows")?;
      let paths = wrapper
        .pointer("/metadata/paths")
        .cloned()
        .ok_or("program selection is missing source paths")?;
      let partial = wrapper
        .pointer("/metadata/partial")
        .cloned()
        .unwrap_or(serde_json::Value::Bool(false));
      let source = serde_json::json!({"schema":"lmprobe.find.v1","exit_status":status,"files":files,"paths":paths,"partial":partial});
      return read_selection(
        &serde_json::to_string(&source).map_err(|error| error.to_string())?,
        false,
      );
    }
    let framed = values.last().ok_or("missing file-selection summary")?;
    let is_stream = framed.get("kind").and_then(|value| value.as_str()) == Some("summary");
    if !is_stream && values.len() != 1 {
      return Err("NDJSON selection must end with a completion summary".into());
    }
    if framed.get("schema").and_then(|value| value.as_str()) != Some("lmprobe.find.v1") {
      return Err("expected a complete lmprobe.find.v1 envelope or NDJSON summary".into());
    }
    match framed.get("partial") {
      None | Some(serde_json::Value::Bool(false)) => {}
      _ => {
        return Err("refusing an incomplete file selection; inspect upstream diagnostics".into())
      }
    }
    match framed.get("exit_status").and_then(|value| value.as_str()) {
      Some("matched" | "no_match") => {}
      _ => return Err("refusing a failed selection or missing completion status".into()),
    }
    let row_values = if is_stream {
      &values[..values.len() - 1]
    } else {
      &[]
    };
    let mut paths = Vec::new();
    let mut rows = Vec::new();
    for value in row_values {
      if let Some(path) = value.as_str() {
        paths.push(path.to_string());
      } else {
        rows.push(
          serde_json::from_value::<FileEntry>(value.clone())
            .map_err(|error| format!("invalid file-selection row: {error}"))?,
        );
      }
    }
    if !paths.is_empty() && !rows.is_empty() {
      return Err("mixed path and object rows in file selection".into());
    }
    let files = if let Some(items) = framed.get("files") {
      let files: Vec<FileEntry> =
        serde_json::from_value(items.clone()).map_err(|error| error.to_string())?;
      let declared_paths: Vec<String> = if is_stream {
        if !rows.is_empty() {
          rows.iter().map(|file| file.path.clone()).collect()
        } else {
          paths
        }
      } else {
        serde_json::from_value(
          framed
            .get("paths")
            .cloned()
            .ok_or("selection is missing paths")?,
        )
        .map_err(|error| error.to_string())?
      };
      if declared_paths
        != files
          .iter()
          .map(|file| file.path.clone())
          .collect::<Vec<_>>()
        || (!rows.is_empty() && rows != files)
      {
        return Err("file rows do not match the completed selection snapshot".into());
      }
      files
    } else {
      if !rows.is_empty() {
        return Err("object selection is missing its snapshots".into());
      }
      if !is_stream {
        paths = serde_json::from_value(
          framed
            .get("paths")
            .cloned()
            .ok_or("selection is missing paths")?,
        )
        .map_err(|error| error.to_string())?;
      }
      raw_paths(paths.iter().map(String::as_str))?
    };
    if framed.get("exit_status").and_then(|value| value.as_str()) == Some("no_match")
      && !files.is_empty()
    {
      return Err("no-match completion contains file targets".into());
    }
    return Ok(files);
  }
  raw_paths(input.lines().filter(|p| !p.is_empty()))
}

fn raw_paths<'a>(paths: impl Iterator<Item = &'a str>) -> Result<Vec<FileEntry>, String> {
  paths
    .map(|p| snapshot(Path::new(p)).map_err(|e| format!("cannot inspect {p:?}: {e}")))
    .collect()
}

fn validate(file: &FileEntry) -> Result<(), String> {
  if file.has_symlink_parent {
    return Err("selection traversed a symlink parent".into());
  }
  if file.file_type != "file" || file.modified_at.is_none() {
    return Err("only regular files with modification metadata can be deleted".into());
  }
  let path = Path::new(&file.absolute_path);
  if !path.is_absolute() {
    return Err("selected file must have an absolute path".into());
  }
  // Never walk through a replaced symlink parent on a stored selection.
  for parent in path.ancestors().skip(1) {
    if fs::symlink_metadata(parent)
      .map_err(|e| e.to_string())?
      .file_type()
      .is_symlink()
    {
      return Err("selected path now has a symlink parent".into());
    }
  }
  let current = snapshot(path).map_err(|e| e.to_string())?;
  if current.file_type != "file"
    || current.size_bytes != file.size_bytes
    || current.modified_at != file.modified_at
    || current.changed_at != file.changed_at
    || current.device != file.device
    || current.inode != file.inode
  {
    return Err("file changed since selection; run the search again".into());
  }
  Ok(())
}

pub fn delete(files: &[FileEntry], force: bool) -> FileDeleteResult {
  let mut seen = HashSet::new();
  let entries: Vec<_> = files
    .iter()
    .map(|file| FileDeleteEntry {
      file: file.clone(),
      status: "preview".into(),
      message: None,
    })
    .collect();
  let mut result = FileDeleteResult {
    schema: "lmprobe.delete.v1".into(),
    mode: if force { "delete" } else { "preview" }.into(),
    exit_status: ExitStatus::from_count(entries.len()),
    files: entries,
    deleted_count: 0,
    audit_log_path: None,
    warnings: Vec::new(),
    partial: false,
  };
  if !force {
    result
      .files
      .retain(|entry| seen.insert(entry.file.absolute_path.clone()));
    if !result.files.is_empty() {
      result.warnings.push(
        Diagnostic::new("DELETE_PREVIEW", "display only; no files deleted")
          .with_hint("add --force to execute the delete action"),
      );
    }
    return result;
  }
  if result.files.is_empty() {
    return result;
  }
  // Validate the whole batch before the first deletion, then recheck each target.
  for entry in &mut result.files {
    if let Err(message) = validate(&entry.file) {
      entry.status = "refused".into();
      entry.message = Some(message);
      result.exit_status = ExitStatus::Error;
    }
  }
  if result.exit_status == ExitStatus::Error {
    result.warnings.push(Diagnostic::new(
      "DELETE_REFUSED",
      "selection failed validation; no files deleted",
    ));
    return result;
  }
  // Validate every original entry before deduplicating: an eligible path
  // must not hide a forbidden symlink alias to the same absolute target.
  result
    .files
    .retain(|entry| seen.insert(entry.file.absolute_path.clone()));
  let audit = (|| -> std::io::Result<_> {
    let directory = crate::history::xdg_cache_dir();
    fs::create_dir_all(&directory)?;
    let path = directory.join(format!(
      "delete-{}-{}.jsonl",
      Utc::now().timestamp_nanos_opt().unwrap_or_default(),
      std::process::id()
    ));
    let mut log = OpenOptions::new()
      .write(true)
      .create_new(true)
      .open(&path)?;
    writeln!(
      log,
      "{}",
      serde_json::json!({"schema":"lmprobe.delete_audit.v1", "files":files})
    )?;
    log.sync_all()?;
    Ok((log, path))
  })();
  let (mut log, audit_path) = match audit {
    Ok(audit) => audit,
    Err(error) => {
      result.exit_status = ExitStatus::Error;
      result
        .warnings
        .push(Diagnostic::new("DELETE_AUDIT_FAILED", error.to_string()));
      return result;
    }
  };
  result.audit_log_path = Some(audit_path.to_string_lossy().into_owned());
  for entry in &mut result.files {
    let outcome = validate(&entry.file).and_then(|()| {
      writeln!(
        log,
        "{}",
        serde_json::json!({"deleting":entry.file.absolute_path})
      )
      .and_then(|()| log.sync_all())
      .map_err(|e| e.to_string())?;
      fs::remove_file(&entry.file.absolute_path).map_err(|e| e.to_string())
    });
    match outcome {
      Ok(()) => {
        entry.status = "deleted".into();
        result.deleted_count += 1;
      }
      Err(message) => {
        entry.status = "failed".into();
        entry.message = Some(message.clone());
        result.partial = true;
        result.warnings.push(Diagnostic::new(
          "DELETE_FAILED",
          format!("{}: {message}", entry.file.path),
        ));
      }
    }
  }
  if result.partial && result.deleted_count == 0 {
    result.exit_status = ExitStatus::Error;
  }
  result
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn age_units_and_outer_whitespace_have_exact_durations() {
    for (input, seconds) in [
      ("1s", 1),
      ("2m", 120),
      ("3h", 10_800),
      ("4d", 345_600),
      (" 1w ", 604_800),
    ] {
      assert_eq!(
        parse_age(input).unwrap(),
        Duration::from_secs(seconds),
        "{input}"
      );
    }
    for input in ["1.5d", "1D", "1ms", "1 d", "1", "18446744073709551616s"] {
      assert!(parse_age(input).is_err(), "{input}");
    }
  }

  #[test]
  fn selection_requires_completed_matching_status_and_shape() {
    for value in [
      serde_json::json!({"schema":"lmprobe.find.v1","paths":[],"files":[]}),
      serde_json::json!({"schema":"lmprobe.find.v1","exit_status":"error","paths":[],"files":[]}),
      serde_json::json!({"schema":"lmprobe.find.v1","exit_status":"matched","partial":true,"paths":[],"files":[]}),
      serde_json::json!({"schema":"lmprobe.find.v1","exit_status":"matched","partial":"false","paths":[],"files":[]}),
      serde_json::json!({"schema":"lmprobe.find.v1","exit_status":"matched","paths":["not-selected"],"files":[]}),
      serde_json::json!({"schema":"lmprobe.grep.v1","exit_status":"matched","paths":[],"files":[]}),
    ] {
      assert!(
        read_selection(&value.to_string(), false).is_err(),
        "{value}"
      );
    }
  }

  #[test]
  fn empty_completed_selection_is_valid_in_both_json_transports() {
    let source = serde_json::json!({"schema":"lmprobe.find.v1","exit_status":"no_match","paths":[],"files":[]});
    assert!(read_selection(&source.to_string(), false)
      .unwrap()
      .is_empty());
    let program = serde_json::json!({"schema":"lmprobe.result.v1","source_schema":"lmprobe.find.v1","status":"no_match","payload_format":"rows","payload":{"rows":[]},"metadata":{"paths":[]}});
    assert!(read_selection(&program.to_string(), false)
      .unwrap()
      .is_empty());
    let footer = serde_json::json!({"kind":"summary","schema":"lmprobe.find.v1","exit_status":"no_match","files":[]});
    assert!(read_selection(&footer.to_string(), false)
      .unwrap()
      .is_empty());
  }

  #[test]
  fn malformed_and_unframed_json_never_becomes_raw_path_selection() {
    for input in [
      "{",
      "[]",
      "\"a.txt\"\n\"b.txt\"",
      "{\"schema\":\"lmprobe.find.v1\"}\ntrailing garbage",
    ] {
      let error = read_selection(input, false).unwrap_err();
      assert!(
        !error.contains("cannot inspect"),
        "reject framing before inspecting paths: {error}"
      );
    }
  }

  #[test]
  fn program_selection_refuses_other_payloads_missing_metadata_and_partial_status() {
    let valid = serde_json::json!({"schema":"lmprobe.result.v1","source_schema":"lmprobe.find.v1","status":"ok","payload_format":"rows","payload":{"rows":[]},"metadata":{"paths":[]}});
    for (key, value) in [
      ("status", serde_json::json!("partial")),
      ("status", serde_json::json!("error")),
      ("source_schema", serde_json::json!("lmprobe.git_status.v1")),
      ("payload_format", serde_json::json!("hierarchical")),
      ("metadata", serde_json::json!({})),
    ] {
      let mut invalid = valid.clone();
      invalid[key] = value;
      assert!(
        read_selection(&invalid.to_string(), false).is_err(),
        "{invalid}"
      );
    }
    let mut invalid = valid;
    invalid["metadata"]["partial"] = serde_json::json!(true);
    assert!(read_selection(&invalid.to_string(), false).is_err());
  }
  #[test]
  fn age_boundaries_and_unicode_inputs_are_safe() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(2_000_000);
    let age = parse_age("1w").unwrap();
    let mut file = FileEntry {
      path: "x".into(),
      absolute_path: "/x".into(),
      file_type: "file".into(),
      has_symlink_parent: false,
      size_bytes: 0,
      modified_at: Some((now - age).into()),
      changed_at: None,
      device: None,
      inode: None,
    };
    assert!(!older_than(&file, age, now));
    file.modified_at = Some((now - age - Duration::from_nanos(1)).into());
    assert!(older_than(&file, age, now));
    file.modified_at = Some((now + Duration::from_secs(1)).into());
    assert!(!older_than(&file, age, now));
    for value in ["", "é", "1é", "0d", "-1w", "18446744073709551615w"] {
      assert!(parse_age(value).is_err());
    }
  }
}
