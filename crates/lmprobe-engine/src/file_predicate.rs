//! Generic, bounded filesystem predicates shared by selection and pruning.

use lmprobe_types::schema::{FileKind, FilePredicateInput, GitRemotePredicateInput};
use regex::Regex;
use std::fs::{self, FileType};
use std::path::{Component, Path};

#[derive(Debug)]
pub struct FilePredicate {
  kind: Option<FileKind>,
  name: Option<Regex>,
  path: Option<Regex>,
  entry: Option<String>,
  git_remote: Option<GitRemotePredicateInput>,
  all: Vec<Self>,
  any: Option<Vec<Self>>,
  not: Option<Box<Self>>,
}

impl FilePredicate {
  pub fn compile(input: &FilePredicateInput) -> Result<Self, String> {
    let mut nodes = 0;
    Self::compile_bounded(input, 0, &mut nodes)
  }

  fn compile_bounded(
    input: &FilePredicateInput,
    depth: usize,
    nodes: &mut usize,
  ) -> Result<Self, String> {
    *nodes += 1;
    if depth > 32 || *nodes > 256 {
      return Err("filesystem predicates allow at most 32 nested levels and 256 clauses".into());
    }
    if let Some(entry) = &input.has_entry {
      let mut components = Path::new(entry).components();
      if entry.is_empty()
        || entry.contains(['/', '\\', '\0'])
        || !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
      {
        return Err(
          "has_entry must be a single direct child name, such as .git or package.json".into(),
        );
      }
    }
    let regex = |value: &Option<String>| {
      value
        .as_deref()
        .map(Regex::new)
        .transpose()
        .map_err(|e| e.to_string())
    };
    let mut group = |values: &Option<Vec<FilePredicateInput>>| -> Result<Vec<Self>, String> {
      values
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|value| Self::compile_bounded(value, depth + 1, nodes))
        .collect()
    };
    let all = group(&input.all)?;
    let any = input.any.as_ref().map(|_| group(&input.any)).transpose()?;
    let not = input
      .not
      .as_deref()
      .map(|value| Self::compile_bounded(value, depth + 1, nodes).map(Box::new))
      .transpose()?;
    Ok(Self {
      kind: input.kind,
      name: regex(&input.name_regex)?,
      path: regex(&input.path_regex)?,
      entry: input.has_entry.clone(),
      git_remote: input.git_remote.clone(),
      all,
      any,
      not,
    })
  }

  pub fn matches(&self, path: &Path, kind: FileType) -> std::io::Result<bool> {
    if self.kind.is_some_and(|expected| !match expected {
      FileKind::File => kind.is_file(),
      FileKind::Directory => kind.is_dir(),
      FileKind::Symlink => kind.is_symlink(),
      FileKind::Other => !kind.is_file() && !kind.is_dir() && !kind.is_symlink(),
    }) {
      return Ok(false);
    }
    if let Some(regex) = &self.name {
      let name = match path.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => {
          let resolved = path.canonicalize()?;
          resolved
            .file_name()
            .unwrap_or(resolved.as_os_str())
            .to_string_lossy()
            .into_owned()
        }
      };
      if !regex.is_match(&name) {
        return Ok(false);
      }
    }
    if self
      .path
      .as_ref()
      .is_some_and(|regex| !regex.is_match(path.to_str().unwrap_or("")))
    {
      return Ok(false);
    }
    if let Some(entry) = &self.entry {
      if !kind.is_dir() {
        return Ok(false);
      }
      match fs::symlink_metadata(path.join(entry)) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
      }
    }
    if let Some(predicate) = &self.git_remote {
      if !kind.is_dir() || !git_remote_matches(path, predicate)? {
        return Ok(false);
      }
    }
    for predicate in &self.all {
      if !predicate.matches(path, kind)? {
        return Ok(false);
      }
    }
    if let Some(any) = &self.any {
      let mut matched = false;
      for predicate in any {
        if predicate.matches(path, kind)? {
          matched = true;
          break;
        }
      }
      if !matched {
        return Ok(false);
      }
    }
    if let Some(not) = &self.not {
      if not.matches(path, kind)? {
        return Ok(false);
      }
    }
    Ok(true)
  }
}

