//! Any text, scanned as each language oxc parses with every feature and doc check on: the parse,
//! the walk, the doc checks and the stubs. The first byte picks the language and the rules, so a
//! source file with a byte in front is a seed.

#![no_main]

use std::path::Path;

use docov::collect::{Error, scan_source};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&pick, source)) = data.split_first() else { return };
    let Ok(source) = std::str::from_utf8(source) else { return };
    let path = Path::new(docov_fuzz::FILES[usize::from(pick) % docov_fuzz::FILES.len()]);
    match scan_source(path, source, &docov_fuzz::rules(pick >> 4)) {
        Ok(report) => docov_fuzz::check(&report, source),
        // Where a syntax error is goes through the same line count.
        Err(Error::Syntax { line, column, .. }) => {
            assert!(line >= 1 && line <= source.split('\n').count() && column >= 1, "{line}:{column}");
        }
        Err(e) => panic!("{e}"),
    }
});
