//! `--watch`: scans again whenever a file under the scanned paths changes, until Ctrl-C.

use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};

use clap::{ArgMatches, FromArgMatches};
use notify::RecursiveMode;
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer};
use oxc_span::SourceType;

use crate::{
    config::FILE_NAMES,
    feature::{Entry, Feature, NoKeys},
};

pub(crate) fn entry() -> Entry {
    Entry::of::<Watch>()
}

/// Whether to scan again on changes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Watch(pub bool);

/// The flag.
#[derive(clap::Args)]
#[group(skip)]
pub struct Flags {
    /// Scan again whenever a file under the paths changes, until Ctrl-C
    #[arg(long)]
    watch: bool,
}

impl Feature for Watch {
    type Flags = Flags;
    type Keys = NoKeys;

    fn new(flags: Flags, _: NoKeys) -> Self {
        Self(flags.watch)
    }
}

/// Whether `matches` has `--watch`, which is known before the config is read.
pub fn enabled(matches: &ArgMatches) -> bool {
    Flags::from_arg_matches(matches).expect("the flags were added by `args`").watch
}

/// The line between one scan's report and the next.
pub const SEPARATOR: &str = "--- docov: files changed, scanning again ---";

/// How long a burst of changes has to go quiet before a scan, so a save or a checkout scans once.
const QUIET: Duration = Duration::from_millis(200);

/// Runs `scan`, which says which paths it scanned and returns an exit code. With `watch`, it then runs it again after each burst of changes to a JavaScript,
/// TypeScript or config file under those paths, printing [`SEPARATOR`] first, and doesn't return
/// unless the paths can't be watched (2). Without it, it returns `scan`'s code.
pub fn run(watch: bool, scan: &mut dyn FnMut(&mut Vec<PathBuf>) -> u8) -> u8 {
    let mut paths = Vec::new();
    let code = scan(&mut paths);
    if !watch {
        return code;
    }
    let (send, changes) = mpsc::channel();
    let mut watched: Option<(Vec<PathBuf>, Debouncer<notify::RecommendedWatcher>)> = None;
    loop {
        // The paths can change with the config between scans.
        if watched.as_ref().is_none_or(|(old, _)| *old != paths) {
            match watcher(&paths, send.clone()) {
                Ok(watcher) => watched = Some((paths.clone(), watcher)),
                Err(e) => {
                    eprintln!("docov: {e}");
                    return 2;
                }
            }
        }
        // This holds a sender, so the channel stays open.
        let _ = changes.iter().find(changed).expect("the channel is open");
        // A closed stdout fails the scan's own write, so this one can fail quietly.
        let _ = writeln!(io::stdout(), "{SEPARATOR}");
        scan(&mut paths);
    }
}

/// A watcher of `paths` that sends each burst of changes to `send`.
fn watcher(paths: &[PathBuf], send: mpsc::Sender<DebounceEventResult>) -> notify::Result<Debouncer<notify::RecommendedWatcher>> {
    new_debouncer(QUIET, send).and_then(|mut debouncer| {
        let watched = paths.iter().try_for_each(|path| debouncer.watcher().watch(path, RecursiveMode::Recursive));
        watched.map(|()| debouncer)
    })
}

/// Whether a burst of changes touched a file a scan reads.
fn changed(changes: &DebounceEventResult) -> bool {
    changes.as_ref().is_ok_and(|events| events.iter().any(|event| scanned(&event.path)))
}

/// Whether a scan reads the file at `path`: JavaScript, TypeScript or a config file.
fn scanned(path: &Path) -> bool {
    SourceType::from_path(path).is_ok() || path.file_name().is_some_and(|name| FILE_NAMES.iter().any(|n| name == *n))
}
