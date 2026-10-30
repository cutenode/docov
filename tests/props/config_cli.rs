//! Properties of config loading and the command line's conveniences.

use std::{
    path::{Path, PathBuf},
    process::Command,
};

use docov::{config, config::Config, walk::Exclude};
use hegel::{TestCase, generators as gs};

use super::write;

/// The folders generated config files go in.
const FOLDERS: [&str; 4] = ["", "x/", "x/y/", "z/"];

/// A generated config file.
struct File {
    folder: &'static str,
    threshold: Option<u8>,
    scope: Option<&'static str>,
    paths: bool,
    exclude: bool,
    /// The files it extends, by index.
    extends: Vec<usize>,
}

impl File {
    fn name(&self, i: usize) -> String {
        format!("{}f{i}.json", self.folder)
    }
}

/// The path from `folder` to `to`, both in the generated project.
fn from(folder: &str, to: &str) -> String {
    "../".repeat(folder.matches('/').count()) + to
}

/// What loading a file should find.
#[derive(Default)]
struct Model {
    threshold: Option<u8>,
    scope: Option<&'static str>,
    paths: Option<Vec<PathBuf>>,
    exclude: Option<Exclude>,
    files: Vec<PathBuf>,
}

/// What loading file `i`, named `path`, should find: its own keys over those of the files it
/// extends, each over the ones before it.
fn model(files: &[File], i: usize, path: &Path) -> Model {
    let dir = path.parent().unwrap();
    let mut merged = Model::default();
    for &base in &files[i].extends {
        let base = model(files, base, &dir.join(from(files[i].folder, &files[base].name(base))));
        merged = Model {
            threshold: base.threshold.or(merged.threshold),
            scope: base.scope.or(merged.scope),
            paths: base.paths.or(merged.paths),
            exclude: base.exclude.or(merged.exclude),
            files: [base.files, merged.files].concat(),
        };
    }
    let file = &files[i];
    Model {
        threshold: file.threshold.or(merged.threshold),
        scope: file.scope.or(merged.scope),
        paths: if file.paths { Some(vec![dir.join(format!("p{i}"))]) } else { merged.paths },
        exclude: if file.exclude {
            Some(Exclude { root: dir.to_owned(), globs: vec![format!("e{i}/**")] })
        } else {
            merged.exclude
        },
        files: [vec![path.to_owned()], merged.files].concat(),
    }
}

/// How a generated chain of config files goes wrong, if it does.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Twist {
    None,
    /// The last file extends the first, which reaches it.
    Cycle,
    /// The last file extends one that isn't there.
    Missing,
    /// The last file extends a folder.
    Folder,
}

/// Config files in different folders extending each other, one path or a list of them at a time:
/// loading the first finds each key in the nearest file that has it, `paths` and `exclude`
/// relative to the file that wrote them, and lists the files nearest first. A file that extends
/// itself through others, one that isn't there and a folder each fail, naming the culprit.
#[hegel::test(test_cases = 60)]
fn extended_configs_merge_nearest_first(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let twist = tc.draw(gs::sampled_from(vec![Twist::None, Twist::Cycle, Twist::Missing, Twist::Folder]));
    let count = tc.draw(gs::integers::<usize>().min_value(1).max_value(4));
    let mut files: Vec<File> = (0..count)
        .map(|i| File {
            folder: tc.draw(gs::sampled_from(FOLDERS.to_vec())),
            threshold: tc.draw(gs::optional(gs::integers::<u8>().max_value(100))),
            scope: tc.draw(gs::optional(gs::sampled_from(vec!["exports", "all"]))),
            paths: tc.draw(gs::booleans()),
            exclude: tc.draw(gs::booleans()),
            // Only later files, so there's no cycle unless the twist makes one.
            extends: tc.draw(gs::vecs(gs::integers::<usize>().min_value(i + 1).max_value(count)).max_size(2)),
        })
        .collect();
    for (i, file) in files.iter_mut().enumerate() {
        file.extends.retain(|&j| j < count);
        // A twist needs the last file reached.
        if twist != Twist::None && i + 1 < count && !file.extends.contains(&(i + 1)) {
            file.extends.push(i + 1);
        }
    }
    for (i, file) in files.iter().enumerate() {
        let mut extends: Vec<String> = file.extends.iter().map(|&j| from(file.folder, &files[j].name(j))).collect();
        if i + 1 == count {
            match twist {
                Twist::None => {}
                Twist::Cycle => extends.push(from(file.folder, &files[0].name(0))),
                Twist::Missing => extends.push("missing.json".into()),
                Twist::Folder => extends.push(".".into()),
            }
        }
        let mut json = serde_json::json!({ "$schema": "https://example.com/docov.schema.json" });
        if extends.len() == 1 && tc.draw(gs::booleans()) {
            json["extends"] = extends[0].clone().into();
        } else if !extends.is_empty() {
            json["extends"] = extends.into();
        }
        if let Some(threshold) = file.threshold {
            json["threshold"] = threshold.into();
        }
        if let Some(scope) = file.scope {
            json["scope"] = scope.into();
        }
        if file.paths {
            // `./` adds nothing.
            json["paths"] = vec![format!("{}p{i}", if tc.draw(gs::booleans()) { "./" } else { "" })].into();
        }
        if file.exclude {
            json["exclude"] = vec![format!("e{i}/**")].into();
        }
        write(dir.path(), &file.name(i), &json.to_string());
    }

    let first = dir.path().join(files[0].name(0));
    let loaded = Config::load(Some(&first));
    match twist {
        Twist::None => {}
        Twist::Cycle => {
            let error = loaded.unwrap_err();
            let config::Error::Cycle { chain } = &error else { panic!("{error:?}") };
            // It goes back to the first file, the only one extended twice.
            let real = |p: &PathBuf| std::fs::canonicalize(p).unwrap();
            assert_eq!((&chain[0], real(&chain[0])), (&first, real(chain.last().unwrap())), "{chain:?}");
            let names: Vec<_> = chain.iter().map(|p| p.display().to_string()).collect();
            assert!(error.to_string().ends_with(&names.join(" -> ")), "{error}");
            return;
        }
        Twist::Missing | Twist::Folder => {
            let Err(config::Error::Read { path, .. }) = loaded else { panic!("{loaded:?}") };
            let name = if twist == Twist::Missing { "missing.json" } else { "." };
            assert!(path.to_str().unwrap().ends_with(name), "{path:?}");
            return;
        }
    }
    let loaded = loaded.unwrap();
    let expected = model(&files, 0, &first);
    assert_eq!(loaded.paths, expected.paths.unwrap_or_default());
    assert_eq!(loaded.exclude, expected.exclude.into_iter().collect::<Vec<_>>());
    assert_eq!(loaded.files, expected.files);
    assert_eq!(loaded.dir, first.parent().unwrap());
    // The merged keys are those of one file with each key's winning value.
    let flat = serde_json::json!({ "threshold": expected.threshold, "scope": expected.scope.unwrap_or("exports") });
    write(dir.path(), "flat.json", &flat.to_string());
    assert_eq!(loaded.keys, Config::load(Some(&dir.path().join("flat.json"))).unwrap().keys);
}

