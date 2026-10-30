//! The `docov` command.

use std::{env, io, path::PathBuf, process::ExitCode, time::SystemTime};

use clap::{CommandFactory, FromArgMatches, Parser};
use docov::{
    Config, Settings, Threshold, Totals,
    feature::{
        self,
        baseline::{self, Baseline},
        changed::Changed,
        diff::Diff,
        overrides::Overrides,
        threshold::Limit,
        update_threshold::UpdateThreshold,
    },
    glob::Globs,
    report,
    root::Root,
    scan_file, walk,
};
use rayon::{iter::Either, prelude::*};

/// Checks how much of a JavaScript or TypeScript project's API has JSDoc.
///
/// Exits 1 when a gate fails (a threshold, say), 2 when a file or the config can't be read.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Files and folders to scan [default: the config's `paths`, or the current folder]
    paths: Vec<PathBuf>,
    /// The config file [default: docov.json, docov.jsonc or .docovrc here or in the nearest folder above]
    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,
    #[command(flatten)]
    report: report::Flags,
    /// Print the config file's JSON Schema, or the baseline file's, and exit
    #[arg(long, value_enum, value_name = "FILE", exclusive = true, num_args = 0..=1, default_missing_value = "config")]
    print_schema: Option<Schema>,
}

/// A file docov reads, by its JSON Schema.
#[derive(Clone, Copy, clap::ValueEnum)]
enum Schema {
    /// docov.json.
    Config,
    /// The file `--write-baseline` writes.
    Baseline,
}

fn main() -> ExitCode {
    // Each feature adds its own flags.
    let matches = feature::args(Cli::command()).get_matches();
    let cli = Cli::from_arg_matches(&matches).expect("the flags are the command's");
    if let Some(schema) = cli.print_schema {
        match schema {
            Schema::Config => print!("{}", Config::json_schema()),
            Schema::Baseline => print!("{}", baseline::File::json_schema()),
        }
        return ExitCode::SUCCESS;
    }
    match run(cli, &matches) {
        Ok(passed) => ExitCode::from(u8::from(!passed)),
        Err(errors) => {
            for error in errors {
                eprintln!("docov: {error}");
            }
            ExitCode::from(2)
        }
    }
}

/// Scans and reports, and says whether every gate passed.
fn run(cli: Cli, matches: &clap::ArgMatches) -> Result<bool, Vec<String>> {
    // GitHub Actions sets it, and reads annotations from the output.
    let github_actions = env::var_os("GITHUB_ACTIONS").is_some_and(|value| value == "true");
    let outputs = cli.report.outputs(github_actions).map_err(|e| vec![e])?;
    let base = cli.report.compare.as_deref().map(report::Base::read).transpose().map_err(|e| vec![e])?;
    let mut config = Config::load(cli.config.as_deref()).map_err(|e| vec![e.to_string()])?;
    let settings = Settings::new(matches, std::mem::take(&mut config.keys));
    // The config's paths are relative to its folder. Collecting the components drops `.` parts after
    // the first (`.././src` is `../src`), which would keep the excludes from matching.
    let config_paths = config.paths.iter().map(|p| config.dir.join(p).components().collect());
    let paths = if cli.paths.is_empty() { config_paths.collect() } else { cli.paths };
    let rules = settings.rules();
    let baseline = settings.get::<Baseline>();
    let excused = baseline.read(&config.dir).map_err(|e| vec![e])?;
    let diff = settings.get::<Diff>();

    let root = Root::new(&config.dir);
    let exclude = Globs::new(std::mem::take(&mut config.exclude)).map_err(|e| vec![e.to_string()])?;
    let excluded = settings.get::<Overrides>().clone().excluder(exclude, root.clone());
    let files = walk::files(&paths, excluded).map_err(|e| vec![e.to_string()])?;
    let files = settings.get::<Changed>().filter(files, &root).map_err(|e| vec![e])?;
    let added = diff.added(&root).map_err(|e| vec![e])?;
    let (reports, errors): (Vec<_>, Vec<_>) = files.par_iter().partition_map(|path| match scan_file(path, &rules) {
        Ok(report) => Either::Left(report),
        Err(e) => Either::Right(e.to_string()),
    });
    if !errors.is_empty() {
        return Err(errors);
    }
    if let Some(edited) = settings.edit(&reports) {
        eprintln!("docov: {}", edited.map_err(|e| vec![e])?);
        return Ok(true);
    }
    // Writing a baseline exits 0, so no gate runs. The gates let the baseline's items through,
    // which the report still lists.
    let failures = if baseline.write {
        Vec::new()
    } else {
        let gated = excused.map(|excused| excused.apply(&root, reports.clone()));
        let gated = gated.as_deref().unwrap_or(&reports);
        let mut failures = settings.gates(gated, &root);
        failures.extend(diff.gate(added.as_ref(), gated, &root));
        failures
    };
    // Badges are colored by the global percentage.
    let threshold = match settings.get::<Threshold>().0.global {
        Some(Limit::Percent(percent)) => Some(percent),
        _ => None,
    };
    let (now, text) = (SystemTime::now(), cli.report.text);
    let context = report::Context { files: &reports, failures: &failures, now, threshold, text, base: base.as_ref() };
    report::write(&outputs, &context, &mut io::stdout().lock()).map_err(|e| vec![e])?;
    if baseline.write {
        let path = baseline.save(&config.dir, &root, &reports).map_err(|e| vec![e])?;
        eprintln!("docov: wrote the undocumented items to {}", path.display());
        return Ok(true);
    }
    let raised = settings.get::<UpdateThreshold>().update(config.path.as_deref(), Totals::of(&reports));
    if let Some(raised) = raised.map_err(|e| vec![e])? {
        eprintln!("docov: {raised}");
    }
    for failure in &failures {
        eprintln!("docov: {failure}");
    }
    Ok(failures.is_empty())
}
