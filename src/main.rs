//! The `docov` command.

use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{CommandFactory, FromArgMatches, Parser};
use docov::{
    Config, Format, Settings, batch,
    feature::{
        self,
        jsdoc_config::JsdocConfig,
        list_files::{self, ListFiles},
        print_config::{self, PrintConfig},
        stdin_filename::{self, StdinFilename},
        threads::Threads,
    },
    init, nested, packages, walk,
};

/// Checks how much of a JavaScript or TypeScript project's API has JSDoc.
///
/// Exits 1 when coverage is under the threshold, 2 when a file or the config can't be read.
#[derive(Parser)]
// No `help` subcommand: `docov help` would no longer scan a folder named help.
#[command(version, args_conflicts_with_subcommands = true, disable_help_subcommand = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
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

/// What docov does besides scanning.
#[derive(clap::Subcommand)]
enum Command {
    /// Write a docov.json here with a threshold of the current coverage, rounded down
    Init,
}

fn main() -> ExitCode {
    // Each feature adds its own flags.
    let matches = feature::args(Cli::command()).get_matches();
    let cli = Cli::from_arg_matches(&matches).expect("the flags are the command's");
    if cli.print_schema {
        print!("{}", Config::json_schema());
        return ExitCode::SUCCESS;
    }
    if let Some(Command::Init) = cli.command {
        return exit(init::init(Path::new(".")).map(|threshold| {
            eprintln!("docov: wrote docov.json, with a {threshold}% threshold");
            true
        }));
    }
    exit(run(cli, &matches))
}

/// Prints `result`'s errors, and exits by it: 0 when coverage passed, 1 when it didn't, and 2 on
/// an error.
fn exit(result: Result<bool, Vec<String>>) -> ExitCode {
    match result {
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
    config.read_jsdoc(settings.get::<JsdocConfig>().0.as_deref()).map_err(|e| vec![e])?;
    let paths = if cli.paths.is_empty() { std::mem::take(&mut config.paths) } else { cli.paths };
    if settings.get::<PrintConfig>().0 {
        return wrote(writeln!(io::stdout(), "{:#}", print_config::json(&config, &paths, &settings)));
    }
    let stdin = settings.get::<StdinFilename>().0.clone();
    let files = match &stdin {
        Some(path) => vec![path.clone()],
        None => walk::files(&paths, &config.exclude).map_err(|e| vec![e.to_string()])?,
    };
    let plan = nested::Plan::new(&config, settings, matches, files).map_err(|e| vec![e])?;
    let files = plan.files();
    if plan.settings().get::<ListFiles>().0 {
        return wrote(list_files::write(files.iter().map(|(file, _)| *file), &mut io::stdout().lock()));
    }
    plan.settings().get::<Threads>().apply();
    let reports = match stdin {
        Some(_) => stdin_filename::scan(&files, &mut io::stdin().lock())?,
        None => batch::scan(&files)?,
    };
    let out = &mut io::stdout().lock();
    packages::write(plan.settings(), &config.dir, cli.format, &reports, out).map_err(|e| vec![e])?;

    let failures = plan.gates(&reports);
    for failure in &failures {
        eprintln!("docov: {failure}");
    }
    Ok(failures.is_empty())
}

/// A write's result as `run`'s: passed, or why not.
fn wrote(result: io::Result<()>) -> Result<bool, Vec<String>> {
    result.map(|()| true).map_err(|e| vec![e.to_string()])
}
