//! Registers every file in src/feature/ as a feature, so adding a feature adds a file and no line
//! in a shared list.

use std::{env, fs, path::Path};

fn main() {
    println!("cargo::rerun-if-changed=src/feature");
    let dir = Path::new(&env::var("CARGO_MANIFEST_DIR").expect("cargo sets it")).join("src/feature");
    let mut names: Vec<String> = fs::read_dir(&dir)
        .expect("src/feature is readable")
        .map(|entry| entry.expect("src/feature is readable").path())
        .filter(|path| path.extension().is_some_and(|e| e == "rs"))
        .map(|path| path.file_stem().and_then(|s| s.to_str()).expect("a UTF-8 file name").to_owned())
        .collect();
    names.sort();

    // `mod` in an included file resolves next to that file, which is in OUT_DIR, so each one names
    // its source.
    let mut out = String::new();
    for name in &names {
        let path = dir.join(format!("{name}.rs"));
        out += &format!("#[path = {:?}]\npub mod {name};\n", path.display().to_string());
    }
    out += "\n/// Makes each feature's entry, in file name order.\nconst ENTRIES: &[fn() -> Entry] = &[\n";
    for name in &names {
        out += &format!("    {name}::entry,\n");
    }
    out += "];\n";
    fs::write(Path::new(&env::var("OUT_DIR").expect("cargo sets it")).join("features.rs"), out)
        .expect("OUT_DIR is writable");
}
