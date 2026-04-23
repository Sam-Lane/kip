use anyhow::{bail, Result};
use git2::{Oid, Repository};
use regex::Regex;
use semver::Version;

use crate::git::{list_tags, resolve_oid};

pub fn resolve_best_ref(repo: &Repository, pattern: &str) -> Result<(String, Oid)> {
    // 1. Try exact ref first.
    if let Ok(oid) = resolve_oid(repo, pattern) {
        return Ok((pattern.to_owned(), oid));
    }

    let tags = list_tags(repo)?;

    // 2. Semver pattern: contains digits and dots/wildcards like "v1.*" or "1.2.*"
    if looks_like_semver_pattern(pattern) {
        return match_semver(repo, &tags, pattern);
    }

    // 3. Fallback: treat pattern as a regex over tag names, pick lexicographic latest.
    match_regex(repo, &tags, pattern)
}

fn looks_like_semver_pattern(pattern: &str) -> bool {
    let stripped = pattern.strip_prefix('v').unwrap_or(pattern);
    stripped.contains('.')
        && stripped
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
}

fn pattern_to_regex(pattern: &str) -> Result<Regex> {
    // Convert glob-style wildcards to regex. Escape everything else.
    let mut re = String::from("^v?");
    let stripped = pattern.strip_prefix('v').unwrap_or(pattern);
    for ch in stripped.chars() {
        match ch {
            '*' => re.push_str(".*"),
            '?' => re.push('.'),
            '.' => re.push_str(r"\."),
            c => {
                let s = regex::escape(&c.to_string());
                re.push_str(&s);
            }
        }
    }
    re.push('$');
    Regex::new(&re).map_err(|e| anyhow::anyhow!("invalid pattern {pattern:?}: {e}"))
}

fn match_semver(repo: &Repository, tags: &[String], pattern: &str) -> Result<(String, Oid)> {
    let best_tag = best_semver_tag(tags, pattern)?;
    let oid = resolve_oid(repo, &best_tag)?;
    Ok((best_tag, oid))
}

fn match_regex(repo: &Repository, tags: &[String], pattern: &str) -> Result<(String, Oid)> {
    let best_tag = best_regex_tag(tags, pattern)?;
    let oid = resolve_oid(repo, &best_tag)?;
    Ok((best_tag, oid))
}

pub(crate) fn best_semver_tag(tags: &[String], pattern: &str) -> Result<String> {
    let re = pattern_to_regex(pattern)?;

    let mut candidates: Vec<(Version, String)> = tags
        .iter()
        .filter(|t| re.is_match(t))
        .filter_map(|t| {
            let stripped = t.strip_prefix('v').unwrap_or(t);
            Version::parse(stripped).ok().map(|v| (v, t.clone()))
        })
        .collect();

    if candidates.is_empty() {
        bail!("no tags match semver pattern {pattern:?}");
    }

    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(candidates.remove(0).1)
}

pub(crate) fn best_regex_tag(tags: &[String], pattern: &str) -> Result<String> {
    let re = Regex::new(pattern)
        .map_err(|e| anyhow::anyhow!("invalid regex pattern {pattern:?}: {e}"))?;

    let mut candidates: Vec<&String> = tags.iter().filter(|t| re.is_match(t)).collect();

    if candidates.is_empty() {
        bail!("no tags match pattern {pattern:?}");
    }

    candidates.sort();
    Ok((*candidates.last().unwrap()).clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    // --- looks_like_semver_pattern ---

    #[test]
    fn semver_pattern_detected_with_v_prefix() {
        assert!(looks_like_semver_pattern("v1.*"));
        assert!(looks_like_semver_pattern("v1.2.*"));
        assert!(looks_like_semver_pattern("v0.1.*"));
    }

    #[test]
    fn semver_pattern_detected_without_v_prefix() {
        assert!(looks_like_semver_pattern("1.*"));
        assert!(looks_like_semver_pattern("1.2.*"));
    }

    #[test]
    fn semver_pattern_not_detected_for_branch_names() {
        assert!(!looks_like_semver_pattern("main"));
        assert!(!looks_like_semver_pattern("release-*"));
        assert!(!looks_like_semver_pattern("feat/thing"));
    }

    // --- pattern_to_regex ---

    #[test]
    fn pattern_to_regex_matches_wildcard() {
        let re = pattern_to_regex("v1.*").unwrap();
        assert!(re.is_match("v1.0.0"));
        assert!(re.is_match("v1.99.0"));
        assert!(re.is_match("1.0.0")); // no v prefix
        assert!(!re.is_match("v2.0.0"));
    }

    #[test]
    fn pattern_to_regex_exact_version() {
        let re = pattern_to_regex("v1.2.3").unwrap();
        assert!(re.is_match("v1.2.3"));
        assert!(re.is_match("1.2.3"));
        assert!(!re.is_match("v1.2.4"));
    }

    #[test]
    fn pattern_to_regex_question_mark_wildcard() {
        let re = pattern_to_regex("v1.?.0").unwrap();
        assert!(re.is_match("v1.2.0"));
        assert!(re.is_match("v1.9.0"));
        assert!(!re.is_match("v1.22.0")); // ? matches single char only
    }

    // --- best_semver_tag ---

    #[test]
    fn semver_picks_highest_matching_version() {
        let t = tags(&["v1.0.0", "v1.2.0", "v1.1.0", "v2.0.0"]);
        assert_eq!(best_semver_tag(&t, "v1.*").unwrap(), "v1.2.0");
    }

    #[test]
    fn semver_picks_across_minor_and_patch() {
        let t = tags(&["v2.0.0", "v2.0.1", "v2.1.0"]);
        assert_eq!(best_semver_tag(&t, "v2.*").unwrap(), "v2.1.0");
    }

    #[test]
    fn semver_ignores_non_matching_tags() {
        let t = tags(&["v1.0.0", "v2.0.0", "v3.0.0"]);
        assert_eq!(best_semver_tag(&t, "v2.*").unwrap(), "v2.0.0");
    }

    #[test]
    fn semver_no_match_returns_error() {
        let t = tags(&["v1.0.0", "v2.0.0"]);
        assert!(best_semver_tag(&t, "v9.*").is_err());
    }

    #[test]
    fn semver_works_with_tags_without_v_prefix() {
        let t = tags(&["1.0.0", "1.1.0", "2.0.0"]);
        assert_eq!(best_semver_tag(&t, "1.*").unwrap(), "1.1.0");
    }

    // --- best_regex_tag ---

    #[test]
    fn regex_picks_lexicographic_latest() {
        let t = tags(&["release-1.0", "release-1.2", "release-1.1"]);
        assert_eq!(best_regex_tag(&t, r"^release-").unwrap(), "release-1.2");
    }

    #[test]
    fn regex_filters_by_pattern() {
        let t = tags(&["v1.0.0", "nightly-2024", "stable-1.0"]);
        assert_eq!(best_regex_tag(&t, r"^stable-").unwrap(), "stable-1.0");
    }

    #[test]
    fn regex_no_match_returns_error() {
        let t = tags(&["v1.0.0", "v2.0.0"]);
        assert!(best_regex_tag(&t, r"^release-").is_err());
    }

    #[test]
    fn regex_invalid_pattern_returns_error() {
        let t = tags(&["v1.0.0"]);
        assert!(best_regex_tag(&t, r"[invalid").is_err());
    }
}
