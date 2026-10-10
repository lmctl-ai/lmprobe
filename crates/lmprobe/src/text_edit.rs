//! Small, deterministic text-edit engine used by the programmatic edit command.
//!
//! Operations run in the order supplied.  Line numbers are one-based and
//! inclusive. Line edits retain the input's LF/CRLF convention. Inserted text
//! is given a trailing newline for line-oriented operations; prepend and
//! append preserve their text exactly.

use anyhow::{bail, Result};
use regex::{Regex, RegexBuilder};
use serde::Deserialize;

fn false_value() -> bool {
  false
}

/// A single sequential edit operation.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum EditOperation {
  Replace {
    pattern: String,
    replacement: String,
    #[serde(default = "false_value")]
    regex: bool,
    #[serde(default = "false_value")]
    word: bool,
    #[serde(default = "false_value")]
    ignore_case: bool,
    #[serde(default = "false_value")]
    first: bool,
  },
  InsertBefore {
    line: usize,
    text: String,
  },
  InsertAfter {
    line: usize,
    text: String,
  },
  DeleteLines {
    start: usize,
    end: usize,
  },
  ReplaceLines {
    start: usize,
    end: usize,
    text: String,
  },
  Prepend {
    text: String,
  },
  Append {
    text: String,
  },
}

/// Apply `ops` in order and return the resulting text.
pub fn transform(input: &str, ops: &[EditOperation]) -> Result<String> {
  let mut output = input.to_owned();
  for operation in ops {
    output = match operation {
      EditOperation::Replace {
        pattern,
        replacement,
        regex,
        word,
        ignore_case,
        first,
      } => replace(
        &output,
        pattern,
        replacement,
        *regex,
        *word,
        *ignore_case,
        *first,
      )?,
      EditOperation::InsertBefore { line, text } => {
        let lines = split_lines(&output);
        let index = checked_line(*line, lines.len())?;
        let newline = newline_for(&output);
        let inserted = terminated(text, newline);
        let mut result = String::new();
        result.push_str(&lines[..index].concat());
        result.push_str(&inserted);
        result.push_str(&lines[index..].concat());
        result
      }
      EditOperation::InsertAfter { line, text } => {
        let lines = split_lines(&output);
        let index = checked_line(*line, lines.len())?;
        let newline = newline_for(&output);
        let inserted = terminated(text, newline);
        let mut result = String::new();
        result.push_str(&lines[..=index].concat());
        if !lines[index].ends_with('\n') {
          result.push_str(newline);
        }
        result.push_str(&inserted);
        result.push_str(&lines[index + 1..].concat());
        result
      }
      EditOperation::DeleteLines { start, end } => {
        let lines = split_lines(&output);
        let (start, end) = checked_range(*start, *end, lines.len())?;
        lines[..start].concat() + &lines[end + 1..].concat()
      }
      EditOperation::ReplaceLines { start, end, text } => {
        let lines = split_lines(&output);
        let (start, end) = checked_range(*start, *end, lines.len())?;
        let selected = lines[start..=end].concat();
        let newline = newline_for(&output);
        let replacement = if selected.ends_with('\n') {
          terminated(text, newline)
        } else {
          normalize_newlines(text, newline)
        };
        lines[..start].concat() + &replacement + &lines[end + 1..].concat()
      }
      EditOperation::Prepend { text } => text.clone() + &output,
      EditOperation::Append { text } => output + text,
    };
  }
  Ok(output)
}

fn replace(
  input: &str,
  pattern: &str,
  replacement: &str,
  is_regex: bool,
  whole_word: bool,
  ignore_case: bool,
  first: bool,
) -> Result<String> {
  if pattern.is_empty() {
    bail!("replace pattern must not be empty");
  }
  let expression = if is_regex {
    pattern.to_owned()
  } else {
    regex::escape(pattern)
  };
  let regex = RegexBuilder::new(&expression)
    .case_insensitive(ignore_case)
    .build()
    .map_err(|error| anyhow::anyhow!("invalid replacement pattern: {error}"))?;
  let mut selected = Vec::new();
  for captures in regex.captures_iter(input) {
    let matched = captures
      .get(0)
      .expect("captures_iter always contains the full match");
    if !whole_word || is_word_boundary(input, matched.start(), matched.end()) {
      let expanded = if is_regex {
        let mut expanded = String::new();
        captures.expand(replacement, &mut expanded);
        expanded
      } else {
        replacement.to_owned()
      };
      selected.push((matched.start(), matched.end(), expanded));
      if first {
        break;
      }
    }
  }
  if selected.is_empty() {
    return Ok(input.to_owned());
  }
  let mut result = String::with_capacity(input.len());
  let mut cursor = 0;
  for (start, end, expanded) in selected {
    result.push_str(&input[cursor..start]);
    result.push_str(&expanded);
    cursor = end;
  }
  result.push_str(&input[cursor..]);
  Ok(result)
}

