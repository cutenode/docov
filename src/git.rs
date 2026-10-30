//! Asks git which files changed, by running it.

use std::{
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