fn git_remote_matches(path: &Path, predicate: &GitRemotePredicateInput) -> std::io::Result<bool> {
  match fs::symlink_metadata(path.join(".git")) {
    Ok(_) => {}
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
    Err(error) => return Err(error),
  }
  let io_error = std::io::Error::other;
  let repository = git2::Repository::open(path).map_err(io_error)?;
  let remotes = repository.remotes().map_err(io_error)?;
  for name in remotes.iter().flatten() {
    if predicate
      .remote
      .as_ref()
      .is_some_and(|wanted| wanted != name)
    {
      continue;
    }
    let remote = repository.find_remote(name).map_err(io_error)?;
    let Some(url) = remote.url() else {
      continue;
    };
    let Some((host, owner, repo)) = remote_identity(url) else {
      continue;
    };
    if predicate
      .host
      .as_ref()
      .is_some_and(|wanted| !wanted.eq_ignore_ascii_case(&host))
      || predicate
        .owner
        .as_ref()
        .is_some_and(|wanted| !wanted.eq_ignore_ascii_case(&owner))
      || predicate
        .repo
        .as_ref()
        .is_some_and(|wanted| !wanted.eq_ignore_ascii_case(&repo))
    {
      continue;
    }
    return Ok(true);
  }
  Ok(false)
}

/// Configured HTTPS/SSH/git URLs, SCP-style URLs, and local remote paths.
/// Does not contact the remote or resolve SSH host aliases.
fn remote_identity(url: &str) -> Option<(String, String, String)> {
  let bytes = url.as_bytes();
  let drive = bytes.first().is_some_and(u8::is_ascii_alphabetic)
    && bytes.get(1) == Some(&b':')
    && matches!(bytes.get(2), Some(b'/' | b'\\'));
  let first_slash = url.find(['/', '\\']);
  let scp_colon = if let Some(close) = url.find(']') {
    (bytes.get(close + 1) == Some(&b':')).then_some(close + 1)
  } else {
    url.find(':')
  };
  let scp =
    !drive && scp_colon.is_some_and(|colon| first_slash.map_or(true, |slash| colon < slash));
  let scheme_url = url.split_once("://").filter(|(scheme, _)| {
    scheme
      .as_bytes()
      .first()
      .is_some_and(u8::is_ascii_alphabetic)
      && scheme
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
  });
  let (authority, path, uri) = if let Some((_, rest)) = scheme_url.filter(|_| !drive) {
    let (authority, path) = rest.split_once('/')?;
    (authority, path, true)
  } else if scp {
    let colon = scp_colon?;
    (&url[..colon], &url[colon + 1..], false)
  } else {
    ("", url, false)
  };
  let authority = authority.rsplit('@').next()?;
  let host = if let Some(ipv6) = authority.strip_prefix('[') {
    ipv6.split_once(']')?.0
  } else {
    authority.split(':').next()?
  }
  .to_string();
  // Query/fragment syntax belongs to URI forms, not literal SCP/local names.
  let path = if uri {
    path.split(['?', '#']).next()?
  } else {
    path
  };
  let normalized = if drive {
    path.replace('\\', "/")
  } else {
    path.to_string()
  };
  let path = normalized.trim_matches('/');
  let (owner, repo) = path.rsplit_once('/').unwrap_or(("", path));
  let repo = repo.strip_suffix(".git").unwrap_or(repo);
  if repo.is_empty() {
    return None;
  }
  Some((host, owner.to_string(), repo.to_string()))
}

#[cfg(test)]
mod tests {
  use super::*;

  struct TempTree(std::path::PathBuf);

  impl TempTree {
    fn new(label: &str) -> Self {
      let nonce = format!(
        "{}-{}-{}",
        label,
        std::process::id(),
        std::time::SystemTime::now()
          .duration_since(std::time::UNIX_EPOCH)
          .unwrap()
          .as_nanos()
      );
      let path = std::env::temp_dir().join(format!("lmprobe-file-predicate-{nonce}"));
      std::fs::create_dir_all(&path).unwrap();
      Self(path)
    }

    fn path(&self) -> &Path {
      &self.0
    }
  }

  impl Drop for TempTree {
    fn drop(&mut self) {
      let _ = std::fs::remove_dir_all(&self.0);
    }
  }

  fn kind(path: &Path) -> FileType {
    std::fs::symlink_metadata(path).unwrap().file_type()
  }

