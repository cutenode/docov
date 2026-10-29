//! The `docov` command.

use std::{io, path::PathBuf, process::ExitCode};

use clap::Parser;
use docov::{Config, Format, Scope, Totals, config, report, scan_file, walk};
use rayon::{iter::Either, prelude::*};

/// Checks how much of a JavaScript or TypeScript project's API has JSDoc.
///
/// Exits 1 when coverage is under the threshold, 2 when a file or the config can't be read.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Files and folders to scan [default: the config's `paths`, or the current folder]
    paths: Vec<PathBuf>,
    /// The config file [default: docov.json, if there is one]
    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,
    /// Fail under this documented percentage
    #[arg(long, value_name = "PERCENT", value_parser = config::percent)]
    threshold: Option<f64>,
    /// Which declarations count [default: the config's, or exports]
    #[arg(long, value_enum)]
    scope: Option<Scope>,
    /// How to write the report
    #[arg(long, value_enum, default_value_t)]
    format: Format,
    /// Print the config file's JSON Schema and exit
    #[arg(long, exclusive = true)]
    print_schema: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if cli.print_schema {
        print!("{}", Config::json_schema());
        return ExitCode::SUCCESS;
    }
    match run(cli) {
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
fn run(cli: Cli) -> Result<bool, Vec<String>> {
    let config = Config::load(cli.config.as_deref()).map_err(|e| vec![e.to_string()])?;
    let paths = if cli.paths.is_empty() { config.paths } else { cli.paths };
    let scope = cli.scope.unwrap_or(config.scope);
    let threshold = cli.threshold.or(config.threshold);

    let files = walk::files(&paths, &config.exclude).map_err(|e| vec![e.to_string()])?;
    let (reports, errors): (Vec<_>, Vec<_>) = files.par_iter().partition_map(|path| match scan_file(path, scope) {
        Ok(report) => Either::Left(report),
        Err(e) => Either::Right(e.to_string()),
    });
    if !errors.is_empty() {
        return Err(errors);
    }
    report::write(cli.format, &reports, &mut io::stdout().lock()).map_err(|e| vec![e.to_string()])?;

    let percent = Totals::of(&reports).percent();
    let passed = threshold.is_none_or(|threshold| percent >= threshold);
    if let Some(threshold) = threshold.filter(|_| !passed) {
        eprintln!("docov: {percent:.1}% documented, under the {threshold}% threshold");
    }
    Ok(passed)
}
