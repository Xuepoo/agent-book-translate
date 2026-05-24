use clap::{Args, Parser, Subcommand};

use agent_book_translate::config::AppConfig;
use agent_book_translate::core::engine::run_with_progress;
use agent_book_translate::core::progress::{JobProgressReporter, TerminalProgressReporter};
use agent_book_translate::error::{AppError, Result};
use agent_book_translate::job::{JobState, JobStatus, JobStore};
use std::env;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use uuid::Uuid;

#[derive(Parser, Debug)]
#[command(name = "agent-book-translate", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<CommandKind>,

    #[command(flatten)]
    translate: TranslateArgs,
}

#[derive(Subcommand, Debug)]
enum CommandKind {
    Translate(TranslateArgs),
    Start(TranslateArgs),
    Status(JobIdArgs),
    List,
    Logs(JobIdArgs),
}

#[derive(Args, Debug, Clone, Default)]
struct TranslateArgs {
    #[arg(short, long)]
    input: Option<PathBuf>,

    #[arg(short, long)]
    output: Option<PathBuf>,

    #[arg(long)]
    config: Option<PathBuf>,

    #[arg(long)]
    job_id: Option<String>,

    #[arg(short = 'l', long)]
    language: Option<String>,

    #[arg(short = 'k', long)]
    api_key: Option<String>,

    #[arg(short = 'u', long)]
    base_url: Option<String>,

    #[arg(short = 'm', long)]
    model: Option<String>,

    #[arg(short = 'c', long)]
    concurrency: Option<usize>,

    #[arg(long, default_value_t = false)]
    bilingual: bool,

    #[arg(short = 'v', long, default_value_t = false)]
    verbose: bool,

    #[arg(short = 's', long)]
    series: Option<String>,

    #[arg(short = 'r', long, default_value_t = true)]
    resume: bool,

    #[arg(long)]
    max_spend_usd: Option<f64>,
}

#[derive(Args, Debug)]
struct JobIdArgs {
    job_id: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(CommandKind::Translate(args)) => translate(args).await,
        Some(CommandKind::Start(args)) => start(args),
        Some(CommandKind::Status(args)) => status(&args.job_id),
        Some(CommandKind::List) => list_jobs(),
        Some(CommandKind::Logs(args)) => logs(&args.job_id),
        None => translate(cli.translate).await,
    }
}

async fn translate(args: TranslateArgs) -> Result<()> {
    let input = required_path(args.input.as_deref(), "input")?;
    let output = required_path(args.output.as_deref(), "output")?;
    let mut config = load_config(&args)?;
    apply_cli_overrides(&mut config, &args);

    let store = JobStore::xdg()?;
    let job_id = args.job_id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let state = JobState::new(job_id.clone(), input.to_path_buf(), output.to_path_buf());
    store.save(&state)?;
    println!("job_id: {job_id}");

    let terminal = TerminalProgressReporter::new();
    let job = JobProgressReporter::new(store.clone(), job_id.clone());
    let reporter = CombinedReporter { terminal, job };

    let result = run_with_progress(input, output, &config, &reporter).await;
    if let Err(error) = &result {
        let mut state = store.load(&job_id)?;
        state.status = JobStatus::Failed;
        state.last_error = Some(error.to_string());
        store.save(&state)?;
    }
    result
}

fn start(args: TranslateArgs) -> Result<()> {
    let input = required_path(args.input.as_deref(), "input")?;
    let output = required_path(args.output.as_deref(), "output")?;
    let store = JobStore::xdg()?;
    store.ensure_log_dir()?;
    let job_id = args
        .job_id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let state = JobState::new(job_id.clone(), input.to_path_buf(), output.to_path_buf());
    store.save(&state)?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(store.log_path(&job_id))?;
    let err_log = log.try_clone()?;

    let exe = env::current_exe()?;
    let mut command = Command::new(exe);
    command
        .arg("translate")
        .arg("--job-id")
        .arg(&job_id)
        .arg("--input")
        .arg(input)
        .arg("--output")
        .arg(output);
    append_optional_path(&mut command, "--config", args.config.as_deref());
    append_optional(&mut command, "--api-key", args.api_key.as_deref());
    append_optional(&mut command, "--base-url", args.base_url.as_deref());
    append_optional(&mut command, "--model", args.model.as_deref());
    if let Some(concurrency) = args.concurrency {
        command.arg("--concurrency").arg(concurrency.to_string());
    }
    if args.bilingual {
        command.arg("--bilingual");
    }
    if args.verbose {
        command.arg("--verbose");
    }
    if let Some(max_spend_usd) = args.max_spend_usd {
        command
            .arg("--max-spend-usd")
            .arg(max_spend_usd.to_string());
    }

    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(err_log));
    command.spawn()?;
    println!("job_id: {job_id}");
    println!("status: {}", store.path_for(&job_id).display());
    println!("log: {}", store.log_path(&job_id).display());
    Ok(())
}

