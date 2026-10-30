//! The `docov` command.

use std::{
    env,
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    time::SystemTime,
};

use clap::{CommandFactory, FromArgMatches, Parser};
use docov::{
    Config, Settings, Threshold, Totals, batch,
    feature::{
        self,
        baseline::{self, Baseline},
        by_package::ByPackage,
        cache::CacheFile,
        changed::Changed,
        color::Color,
        diff::Diff,
        examples::Examples,
        explain::Explain,
        jsdoc_config::JsdocConfig,
        list_files::{self, ListFiles},
        overrides::Overrides,
        print_config::{self, PrintConfig},
        sort::Sort,
        stdin_filename::{self, StdinFilename},
        threads::Threads,
        threshold::Limit,
        update_threshold::UpdateThreshold,
        watch,
    },
    init, nested, report, walk,
};

/// Checks how much of a JavaScript or TypeScript project's API has JSDoc.
///
/// Exits 1 when a gate fails (a threshold, say), 2 when a file or the config can't be read.
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
    #[command(flatten)]
    report: report::Flags,
    /// Print the config file's JSON Schema, or the baseline file's, and exit
    #[arg(long, value_enum, value_name = "FILE", exclusive = true, num_args = 0..=1, default_missing_value = "config")]
    print_schema: Option<Schema>,
}

/// What docov does besides scanning.
#[derive(clap::Subcommand)]
enum Command {
    /// Write a docov.json here with a threshold of the current coverage, rounded down
    Init,
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
    if let Some(Command::Init) = cli.command {
        return ExitCode::from(exit(init::init(Path::new(".")).map(|threshold| {
            eprintln!("docov: wrote docov.json, with a {threshold}% threshold");
            true
        })));
    }
    ExitCode::from(watch::run(watch::enabled(&matches), &mut |watched| exit(run(&cli, &matches, watched))))
}

/// Prints `result`'s errors, and exits by it: 0 when every gate passed, 1 when one didn't, and 2
/// on an error.
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

/// Scans and reports, and says whether every gate passed. `watched` gets the paths it scans, for
/// `--watch`.
fn run(cli: &Cli, matches: &clap::ArgMatches, watched: &mut Vec<PathBuf>) -> Result<bool, Vec<String>> {
    // The current folder, unless there are paths: no paths scan it, and a config that doesn't load
    // has none.
    *watched = vec![PathBuf::from(".")];
    // GitHub Actions sets it, and reads annotations from the output.
    let github_actions = env::var_os("GITHUB_ACTIONS").is_some_and(|value| value == "true");
    let outputs = cli.report.outputs(github_actions).map_err(|e| vec![e])?;
    let base = cli.report.compare.as_deref().map(report::Base::read).transpose().map_err(|e| vec![e])?;
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

    let root = config.root();
    let stdin = settings.get::<StdinFilename>().0.clone();
    let explain = settings.get::<Explain>().0.clone();
    let files = match stdin.as_ref().or(explain.as_ref()) {
        Some(path) => vec![path.clone()],
        None => {
            let excluded = settings.get::<Overrides>().clone().excluder(config.exclude.clone(), &root);
            let files = walk::files(&paths, excluded).map_err(|e| vec![e.to_string()])?;
            settings.get::<Changed>().filter(files, &root).map_err(|e| vec![e])?
        }
    };
    let mut plan = nested::Plan::new(&config, settings, matches, files).map_err(|e| vec![e])?;
    plan.load_public(&paths)?;
    let (settings, files) = (plan.settings(), plan.files());
    if let Some(explained) = settings.get::<Explain>().write(&files, &mut io::stdout().lock()) {
        return explained.map(|()| true).map_err(|e| vec![e]);
    }
    if settings.get::<ListFiles>().0 {
        return wrote(list_files::write(files.iter().map(|(file, _)| *file), &mut io::stdout().lock()));
    }
    let added = settings.get::<Diff>().added(&root).map_err(|e| vec![e])?;
    settings.get::<Threads>().apply();
    let reports = match stdin {
        Some(_) => stdin_filename::scan(&files, &mut io::stdin().lock())?,
        None => batch::scan(&files, &settings.get::<CacheFile>().open(&config.dir))?,
    };
    if let Some(edited) = settings.edit(&reports) {
        eprintln!("docov: {}", edited.map_err(|e| vec![e])?);
        return Ok(true);
    }

    // Writing a baseline exits 0, so no gate runs. The gates let the baseline's items through,
    // which the report still lists.
    let baseline = settings.get::<Baseline>();
    let failures = if baseline.write {
        Vec::new()
    } else {
        let (gated, mut failures) = plan.gates(&reports);
        failures.extend(settings.get::<Diff>().gate(added.as_ref(), &gated, &root));
        failures
    };
    let packages = settings.get::<ByPackage>().packages(&config.dir, &reports).map_err(|e| vec![e])?;
    // Badges and colors go by the global percentage.
    let threshold = match settings.get::<Threshold>().0.global {
        Some(Limit::Percent(percent)) => Some(percent),
        _ => None,
    };
    let color = *settings.get::<Color>();
    let context = report::Context {
        files: &reports,
        failures: &failures,
        now: SystemTime::now(),
        threshold,
        text: cli.report.text,
        base: base.as_ref(),
        packages: packages.as_deref(),
        color: color != Color::Never,
        sort: *settings.get::<Sort>(),
        examples: settings.get::<Examples>().text,
        metrics: &settings.metrics(),
    };
    report::write(&outputs, &context, &mut color.stream(io::stdout().lock())).map_err(|e| vec![e])?;
    for (file, diagnostic) in report::diagnostics(&reports) {
        eprintln!("docov: {}:{diagnostic}", file.path.display());
    }
    if baseline.write {
        for path in plan.save_baselines(&reports).map_err(|e| vec![e])? {
            eprintln!("docov: wrote the undocumented items to {}", path.display());
        }
        return Ok(true);
    }
    // The root config's threshold is for its own files.
    let own = Totals::of(&plan.groups[0].members(&reports));
    let raised = settings.get::<UpdateThreshold>().update(config.path(), own);
    if let Some(raised) = raised.map_err(|e| vec![e])? {
        eprintln!("docov: {raised}");
    }
    for failure in &failures {
        eprintln!("docov: {failure}");
    }
    Ok(failures.is_empty())
}

/// A write's result as `run`'s: passed, or why not.
fn wrote(result: io::Result<()>) -> Result<bool, Vec<String>> {
    result.map(|()| true).map_err(|e| vec![e.to_string()])
}
