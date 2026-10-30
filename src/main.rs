//! The `docov` command.

use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{CommandFactory, FromArgMatches, Parser};
use docov::{
    Config, Format, Settings, Threshold, batch,
    feature::{
        self,
        color::{Color, Styled},
        jsdoc_config::JsdocConfig,
        list_files::{self, ListFiles},
        print_config::{self, PrintConfig},
        stdin_filename::{self, StdinFilename},
        threads::Threads,
        watch,
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
        return ExitCode::from(exit(init::init(Path::new(".")).map(|threshold| {
            eprintln!("docov: wrote docov.json, with a {threshold}% threshold");
            true
        })));
    }
    ExitCode::from(watch::run(watch::enabled(&matches), &mut |watched| exit(run(&cli, &matches, watched))))
}

/// Prints `result`'s errors, and exits by it: 0 when coverage passed, 1 when it didn't, and 2 on
/// an error.
fn exit(result: Result<bool, Vec<String>>) -> u8 {
    match result {
        Ok(passed) => u8::from(!passed),
        Err(errors) => {
            for error in errors {
                eprintln!("docov: {error}");
            }
            2
        }
    }
}

/// Scans and reports, and says whether coverage met the threshold. `watched` gets the paths it
/// scans, for `--watch`.
fn run(cli: &Cli, matches: &clap::ArgMatches, watched: &mut Vec<PathBuf>) -> Result<bool, Vec<String>> {
    // The current folder, unless there are paths: no paths scan it, and a config that doesn't load
    // has none.
    *watched = vec![PathBuf::from(".")];
    let mut config = Config::load(cli.config.as_deref()).map_err(|e| vec![e.to_string()])?;
    let settings = Settings::new(matches, std::mem::take(&mut config.keys));
    config.read_jsdoc(settings.get::<JsdocConfig>().0.as_deref()).map_err(|e| vec![e])?;
    let paths = if cli.paths.is_empty() { std::mem::take(&mut config.paths) } else { cli.paths.clone() };
    if !paths.is_empty() {
        watched.clone_from(&paths);
    }
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
    let (settings, text) = (plan.settings(), cli.format == Format::Text);
    let stream = &mut settings.get::<Color>().stream(io::stdout().lock());
    let out = &mut Styled::new(stream, text, settings.get::<Threshold>().0);
    packages::write(settings, &config.dir, cli.format, &reports, out).map_err(|e| vec![e])?;

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