fn status(job_id: &str) -> Result<()> {
    let store = JobStore::xdg()?;
    let state = store.load(job_id)?;
    print_state(&state);
    Ok(())
}

fn list_jobs() -> Result<()> {
    let store = JobStore::xdg()?;
    for state in store.list()? {
        println!(
            "{}\t{:?}\t{}/{} chunks\t{} tokens\t{}s",
            state.job_id,
            state.status,
            state.metrics.completed_chunks,
            state.metrics.total_chunks,
            state.metrics.total_tokens,
            state.elapsed_seconds()
        );
    }
    Ok(())
}

fn logs(job_id: &str) -> Result<()> {
    let store = JobStore::xdg()?;
    let path = store.log_path(job_id);
    File::open(&path)?;
    println!("{}", path.display());
    Ok(())
}

fn required_path<'a>(path: Option<&'a Path>, name: &str) -> Result<&'a Path> {
    path.ok_or_else(|| AppError::Config(format!("missing required --{name}")))
}

fn load_config(args: &TranslateArgs) -> Result<AppConfig> {
    AppConfig::load_from_path(args.config.as_deref())
}

fn apply_cli_overrides(config: &mut AppConfig, args: &TranslateArgs) {
    if let Some(value) = args.api_key.clone() {
        config.api_key = value;
    }
    if let Some(value) = args.base_url.clone() {
        config.base_url = value;
    }
    if let Some(value) = args.model.clone() {
        config.default_model = value;
    }
    if let Some(value) = args.concurrency {
        config.concurrency = value;
    }
    if args.bilingual {
        config.bilingual = true;
    }
    if let Some(value) = args.max_spend_usd {
        config.max_spend_usd = Some(value);
    }

    let _ = &args.language;
    let _ = args.verbose;
    let _ = &args.series;
    let _ = args.resume;
}

fn append_optional(command: &mut Command, flag: &str, value: Option<&str>) {
    if let Some(value) = value {
        command.arg(flag).arg(value);
    }
}

fn append_optional_path(command: &mut Command, flag: &str, value: Option<&Path>) {
    if let Some(value) = value {
        command.arg(flag).arg(value);
    }
}

fn print_state(state: &JobState) {
    println!("job_id: {}", state.job_id);
    println!("status: {:?}", state.status);
    println!("input: {}", state.input.display());
    println!("output: {}", state.output.display());
    println!(
        "current_file: {}",
        state.current_file.as_deref().unwrap_or("-")
    );
    println!(
        "chunks: {}/{}",
        state.metrics.completed_chunks, state.metrics.total_chunks
    );
    println!(
        "text_files: {}/{}",
        state.metrics.completed_text_files, state.metrics.total_text_files
    );
    println!("requests: {}", state.metrics.request_count);
    println!("retries: {}", state.metrics.retry_count);
    println!(
        "tokens: total={} prompt={} completion={}",
        state.metrics.total_tokens, state.metrics.prompt_tokens, state.metrics.completion_tokens
    );
    println!("elapsed_seconds: {}", state.elapsed_seconds());
    if let Some(error) = &state.last_error {
        println!("last_error: {error}");
    }
}

struct CombinedReporter {
    terminal: TerminalProgressReporter,
    job: JobProgressReporter,
}

impl agent_book_translate::core::progress::ProgressReporter for CombinedReporter {
    fn on_event(&self, event: agent_book_translate::core::progress::ProgressEvent) {
        self.terminal.on_event(event.clone());
        self.job.on_event(event);
    }
}
