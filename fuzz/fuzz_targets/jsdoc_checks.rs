//! Any text as the inside of the JSDoc blocks of a function and a class member, with every doc
//! check on: `valid-link`'s and `doc-markdown`'s offsets into the block, and where they land as
//! lines and columns. The parser never sees anything but a block, so this gets to the checks sooner
//! than the scan target.

#![no_main]

use std::path::Path;

use docov::collect::scan_source;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: (u8, &str)| {
    let (pick, text) = input;
    // Unless the text ends the block itself (`*/`, or a `/` that makes `/**/`), the source parses.
    let block = format!("/**{text}*/");
    if block.find("*/") != Some(block.len() - 2) {
        return;
    }
    // Type parameters are TypeScript's.
    let (path, generic) = if pick & 1 == 0 { ("a.ts", "<T>") } else { ("a.js", "") };
    let source = format!(
        "{block}\nexport function* f{generic}(a, {{ b }}, ...c) {{ yield a; throw b; }}\n\
         export class C {{\n  {block}\n  m(x) {{ return x; }}\n}}\n"
    );
    let path = Path::new(path);
    let report = scan_source(path, &source, &docov_fuzz::rules(pick >> 1)).expect("the source parses");
    docov_fuzz::check(&report, &source);
});