  #[test]
  fn remote_forms_preserve_literal_filesystem_names() {
    for (url, host, owner, repo) in [
      (
        "https://user:token@github.com:443/acme/widget.git?x=1#readme",
        "github.com",
        "acme",
        "widget",
      ),
      (
        "ssh://git@github.com:2222/acme/widget.git",
        "github.com",
        "acme",
        "widget",
      ),
      (
        "git://github.com/acme/widget.git",
        "github.com",
        "acme",
        "widget",
      ),
      (
        "file:///srv/mirrors/widget.git",
        "",
        "srv/mirrors",
        "widget",
      ),
      (
        "git@github.com:acme/widget.git",
        "github.com",
        "acme",
        "widget",
      ),
      ("git@[::1]:group/widget.git", "::1", "group", "widget"),
      (
        "git@host:group/widget#archive.git",
        "host",
        "group",
        "widget#archive",
      ),
      (
        "git@host:group/widget?archive.git",
        "host",
        "group",
        "widget?archive",
      ),
      (
        "/srv/mirrors/widget#archive.git",
        "",
        "srv/mirrors",
        "widget#archive",
      ),
      (
        "/srv/mirrors/widget?archive.git",
        "",
        "srv/mirrors",
        "widget?archive",
      ),
      (
        "./mirrors/team:archive/widget.git",
        "",
        "./mirrors/team:archive",
        "widget",
      ),
      (
        "./mirrors/uri://widget.git",
        "",
        "./mirrors/uri:/",
        "widget",
      ),
      ("../mirrors/widget.git", "", "../mirrors", "widget"),
      (r"C:\mirrors\widget.git", "", "C:/mirrors", "widget"),
      ("C:/mirrors/widget.git", "", "C:/mirrors", "widget"),
    ] {
      assert_eq!(
        remote_identity(url),
        Some((host.into(), owner.into(), repo.into())),
        "{url}"
      );
    }
  }
  #[test]
  fn predicate_depth_and_node_budget_are_bounded() {
    let mut value = FilePredicateInput::default();
    for _ in 0..34 {
      value = FilePredicateInput {
        not: Some(Box::new(value)),
        ..Default::default()
      };
    }
    assert!(FilePredicate::compile(&value).is_err());
    let wide = FilePredicateInput {
      all: Some(vec![FilePredicateInput::default(); 256]),
      ..Default::default()
    };
    assert!(FilePredicate::compile(&wide).is_err());
  }

  #[test]
  fn boolean_groups_have_expected_empty_and_nested_semantics() {
    let root = TempTree::new("boolean");
    let file = root.path().join("match.txt");
    std::fs::write(&file, "x").unwrap();
    let empty_all = FilePredicateInput {
      all: Some(Vec::new()),
      ..Default::default()
    };
    assert!(FilePredicate::compile(&empty_all)
      .unwrap()
      .matches(&file, kind(&file))
      .unwrap());
    let empty_any = FilePredicateInput {
      any: Some(Vec::new()),
      ..Default::default()
    };
    assert!(!FilePredicate::compile(&empty_any)
      .unwrap()
      .matches(&file, kind(&file))
      .unwrap());
    let nested = FilePredicateInput {
      all: Some(vec![FilePredicateInput {
        any: Some(vec![FilePredicateInput {
          name_regex: Some("^match\\.txt$".into()),
          ..Default::default()
        }]),
        ..Default::default()
      }]),
      not: Some(Box::new(FilePredicateInput {
        name_regex: Some("^other$".into()),
        ..Default::default()
      })),
      ..Default::default()
    };
    assert!(FilePredicate::compile(&nested)
      .unwrap()
      .matches(&file, kind(&file))
      .unwrap());
  }

  #[test]
  fn multiple_conjunct_fields_require_kind_name_and_path() {
    let root = TempTree::new("conjunct");
    let dir = root.path().join("src");
    let file = dir.join("target.rs");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(&file, "fn main() {}").unwrap();
    let input = FilePredicateInput {
      kind: Some(FileKind::File),
      name_regex: Some("^target\\.rs$".into()),
      path_regex: Some(r"src[/\\][^/\\]+\.rs$".into()),
      ..Default::default()
    };
    let predicate = FilePredicate::compile(&input).unwrap();
    assert!(predicate.matches(&file, kind(&file)).unwrap());
    assert!(!predicate.matches(&dir, kind(&dir)).unwrap());
    let wrong_name = dir.join("other.rs");
    std::fs::write(&wrong_name, "x").unwrap();
    assert!(!predicate.matches(&wrong_name, kind(&wrong_name)).unwrap());
    let wrong_parent = root.path().join("target.rs");
    std::fs::write(&wrong_parent, "x").unwrap();
    assert!(!predicate
      .matches(&wrong_parent, kind(&wrong_parent))
      .unwrap());
    // Keep both lexical fields matching while changing only the entry kind.
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir(&file).unwrap();
    assert!(!predicate.matches(&file, kind(&file)).unwrap());
  }

