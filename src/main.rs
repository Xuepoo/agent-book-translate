use clap::Parser;

use agent_book_translate::config::AppConfig;
use agent_book_translate::core::engine::run;
use agent_book_translate::error::Result;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "agent-book-translate", version, about)]
struct Args {
    #[arg(short, long)]
    input: PathBuf,

    #[arg(short, long)]
    output: PathBuf,

    #[arg(long)]
    config: Option<PathBuf>,

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

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let mut config = AppConfig::load_from_path(args.config.as_deref())?;

    if let Some(value) = args.api_key {
        config.api_key = value;
    }
    if let Some(value) = args.base_url {
        config.base_url = value;
    }
    if let Some(value) = args.model {
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

    let _ = args.language;
    let _ = args.verbose;
    let _ = args.series;
    let _ = args.resume;

    run(&args.input, &args.output, &config).await
}
