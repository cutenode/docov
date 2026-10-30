//! The `docov` command.

use std::{
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};

use clap::{CommandFactory, FromArgMatches, Parser};
use docov::{
    Config, Format, Scope, Settings,
    feature::{
        self,
        list_files::{self, ListFiles},
        print_config::{self, PrintConfig},
    },
    report, scan_file, walk,
};
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
    /// How to write the report
    #[arg(long, value_enum, default_value_t)]
    format: Format,
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
    let mut config = Config::load(cli.config.as_deref()).map_err(|e| vec![e.to_string()])?;
    let settings = Settings::new(matches, std::mem::take(&mut config.keys));
    let paths = if cli.paths.is_empty() { std::mem::take(&mut config.paths) } else { cli.paths };
    if settings.get::<PrintConfig>().0 {
        return wrote(writeln!(io::stdout(), "{:#}", print_config::json(&config, &paths, &settings)));
    }
    let scope = *settings.get::<Scope>();

    let files = walk::files(&paths, &config.exclude).map_err(|e| vec![e.to_string()])?;
    if settings.get::<ListFiles>().0 {
        return wrote(list_files::write(&files, &mut io::stdout().lock()));
    }
    let (reports, errors): (Vec<_>, Vec<_>) = files.par_iter().partition_map(|path| match scan_file(path, scope) {
        Ok(report) => Either::Left(report),
        Err(e) => Either::Right(e.to_string()),
    });
    if !errors.is_empty() {
        return Err(errors);
    }
    report::write(cli.format, &reports, &mut io::stdout().lock()).map_err(|e| vec![e.to_string()])?;

    let failures = settings.gates(&reports);
    for failure in &failures {
        eprintln!("docov: {failure}");
    }
    Ok(failures.is_empty())
}

/// A write's result as `run`'s: passed, or why not.
fn wrote(result: io::Result<()>) -> Result<bool, Vec<String>> {
    result.map(|()| true).map_err(|e| vec![e.to_string()])
}
