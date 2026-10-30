//! Asks git which files and lines changed, by running it.

use std::{
    ops::RangeInclusive,
    path::{Path, PathBuf},
    process::Command,
};

/// The repository's top folder, as a path from the current folder: git names files from it.
const TOP: &[&str] = &["rev-parse", "--show-cdup"];
/// Files git doesn't track and doesn't ignore, anywhere in the repository.
const UNTRACKED: &[&str] = &["ls-files", "--others", "--exclude-standard", "--full-name", "-z", "--", ":/"];

/// Each command's output, run in the current folder, or why the first to fail did.
fn git(commands: &[&[&str]]) -> Result<Vec<String>, String> {
    let run = |args: &&[&str]| {
        let command = format!("git {}", args.join(" "));
        let out = Command::new("git").args(*args).output().map_err(|e| format!("{command}: {e}"))?;
        if !out.status.success() {
            return Err(format!("{command}: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    };
    commands.iter().map(run).collect()
}

/// What to compare with: `since`, else `origin/HEAD` (the remote's default branch) when there is
/// one, else `main`. Git compares with where HEAD branched from it.
pub fn base(since: Option<&str>) -> &str {
    let has_origin = || git(&[&["rev-parse", "--verify", "--quiet", "origin/HEAD"]]).is_ok();
    since.unwrap_or_else(|| if has_origin() { "origin/HEAD" } else { "main" })
}

/// The `\0`-separated names in `list`, which git gives from the top folder `top` (its
/// `--show-cdup` output), as paths from the current folder.
fn names<'a>(top: &'a str, list: &'a str) -> impl Iterator<Item = PathBuf> + 'a {
    list.split_terminator('\0').map(|name| Path::new(top.trim_end()).join(name))
}

/// Files that differ from where HEAD branched from `base`, committed or not, and untracked ones,
/// as paths from the current folder.
pub fn changed_since(base: &str) -> Result<Vec<PathBuf>, String> {
    let out = git(&[TOP, &["diff", "--name-only", "-z", "--merge-base", base], UNTRACKED])?;
    Ok(names(&out[0], &out[1]).chain(names(&out[0], &out[2])).collect())
}

/// Files staged for the next commit, as paths from the current folder.
pub fn staged() -> Result<Vec<PathBuf>, String> {
    let out = git(&[TOP, &["diff", "--name-only", "-z", "--cached"]])?;
    Ok(names(&out[0], &out[1]).collect())
}

/// A file's lines that a change added.
pub struct Added {
    /// The file, as a path from the current folder.
    pub path: PathBuf,
    /// The lines, 1-based, or `None` for all of them.
    pub lines: Option<Vec<RangeInclusive<usize>>>,
}

/// The lines added since where HEAD branched from `base`, committed or not, in each file; all of
/// an untracked file's.
pub fn added_lines(base: &str) -> Result<Vec<Added>, String> {
    // Without a/ and b/ before the names, and without quotes around names with characters past
    // ASCII. Control characters, `"` and `\` still get them, and those files are missed.
    let diff = ["-c", "core.quotePath=false", "diff", "--unified=0", "--no-prefix", "--no-color", "--no-ext-diff"];
    let out = git(&[TOP, &[&diff[..], &["--merge-base", base]].concat(), UNTRACKED])?;
    let mut files: Vec<Added> = Vec::new();
    // `+++ ` starts a file's name only in its header, before its first hunk: in a hunk, it's an
    // added line that starts with `++ `.
    let mut header = false;
    for line in out[1].lines() {
        if line.starts_with("diff --git ") {
            header = true;
        } else if let Some(name) = line.strip_prefix("+++ ").filter(|_| header) {
            files.push(Added { path: Path::new(out[0].trim_end()).join(name), lines: Some(Vec::new()) });
        } else if let Some(hunk) = line.strip_prefix("@@ ") {
            header = false;
            // `@@ -a,b +c,d @@`: d lines from line c, where a missing d is 1.
            let added = hunk.split(' ').nth(1).unwrap_or_default().trim_start_matches('+');
            let (start, count) = added.split_once(',').unwrap_or((added, "1"));
            let (start, count): (usize, usize) = (start.parse().unwrap_or_default(), count.parse().unwrap_or_default());
            if let Some(Added { lines: Some(lines), .. }) = files.last_mut().filter(|_| count > 0) {
                lines.push(start..=start + count - 1);
            }
        }
    }
    files.extend(names(&out[0], &out[2]).map(|path| Added { path, lines: None }));
    Ok(files)
}
