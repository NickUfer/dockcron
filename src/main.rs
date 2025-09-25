mod cli;
mod job;

use crate::cli::{Command, RunArgs};
use anyhow::{Result, anyhow};
use bollard::Docker;
use std::str::FromStr;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

#[derive(Debug, Clone, Copy)]
enum OverlapPolicy {
    Allow,
    Skip, // "no-overlap": skip tick if previous run still executing
}

#[derive(Debug, Clone)]
struct Label {
    key: String,
    value: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let cli = cli::Cli::parse();
    match cli.command {
        Command::Run(run_args) => {
            let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(1);
            let mut sigterm =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
            let mut sigint =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;

            tokio::spawn(async move {
                tokio::select! {
                    _ = sigterm.recv() => info!("received SIGTERM"),
                    _ = sigint.recv() => info!("received SIGINT"),
                }
                shutdown_tx.send(()).ok();
            });

            run(run_args, shutdown_rx).await
        }
    }
}

async fn run(args: RunArgs, mut shutdown: tokio::sync::broadcast::Receiver<()>) -> Result<()> {
    let docker = docker_client(args.docker_host)?;

    let container_label_selector = args.container_label_selector.map(|selector| {
        Label::from_str(selector.as_str())
            .map_err(|e| e.context("invalid container label selector"))
    });

    let container_label_selector = match container_label_selector {
        None => None,
        Some(label_result) => match label_result {
            Ok(label) => Some(label),
            Err(e) => return Err(e),
        },
    };

    let jobs = job::discover(&docker, container_label_selector, &args.label_prefixes).await?;
    if jobs.is_empty() {
        warn!("no jobs discovered; make sure labels are set and Docker is reachable");
        return Ok(());
    }

    info!(count = jobs.len(), "starting jobs");

    let mut handles: Vec<JoinHandle<()>> = Vec::new();
    for job in jobs {
        let docker = docker.clone();
        let mut shutdown_rx = shutdown.resubscribe();
        handles.push(tokio::spawn(async move {
            tokio::select! {
                res = job::run_loop(docker, job) => {
                    if let Err(e) = res {
                        error!(error = ?e, "job loop terminated with error");
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("job shutdown requested");
                }
            }
        }));
    }

    tokio::select! {
        _ = async {
            for handle in handles {
                let _ = handle.await; // errors already logged inside task
            }
        } => {}
        _ = shutdown.recv() => {
            info!("graceful shutdown initiated");
        }
    }

    Ok(())
}

fn docker_client(docker_host: String) -> Result<Docker> {
    if let Some(path) = docker_host.strip_prefix("unix://") {
        return Ok(Docker::connect_with_unix(
            path,
            60,
            bollard::API_DEFAULT_VERSION,
        )?);
    }
    if docker_host.starts_with("tcp://") {
        // Let bollard read TLS env vars (DOCKER_TLS_VERIFY, DOCKER_CERT_PATH):
        return Ok(Docker::connect_with_local_defaults()?);
    }
    Err(anyhow!("Unsupported DOCKER_HOST: {}", docker_host))
}

fn init_tracing() {
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into());
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .init();
}

impl FromStr for Label {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let mut split = s.splitn(2, '=');
        let key = split.next().ok_or_else(|| anyhow!("invalid key: {}", s))?;
        let value = split
            .next()
            .ok_or_else(|| anyhow!("invalid value: {}", s))?;
        Ok(Label {
            key: key.to_string(),
            value: value.to_string(),
        })
    }
}