fn is_word_boundary(input: &str, start: usize, end: usize) -> bool {
  fn word(character: Option<char>) -> bool {
    static WORD: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    character.is_some_and(|c| {
      WORD
        .get_or_init(|| Regex::new(r"^\w$").unwrap())
        .is_match(c.encode_utf8(&mut [0; 4]))
    })
  }
  !word(input[..start].chars().next_back()) && !word(input[end..].chars().next())
}

fn split_lines(input: &str) -> Vec<String> {
  if input.is_empty() {
    return Vec::new();
  }
  input.split_inclusive('\n').map(str::to_owned).collect()
}

fn newline_for(input: &str) -> &'static str {
  if input.contains("\r\n") {
    "\r\n"
  } else {
    "\n"
  }
}

fn terminated(text: &str, newline: &str) -> String {
  let text = normalize_newlines(text, newline);
  if text.is_empty() || text.ends_with(newline) {
    text
  } else {
    format!("{text}{newline}")
  }
}

fn normalize_newlines(text: &str, newline: &str) -> String {
  let text = text.replace("\r\n", "\n").replace('\r', "\n");
  if newline == "\r\n" {
    text.replace('\n', "\r\n")
  } else {
    text
  }
}

fn checked_line(line: usize, count: usize) -> Result<usize> {
  if line == 0 || line > count {
    bail!("line {line} is outside the input (1..={count})");
  }
  Ok(line - 1)
}