/// Runs the binary in `dir`, with stdout closed if `closed` (a pipe whose reader has gone, as with
/// `docov | head` after head exits), and returns its code, stdout and stderr.
fn docov(dir: &Path, args: &[&str], closed: bool) -> (i32, String, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_docov"));
    command.args(args).current_dir(dir);
    if closed {
        let (reader, writer) = std::io::pipe().unwrap();
        drop(reader);
        command.stdout(writer);
    }
    let out = command.output().unwrap();
    let text = |bytes| String::from_utf8(bytes).unwrap();
    (out.status.code().unwrap(), text(out.stdout), text(out.stderr))
}

/// `--print-config` prints the files the settings came from, the paths, the excludes and each
/// feature's setting, flags over keys, and `--list-files` the files a scan would read. Neither
/// scans, so a file that doesn't parse changes nothing, and each fails when stdout is closed.
#[hegel::test(test_cases = 5)]
fn the_cli_prints_its_settings_and_files(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("lib/gen")).unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    let mut listed = Vec::new();
    for f in 0..tc.draw(gs::integers::<usize>().max_value(4)) {
        let folder = tc.draw(gs::sampled_from(vec!["src", "lib", "lib/gen"]));
        let path = format!("{folder}/f{f}.ts");
        write(dir.path(), &path, "export function (");
        if folder != "lib/gen" {
            listed.push(Path::new(folder).join(format!("f{f}.ts")).display().to_string());
        }
    }
    listed.sort();
    let threshold = tc.draw(gs::optional(gs::integers::<u8>().max_value(100)));
    let scope = tc.draw(gs::sampled_from(vec!["exports", "all"]));
    let config =
        serde_json::json!({ "paths": ["src", "lib"], "exclude": ["gen/"], "threshold": threshold, "scope": scope });
    write(dir.path(), "docov.json", &config.to_string());
    let flag = tc.draw(gs::optional(gs::sampled_from(vec!["exports", "all"])));
    let mut args = vec!["--print-config"];
    if let Some(flag) = flag {
        args.extend(["--scope", flag]);
    }

    let (code, out, err) = docov(dir.path(), &args, false);
    assert_eq!((code, err.as_str()), (0, ""));
    let json: serde_json::Value = serde_json::from_str(&out).unwrap();
    let expected = serde_json::json!({
        "files": ["docov.json"],
        "paths": ["src", "lib"],
        "exclude": [{ "root": "", "globs": ["gen/"] }],
        "features": { "scope": flag.unwrap_or(scope), "threshold": threshold.map(f64::from) },
    });
    assert_eq!(json["features"]["scope"], expected["features"]["scope"]);
    assert_eq!(json["features"]["threshold"], expected["features"]["threshold"]);
    for key in ["files", "paths", "exclude"] {
        assert_eq!(json[key], expected[key], "{key}");
    }

    let (code, out, err) = docov(dir.path(), &["--list-files"], false);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(out.lines().collect::<Vec<_>>(), listed);

    for args in [["--print-config"], ["--list-files"]] {
        let (code, _, err) = docov(dir.path(), &args, true);
        // With nothing to list, there's nothing to fail to write.
        let expected = if args[0] == "--list-files" && listed.is_empty() { 0 } else { 2 };
        assert_eq!(code, expected, "{args:?} {err}");
        assert!(code == 0 || err.starts_with("docov: "), "{err}");
    }
}
