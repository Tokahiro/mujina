//! The release notes: a version's section of CHANGELOG.md, as it appears on the release page,
//! with Slint's badge under it.

use std::fs;

use crate::TaskResult;
use crate::workspace;

/// Slint's attribution badge, which its royalty-free licence asks for (condition 2 (b)),
/// preferably where the binaries are downloaded.
const SLINT_BADGE: &str = "<a href=\"https://slint.dev\"><img src=\"https://slint.dev/logo/\
                           MadeWithSlint-logo-whitebg.png\" height=\"60\" alt=\"Made with \
                           Slint\"></a>";

/// Prints the section of `tag` (default: `$GITHUB_REF_NAME`) from CHANGELOG.md, then the badge.
pub fn release_notes(tag: Option<&str>) -> TaskResult {
    let tag = match tag {
        Some(tag) => tag.to_string(),
        None => std::env::var("GITHUB_REF_NAME")
            .map_err(|_| "no tag given and GITHUB_REF_NAME is not set".to_string())?,
    };
    let version = tag.strip_prefix('v').unwrap_or(&tag);
    let path = workspace::root().join("CHANGELOG.md");
    let changelog =
        fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let notes = section(&changelog, version)
        .ok_or_else(|| format!("CHANGELOG.md has no entry for {version}"))?;
    println!("{}", page(notes));
    Ok(())
}

/// The release page's text: `notes` unwrapped, and the badge.
fn page(notes: &str) -> String {
    format!("{}\n\n{SLINT_BADGE}", unwrap(notes))
}

/// Joins hard-wrapped lines: GitHub shows every line break of a release body as a break.
fn unwrap(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut previous = "";
    for line in text.lines() {
        let continues = !line.is_empty()
            && !previous.is_empty()
            && !previous.starts_with('#')
            && !line.trim_start().starts_with("- ")
            && !line.starts_with('#');
        if continues {
            out.push(' ');
            out.push_str(line.trim_start());
        } else {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(line);
        }
        previous = line;
    }
    out
}

/// The body of `## [version] …` up to the next `## ` heading, without surrounding blank lines.
/// `None` if the version has no heading or the heading has nothing under it.
fn section<'a>(changelog: &'a str, version: &str) -> Option<&'a str> {
    let heading = format!("## [{version}]");
    let mut offset = 0;
    let mut start = None;
    for line in changelog.split_inclusive('\n') {
        let next = offset + line.len();
        if let Some(from) = start {
            if line.starts_with("## ") {
                return non_empty(&changelog[from..offset]);
            }
        } else if line.starts_with(&heading) {
            start = Some(next);
        }
        offset = next;
    }
    start.and_then(|from| non_empty(&changelog[from..]))
}

fn non_empty(text: &str) -> Option<&str> {
    let text = text.trim();
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::{page, section, unwrap};

    #[test]
    fn wrapped_lines_are_joined_and_items_kept_apart() {
        let text = "### Added\n\n- One,\n  wrapped.\n- Two:\n  - nested,\n    wrapped.\n\nA paragraph\nwrapped.";
        assert_eq!(
            unwrap(text),
            "### Added\n\n- One, wrapped.\n- Two:\n  - nested, wrapped.\n\nA paragraph wrapped."
        );
    }

    const CHANGELOG: &str = "\
# Changelog

## [Unreleased]

## [0.2.0] - 2026-09-22

### Added

- Two.

## [0.1.0] - 2026-09-20

### Fixed

- One.
";

    #[test]
    fn a_section_ends_at_the_next_version() {
        assert_eq!(section(CHANGELOG, "0.2.0"), Some("### Added\n\n- Two."));
    }

    #[test]
    fn the_last_section_ends_with_the_file() {
        assert_eq!(section(CHANGELOG, "0.1.0"), Some("### Fixed\n\n- One."));
    }

    #[test]
    fn a_missing_or_empty_section_is_none() {
        assert_eq!(section(CHANGELOG, "0.3.0"), None);
        assert_eq!(section(CHANGELOG, "Unreleased"), None);
    }

    #[test]
    fn the_page_ends_with_slints_badge() {
        let page = page("### Added\n\n- Two,\n  wrapped.");
        assert!(page.starts_with("### Added\n\n- Two, wrapped.\n\n<a href=\"https://slint.dev\">"));
        assert!(page.contains(
            "<img src=\"https://slint.dev/logo/MadeWithSlint-logo-whitebg.png\" height=\"60\" \
             alt=\"Made with Slint\">"
        ));
        assert!(page.ends_with("</a>"));
    }

    #[test]
    fn a_version_is_not_mistaken_for_a_longer_one() {
        assert_eq!(section(CHANGELOG, "0.2"), None);
    }
}
