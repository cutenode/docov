//! Any text as docov.json, loaded, then raised by `--update-threshold`. Loading and the rewrite read
//! the file with different parsers, so this is where they disagree: a config that loads has to be
//! one the rewrite can read, whose raised threshold loads, and that a second raise leaves alone.

#![no_main]

use std::{fs, sync::OnceLock};

use docov::{Config, Totals, feature::update_threshold::UpdateThreshold};
use libfuzzer_sys::fuzz_target;

/// A folder of this process's own to write the config in.
fn dir() -> &'static tempfile::TempDir {
    static DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
    DIR.get_or_init(|| tempfile::tempdir().expect("a temporary folder"))
}

fuzz_target!(|input: (u8, u8, &str)| {
    let (documented, total, text) = input;
    // `extends` reads the files a config names, and reading a device or a pipe wouldn't end.
    if text.contains("dev/") || text.contains("proc/") {
        return;
    }
    let path = dir().path().join("docov.json");
    fs::write(&path, text).expect("the config is written");
    if Config::load(Some(&path)).is_err() {
        return;
    }
    let totals = Totals { documented: usize::from(documented.min(total)), total: usize::from(total) };
    let raised = UpdateThreshold(true).update(Some(&path), totals).expect("the config is rewritten");
    if raised.is_some() {
        let text = fs::read_to_string(&path).expect("the config is read");
        Config::load(Some(&path)).unwrap_or_else(|e| panic!("{e}\n{text}"));
        assert_eq!(UpdateThreshold(true).update(Some(&path), totals), Ok(None), "{text}");
    }
});
