use std::fs;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use metactl::{JsonRpcService, McpService, ReferenceKernel};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
struct Args {
    #[arg(long)]
    fixtures_dir: Option<PathBuf>,
    #[arg(long = "library-root")]
    library_roots: Vec<PathBuf>,
    #[arg(long, default_value_t = false)]
    stdio: bool,
    #[arg(long, default_value_t = false)]
    mcp: bool,
    #[arg(long)]
    once: Option<PathBuf>,
}

fn main() -> Result<()> {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("metactld=info,metactl=info"));
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .init();

    let args = Args::parse();
    let kernel = if !args.library_roots.is_empty() {
        ReferenceKernel::load_from_library_roots(args.library_roots.clone())
            .context("load library roots")?
    } else {
        let fixtures_dir = args
            .fixtures_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("fixtures/golden"));
        ReferenceKernel::load_from_dir(&fixtures_dir)
            .with_context(|| format!("load fixtures from {}", fixtures_dir.display()))?
    };
    if args.mcp {
        let service = McpService::new(kernel);

        if let Some(path) = args.once {
            let raw =
                fs::read(&path).with_context(|| format!("read request {}", path.display()))?;
            if let Some(response) = service.dispatch_bytes(&raw)? {
                println!("{}", String::from_utf8(response)?);
            }
            return Ok(());
        }

        if args.stdio {
            let stdin = io::stdin();
            let mut stdout = io::stdout();
            for line in stdin.lock().lines() {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }
                if let Some(response) = service.dispatch_bytes(line.as_bytes())? {
                    stdout.write_all(&response)?;
                    stdout.write_all(b"\n")?;
                    stdout.flush()?;
                }
            }
        }

        return Ok(());
    }

    let service = JsonRpcService::new(kernel);

    if let Some(path) = args.once {
        let raw = fs::read(&path).with_context(|| format!("read request {}", path.display()))?;
        let response = service.dispatch_bytes(&raw)?;
        println!("{}", String::from_utf8(response)?);
        return Ok(());
    }

    if args.stdio {
        let stdin = io::stdin();
        let mut stdout = io::stdout();
        for line in stdin.lock().lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let response = service.dispatch_bytes(line.as_bytes())?;
            stdout.write_all(&response)?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
        }
    }

    Ok(())
}
