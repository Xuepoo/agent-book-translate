use clap::{Args, Parser, Subcommand};

use agent_book_translate::config::AppConfig;
use agent_book_translate::core::engine::{JobControl, run_with_progress_and_control};
use agent_book_translate::core::progress::{JobProgressReporter, TerminalProgressReporter};
use agent_book_translate::core::qa::run_epub_qa;
use agent_book_translate::error::{AppError, Result};
use agent_book_translate::job::control::{request_pause, request_resume, request_resume_force};
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
    Pause(JobIdArgs),
    Resume(ResumeArgs),
    Status(JobIdArgs),
    List,
    Logs(JobIdArgs),
    /// Run quality assurance checks on a generated EPUB file.
    Qa(QaArgs),
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

#[derive(Args, Debug, Clone, Default)]
struct ResumeArgs {
    #[arg(long)]
    job_id: String,

    #[arg(long)]
    config: Option<PathBuf>,

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

    #[arg(long)]
    max_spend_usd: Option<f64>,

    /// Force resume even if the job appears to be Running. Use after a crash
    /// or power loss where the process is known to be dead.
    #[arg(long, default_value_t = false)]
    force: bool,
}

#[derive(Debug, Clone, Default)]
struct LaunchOptions {
    config: Option<PathBuf>,
    api_key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    concurrency: Option<usize>,
    bilingual: bool,
    verbose: bool,
    max_spend_usd: Option<f64>,
}

impl From<&TranslateArgs> for LaunchOptions {
    fn from(value: &TranslateArgs) -> Self {
        Self {
            config: value.config.clone(),
            api_key: value.api_key.clone(),
            base_url: value.base_url.clone(),
            model: value.model.clone(),
            concurrency: value.concurrency,
            bilingual: value.bilingual,
            verbose: value.verbose,
            max_spend_usd: value.max_spend_usd,
        }
    }
}

impl From<&ResumeArgs> for LaunchOptions {
    fn from(value: &ResumeArgs) -> Self {
        Self {
            config: value.config.clone(),
            api_key: value.api_key.clone(),
            base_url: value.base_url.clone(),
            model: value.model.clone(),
            concurrency: value.concurrency,
            bilingual: value.bilingual,
            verbose: value.verbose,
            max_spend_usd: value.max_spend_usd,
        }
    }
}

#[derive(Args, Debug)]
struct JobIdArgs {
    job_id: String,
}

#[derive(Args, Debug)]
struct QaArgs {
    /// Path to the EPUB file to inspect.
    epub: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(CommandKind::Translate(args)) => translate(args).await,
        Some(CommandKind::Start(args)) => start(args),
        Some(CommandKind::Pause(args)) => pause(args),
        Some(CommandKind::Resume(args)) => resume(args),
        Some(CommandKind::Status(args)) => status(&args.job_id),
        Some(CommandKind::List) => list_jobs(),
        Some(CommandKind::Logs(args)) => logs(&args.job_id),
        Some(CommandKind::Qa(args)) => run_qa(args),
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
    let _state = load_or_create_job_state(&store, &job_id, input, output)?;
    println!("job_id: {job_id}");

    let terminal = TerminalProgressReporter::new();
    let job = JobProgressReporter::new(store.clone(), job_id.clone());
    let reporter = CombinedReporter { terminal, job };

    let result = run_with_progress_and_control(
        input,
        output,
        &config,
        &reporter,
        Some(JobControl {
            store: store.clone(),
            job_id: job_id.clone(),
        }),
    )
    .await;
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
    let job_id = args
        .job_id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let state = JobState::new(job_id.clone(), input.to_path_buf(), output.to_path_buf());
    store.save(&state)?;
    spawn_background_translate(&store, &job_id, input, output, &LaunchOptions::from(&args))?;
    Ok(())
}

fn pause(args: JobIdArgs) -> Result<()> {
    let store = JobStore::xdg()?;
    let state = request_pause(&store, &args.job_id)?;
    print_state(&state);
    Ok(())
}

fn resume(args: ResumeArgs) -> Result<()> {
    let store = JobStore::xdg()?;
    let state = if args.force {
        request_resume_force(&store, &args.job_id)?
    } else {
        request_resume(&store, &args.job_id)?
    };
    let options = LaunchOptions::from(&args);
    spawn_background_translate(
        &store,
        &args.job_id,
        state.input.as_path(),
        state.output.as_path(),
        &options,
    )?;
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

fn load_or_create_job_state(
    store: &JobStore,
    job_id: &str,
    input: &Path,
    output: &Path,
) -> Result<JobState> {
    let path = store.path_for(job_id);
    if path.exists() {
        let state = store.load(job_id)?;
        if state.input != input || state.output != output {
            return Err(AppError::Config(format!(
                "job state paths do not match requested paths: {job_id}"
            )));
        }
        if state.status == JobStatus::Completed {
            return Err(AppError::Config(format!("job already completed: {job_id}")));
        }
        return Ok(state);
    }

    let state = JobState::new(
        job_id.to_string(),
        input.to_path_buf(),
        output.to_path_buf(),
    );
    store.save(&state)?;
    Ok(state)
}

fn spawn_background_translate(
    store: &JobStore,
    job_id: &str,
    input: &Path,
    output: &Path,
    options: &LaunchOptions,
) -> Result<()> {
    store.ensure_log_dir()?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(store.log_path(job_id))?;
    let err_log = log.try_clone()?;

    let exe = env::current_exe()?;
    let mut command = Command::new(exe);
    command
        .arg("translate")
        .arg("--job-id")
        .arg(job_id)
        .arg("--input")
        .arg(input)
        .arg("--output")
        .arg(output);
    append_launch_options(&mut command, options);

    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(err_log));
    if let Some(xdg_state_home) = env::var_os("XDG_STATE_HOME") {
        command.env("XDG_STATE_HOME", xdg_state_home);
    }
    command.spawn()?;
    println!("job_id: {job_id}");
    println!("status: {}", store.path_for(job_id).display());
    println!("log: {}", store.log_path(job_id).display());
    Ok(())
}

fn append_launch_options(command: &mut Command, options: &LaunchOptions) {
    append_optional_path(command, "--config", options.config.as_deref());
    append_optional(command, "--api-key", options.api_key.as_deref());
    append_optional(command, "--base-url", options.base_url.as_deref());
    append_optional(command, "--model", options.model.as_deref());
    if let Some(concurrency) = options.concurrency {
        command.arg("--concurrency").arg(concurrency.to_string());
    }
    if options.bilingual {
        command.arg("--bilingual");
    }
    if options.verbose {
        command.arg("--verbose");
    }
    if let Some(max_spend_usd) = options.max_spend_usd {
        command
            .arg("--max-spend-usd")
            .arg(max_spend_usd.to_string());
    }
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
fn run_qa(args: QaArgs) -> Result<()> {
    let report = run_epub_qa(&args.epub)?;
    report.print_summary();
    if report.passed() {
        Ok(())
    } else {
        Err(AppError::Config("EPUB QA checks failed".to_string()))
    }
}
