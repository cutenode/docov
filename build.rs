//! Registers every file in src/feature/ as a feature and every file in src/check/ as a doc check,
//! so adding either adds a file and no line in a shared list.

use std::{env, fs, path::Path};

fn main() {
    register(
        "feature",
        "features.rs",
        "/// Makes each feature's entry, in file name order.\nconst ENTRIES: &[fn() -> Entry]",
        |name| format!("{name}::entry"),
    );
    // A check's rule id is its file name in kebab case.
    register(
        "check",
        "checks.rs",
        "/// Each check's rule id and its entry, in file name order.\nconst ENTRIES: &[Registered]",
        |name| format!("({:?}, {name}::entry)", name.replace('_', "-")),
    );
}

/// Writes `out` in OUT_DIR: a module for each file in src/`folder`/, then `list` holding `entry` of
/// each.
fn register(folder: &str, out: &str, list: &str, entry: impl Fn(&str) -> String) {
    println!("cargo::rerun-if-changed=src/{folder}");
    let dir = Path::new(&env::var("CARGO_MANIFEST_DIR").expect("cargo sets it")).join("src").join(folder);
    let mut names: Vec<String> = fs::read_dir(&dir)
        .expect("the folder is readable")
        .map(|entry| entry.expect("the folder is readable").path())
        .filter(|path| path.extension().is_some_and(|e| e == "rs"))
        .map(|path| path.file_stem().and_then(|s| s.to_str()).expect("a UTF-8 file name").to_owned())
        .collect();
    names.sort();

    // `mod` in an included file resolves next to that file, which is in OUT_DIR, so each one names
    // its source.
    let mut text = String::new();
    for name in &names {
        let path = dir.join(format!("{name}.rs"));
        text += &format!("#[path = {:?}]\npub mod {name};\n", path.display().to_string());
    }
    text += &format!("\n{list} = &[\n");
    for name in &names {
        text += &format!("    {},\n", entry(name));
    }
    text += "];\n";
    fs::write(Path::new(&env::var("OUT_DIR").expect("cargo sets it")).join(out), text).expect("OUT_DIR is writable");
}