  #[test]
  fn has_entry_requires_one_normal_component_and_missing_entries_do_not_match() {
    let root = TempTree::new("entry");
    let dir = root.path().join("checkout");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join(".git"), "marker").unwrap();
    let predicate = FilePredicate::compile(&FilePredicateInput {
      has_entry: Some(".git".into()),
      ..Default::default()
    })
    .unwrap();
    assert!(predicate.matches(&dir, kind(&dir)).unwrap());
    let missing = FilePredicate::compile(&FilePredicateInput {
      has_entry: Some("package.json".into()),
      ..Default::default()
    })
    .unwrap();
    assert!(!missing.matches(&dir, kind(&dir)).unwrap());
    assert!(!missing
      .matches(&dir.join(".git"), kind(&dir.join(".git")))
      .unwrap());
    for entry in ["", ".", "..", "a/b", r"a\b", "bad\0name"] {
      assert!(
        FilePredicate::compile(&FilePredicateInput {
          has_entry: Some(entry.into()),
          ..Default::default()
        })
        .is_err(),
        "{entry:?}"
      );
    }
  }

  #[test]
  fn invalid_name_and_path_regexes_are_rejected() {
    for input in [
      FilePredicateInput {
        name_regex: Some("[".into()),
        ..Default::default()
      },
      FilePredicateInput {
        path_regex: Some("(".into()),
        ..Default::default()
      },
    ] {
      assert!(FilePredicate::compile(&input).is_err());
    }
  }

  #[test]
  fn exact_depth_and_node_limits_are_accepted_before_the_next_unit() {
    let mut depth = FilePredicateInput::default();
    for _ in 0..32 {
      depth = FilePredicateInput {
        not: Some(Box::new(depth)),
        ..Default::default()
      };
    }
    assert!(FilePredicate::compile(&depth).is_ok());
    let too_deep = FilePredicateInput {
      not: Some(Box::new(depth)),
      ..Default::default()
    };
    assert!(FilePredicate::compile(&too_deep).is_err());
    let at_limit = FilePredicateInput {
      all: Some(vec![FilePredicateInput::default(); 255]),
      ..Default::default()
    };
    assert!(FilePredicate::compile(&at_limit).is_ok());
    let over_limit = FilePredicateInput {
      all: Some(vec![FilePredicateInput::default(); 256]),
      ..Default::default()
    };
    assert!(FilePredicate::compile(&over_limit).is_err());
  }

  #[test]
  fn git_remote_identity_is_case_insensitive_but_remote_name_is_exact() {
    let root = TempTree::new("remote");
    let repo_path = root.path().join("checkout");
    std::fs::create_dir(&repo_path).unwrap();
    let repository = git2::Repository::init(&repo_path).unwrap();
    repository
      .remote("origin", "https://github.com/acme/widget.git")
      .unwrap();
    let matching = FilePredicate::compile(&FilePredicateInput {
      git_remote: Some(GitRemotePredicateInput {
        host: Some("GITHUB.COM".into()),
        owner: Some("ACME".into()),
        repo: Some("WIDGET".into()),
        ..Default::default()
      }),
      ..Default::default()
    })
    .unwrap();
    assert!(matching.matches(&repo_path, kind(&repo_path)).unwrap());
    for remote in ["Origin", "upstream"] {
      let predicate = FilePredicate::compile(&FilePredicateInput {
        git_remote: Some(GitRemotePredicateInput {
          remote: Some(remote.into()),
          ..Default::default()
        }),
        ..Default::default()
      })
      .unwrap();
      assert!(!predicate.matches(&repo_path, kind(&repo_path)).unwrap());
    }
  }

  #[test]
  fn git_remote_predicate_accepts_worktree_git_file() {
    let root = TempTree::new("worktree");
    let repo_path = root.path().join("source");
    std::fs::create_dir(&repo_path).unwrap();
    let repository = git2::Repository::init(&repo_path).unwrap();
    repository
      .remote("origin", "git@github.com:acme/widget.git")
      .unwrap();
    std::fs::write(repo_path.join("tracked.txt"), "tracked").unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(Path::new("tracked.txt")).unwrap();
    let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
    let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
    repository
      .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
      .unwrap();
    let worktree_path = root.path().join("worktree");
    repository
      .worktree("fixture", &worktree_path, None)
      .unwrap();
    assert!(worktree_path.join(".git").is_file());
    let predicate = FilePredicate::compile(&FilePredicateInput {
      git_remote: Some(GitRemotePredicateInput {
        repo: Some("widget".into()),
        ..Default::default()
      }),
      ..Default::default()
    })
    .unwrap();
    assert!(predicate
      .matches(&worktree_path, kind(&worktree_path))
      .unwrap());
  }
}
