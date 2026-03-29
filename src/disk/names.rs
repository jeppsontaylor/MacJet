//! Filename normalization for duplicate heuristics (copy suffixes).

use once_cell::sync::Lazy;
use regex::Regex;
use std::ffi::OsStr;

use crate::disk::model::NormalizedName;

static COPY_SUFFIX_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\s*\((\d{1,4})\)\s*$").expect("regex"));

static COPY_WORD_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(^copy of\s+|\s*-\s*copy(?: \d+)?\s*$|\s+copy(?: \d+)?\s*$)").expect("regex")
});

fn collapse_separators(mut s: String) -> String {
    s = s
        .chars()
        .map(|c| match c {
            '_' | '-' | '.' => ' ',
            _ => c,
        })
        .collect::<String>();

    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        let is_space = ch.is_whitespace();
        if is_space {
            if !prev_space {
                out.push(' ');
            }
        } else {
            out.push(ch);
        }
        prev_space = is_space;
    }
    out.trim().to_string()
}

pub fn normalize_filename(name: &OsStr) -> NormalizedName {
    let display_name = name.to_string_lossy().into_owned();
    let (stem_raw, ext) = match display_name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem.to_string(), ext.to_ascii_lowercase()),
        _ => (display_name.clone(), String::new()),
    };

    let stem_lc = stem_raw.to_ascii_lowercase();
    let copy_index = COPY_SUFFIX_RE
        .captures(&stem_lc)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<u32>().ok());

    let stem_no_copy_num = COPY_SUFFIX_RE.replace(&stem_lc, "").into_owned();
    let stem_no_copy_words = COPY_WORD_RE.replace(&stem_no_copy_num, "").into_owned();
    let normalized_stem = collapse_separators(stem_no_copy_words);

    NormalizedName {
        display_name,
        ext,
        normalized_stem,
        copy_index,
        has_copy_suffix: copy_index.is_some() || COPY_WORD_RE.is_match(&stem_lc),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn normalize_copy_suffix_paren() {
        let n = normalize_filename(OsStr::new("report (1).pdf"));
        assert_eq!(n.ext, "pdf");
        assert!(n.copy_index.is_some());
        assert!(n.has_copy_suffix);
    }

    #[test]
    fn normalize_copy_of_phrase() {
        let n = normalize_filename(OsStr::new("Copy of Budget.numbers"));
        assert!(n.has_copy_suffix);
        assert!(n.normalized_stem.to_ascii_lowercase().contains("budget"));
    }

    #[test]
    fn normalize_stem_collapses_separators() {
        let n = normalize_filename(OsStr::new("my-file__name.txt"));
        assert_eq!(n.ext, "txt");
        assert!(n.normalized_stem.contains("my"));
    }
}
