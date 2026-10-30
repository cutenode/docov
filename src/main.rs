//! The `docov` command.

use std::{env, io, path::PathBuf, process::ExitCode, time::SystemTime};

use clap::{CommandFactory, FromArgMatches, Parser};
use docov::{Config, Scope, Settings, Threshold, feature, report, scan_file, walk};
use rayon::{iter::Either, prelude::*};

/// Checks how much of a JavaScript or TypeScript project's API has JSDoc.
///
/// Exits 1 when coverage is under the threshold, 2 when a file or the config can't be read.
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
    /// Print the config file's JSON Schema and exit
    #[arg(long, exclusive = true)]
    print_schema: bool,
}

fn main() -> ExitCode {
    // Each feature adds its own flags.
    let matches = feature::args(Cli::command()).get_matches();
    let cli = Cli::from_arg_matches(&matches).expect("the flags are the command's");
    if cli.print_schema {
        print!("{}", Config::json_schema());
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

/// Scans and reports, and says whether coverage met the threshold.
fn run(cli: Cli, matches: &clap::ArgMatches) -> Result<bool, Vec<String>> {
    // GitHub Actions sets it, and reads annotations from the output.
    let github_actions = env::var_os("GITHUB_ACTIONS").is_some_and(|value| value == "true");
    let outputs = cli.report.outputs(github_actions).map_err(|e| vec![e])?;
    let mut config = Config::load(cli.config.as_deref()).map_err(|e| vec![e.to_string()])?;
    let settings = Settings::new(matches, std::mem::take(&mut config.keys));
    // The config's paths are relative to its folder. Collecting the components drops `.` parts after
    // the first (`.././src` is `../src`), which would keep the excludes from matching.
    let config_paths = config.paths.iter().map(|p| config.dir.join(p).components().collect());
    let paths = if cli.paths.is_empty() { config_paths.collect() } else { cli.paths };
    let scope = *settings.get::<Scope>();

    let files = walk::files(&paths, &config.dir, &config.exclude).map_err(|e| vec![e.to_string()])?;
    let (reports, errors): (Vec<_>, Vec<_>) = files.par_iter().partition_map(|path| match scan_file(path, scope) {
        Ok(report) => Either::Left(report),
        Err(e) => Either::Right(e.to_string()),
    });
    if !errors.is_empty() {
        return Err(errors);
    }
    let failures = settings.gates(&reports);
    let threshold = settings.get::<Threshold>().0;
    let (now, text) = (SystemTime::now(), cli.report.text);
    let context = report::Context { files: &reports, failures: &failures, now, threshold, text };
    report::write(&outputs, &context, &mut io::stdout().lock()).map_err(|e| vec![e])?;
    for failure in &failures {
        eprintln!("docov: {failure}");
    }
    Ok(failures.is_empty())
}