fn checked_range(start: usize, end: usize, count: usize) -> Result<(usize, usize)> {
  if start == 0 || end == 0 || start > end {
    bail!("invalid line range {start}..={end}");
  }
  Ok((checked_line(start, count)?, checked_line(end, count)?))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn final_line_replacement_normalizes_internal_newlines_without_adding_one() {
    for (text, expected) in [
      ("alpha\nbeta", "one\r\nalpha\r\nbeta"),
      ("alpha\r\nbeta\n", "one\r\nalpha\r\nbeta\r\n"),
    ] {
      assert_eq!(
        transform(
          "one\r\ntwo",
          &[EditOperation::ReplaceLines {
            start: 2,
            end: 2,
            text: text.into()
          }]
        )
        .unwrap(),
        expected
      );
    }
  }

  #[test]
  fn literal_dollar_is_not_a_capture() {
    let operation = EditOperation::Replace {
      pattern: "old".into(),
      replacement: "$1".into(),
      regex: false,
      word: false,
      ignore_case: false,
      first: false,
    };
    assert_eq!(transform("old old", &[operation]).unwrap(), "$1 $1");
  }

  #[test]
  fn regex_captures_and_first() {
    let operation = EditOperation::Replace {
      pattern: r"(old)([0-9])".into(),
      replacement: "$2-$1".into(),
      regex: true,
      word: false,
      ignore_case: false,
      first: true,
    };
    assert_eq!(transform("old1 old2", &[operation]).unwrap(), "1-old old2");
  }

  #[test]
  fn whole_word_is_unicode_aware() {
    let operation = EditOperation::Replace {
      pattern: "café".into(),
      replacement: "tea".into(),
      regex: false,
      word: true,
      ignore_case: true,
      first: false,
    };
    assert_eq!(
      transform("Café caféine café", &[operation]).unwrap(),
      "tea caféine tea"
    );
  }

  #[test]
  fn sequential_line_operations_preserve_crlf() {
    let operations = [
      EditOperation::InsertAfter {
        line: 1,
        text: "middle".into(),
      },
      EditOperation::ReplaceLines {
        start: 3,
        end: 3,
        text: "tail".into(),
      },
    ];
    assert_eq!(
      transform("one\r\ntwo\r\nthree", &operations).unwrap(),
      "one\r\nmiddle\r\ntail\r\nthree"
    );
  }

  #[test]
  fn line_ranges_validate_and_insert_does_not_join_lines() {
    assert!(transform(
      "one\ntwo",
      &[EditOperation::DeleteLines { start: 2, end: 1 }]
    )
    .is_err());
    assert_eq!(
      transform(
        "one\ntwo",
        &[EditOperation::InsertBefore {
          line: 2,
          text: "new".into()
        }]
      )
      .unwrap(),
      "one\nnew\ntwo"
    );
  }

  #[test]
  fn prepend_and_append_are_exact_byte_concatenations() {
    let operations = [
      EditOperation::Prepend { text: "<".into() },
      EditOperation::Append { text: ">".into() },
    ];
    assert_eq!(transform("body\r\n", &operations).unwrap(), "<body\r\n>");
  }

  #[test]
  fn regex_captures_are_expanded_from_the_original_context() {
    let operation = EditOperation::Replace {
      pattern: r"(?m)^old$".into(),
      replacement: "new".into(),
      regex: true,
      word: false,
      ignore_case: false,
      first: false,
    };
    assert_eq!(
      transform("old\noldish\nold", &[operation]).unwrap(),
      "new\noldish\nnew"
    );
  }

  #[test]
  fn serde_rejects_unknown_fields_and_defaults_flags() {
    let parsed: EditOperation =
      serde_json::from_str(r#"{"op":"replace","pattern":"a","replacement":"b"}"#).unwrap();
    assert_eq!(
      parsed,
      EditOperation::Replace {
        pattern: "a".into(),
        replacement: "b".into(),
        regex: false,
        word: false,
        ignore_case: false,
        first: false
      }
    );
    assert!(
      serde_json::from_str::<EditOperation>(r#"{"op":"prepend","text":"x","extra":true}"#).is_err()
    );
  }

  #[test]
  fn empty_input_supports_exact_prefix_and_suffix() {
    assert_eq!(
      transform(
        "",
        &[
          EditOperation::Prepend { text: "a".into() },
          EditOperation::Append { text: "b".into() },
        ]
      )
      .unwrap(),
      "ab"
    );
    assert_eq!(transform("", &[]).unwrap(), "");
  }

  #[test]
  fn line_zero_and_out_of_bounds_ranges_are_rejected() {
    for operation in [
      EditOperation::InsertBefore {
        line: 0,
        text: "x".into(),
      },
      EditOperation::InsertAfter {
        line: 3,
        text: "x".into(),
      },
      EditOperation::DeleteLines { start: 1, end: 3 },
      EditOperation::ReplaceLines {
        start: 0,
        end: 1,
        text: "x".into(),
      },
    ] {
      assert!(transform("one\ntwo", &[operation]).is_err());
    }
  }

  #[test]
  fn insert_after_unterminated_last_line_adds_one_separator() {
    assert_eq!(
      transform(
        "last",
        &[EditOperation::InsertAfter {
          line: 1,
          text: "added".into(),
        }]
      )
      .unwrap(),
      "last\nadded\n"
    );
  }

  #[test]
  fn deleting_all_lines_from_a_terminated_file_leaves_empty_text() {
    assert_eq!(
      transform(
        "one\ntwo\n",
        &[EditOperation::DeleteLines { start: 1, end: 2 }]
      )
      .unwrap(),
      ""
    );
  }

  #[test]
  fn sequential_line_edits_use_the_shifted_line_numbers() {
    assert_eq!(
      transform(
        "a\nb\nc",
        &[
          EditOperation::InsertBefore {
            line: 1,
            text: "x".into()
          },
          EditOperation::DeleteLines { start: 3, end: 3 },
        ]
      )
      .unwrap(),
      "x\na\nc"
    );
  }

  #[test]
  fn literal_replacement_escapes_regex_punctuation() {
    assert_eq!(
      transform(
        "a.* a.*",
        &[EditOperation::Replace {
          pattern: "a.*".into(),
          replacement: "done".into(),
          regex: false,
          word: false,
          ignore_case: false,
          first: false,
        }]
      )
      .unwrap(),
      "done done"
    );
  }

  #[test]
  fn whole_word_boundaries_include_adjacent_combining_marks() {
    for (input, pattern, expected) in [
      (
        " café\u{301} café\u{301}x",
        "café\u{301}",
        " tea café\u{301}x",
      ),
      (" café\u{301} café ", "café", " café\u{301} tea "),
      (" \u{301}café café ", "café", " \u{301}café tea "),
    ] {
      assert_eq!(
        transform(
          input,
          &[EditOperation::Replace {
            pattern: pattern.into(),
            replacement: "tea".into(),
            regex: false,
            word: true,
            ignore_case: false,
            first: false
          }]
        )
        .unwrap(),
        expected
      );
    }
  }

  #[test]
  fn named_regex_captures_expand_escaped_dollar_text() {
    assert_eq!(
      transform(
        "user:42",
        &[EditOperation::Replace {
          pattern: r"(?P<name>\w+):(?P<id>\d+)".into(),
          replacement: "${name}=$$${id}".into(),
          regex: true,
          word: false,
          ignore_case: false,
          first: false,
        }]
      )
      .unwrap(),
      "user=$42"
    );
  }

  #[test]
  fn regex_zero_width_anchors_replace_each_matching_line() {
    assert_eq!(
      transform(
        "a\nb",
        &[EditOperation::Replace {
          pattern: r"(?m)^".into(),
          replacement: ">".into(),
          regex: true,
          word: false,
          ignore_case: false,
          first: false,
        }]
      )
      .unwrap(),
      ">a\n>b"
    );
  }

  #[test]
  fn malformed_regex_and_empty_pattern_report_errors() {
    assert!(transform(
      "text",
      &[EditOperation::Replace {
        pattern: "[".into(),
        replacement: "x".into(),
        regex: true,
        word: false,
        ignore_case: false,
        first: false,
      }]
    )
    .is_err());
    assert!(transform(
      "text",
      &[EditOperation::Replace {
        pattern: "".into(),
        replacement: "x".into(),
        regex: false,
        word: false,
        ignore_case: false,
        first: false,
      }]
    )
    .is_err());
  }
}
