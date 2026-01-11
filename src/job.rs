use crate::{Label, OverlapPolicy};
use anyhow::{Context, anyhow};
use bollard::Docker;
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::query_parameters::{InspectContainerOptions, ListContainersOptions};
use chrono::{DateTime, Utc};
use cron::Schedule;
use futures::StreamExt;
use regex::Regex;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::time::sleep_until;
use tracing::{error, info, warn};

#[derive(Debug)]
pub struct Job {
    pub container_id: String,
    pub container_name: String,
    pub name: String,
    pub schedule: JobSchedule,
    pub command: String,
    pub overlap: OverlapPolicy,
    pub gate: Semaphore, // 1-permit semaphore to guard overlap
}

#[derive(Debug, Clone)]
pub enum JobSchedule {
    Every(Duration),
    // Boxed to avoid large enum size and clippy::large_enum_variants warnings
    Cron(Box<Schedule>),
}

const JOB_SCHEDULE_EVERY_DEFINITION_PREFIX: &str = "@every ";
const JOB_SCHEDULE_CRON_DEFINITION_PREFIX: &str = "@cron ";

impl Display for JobSchedule {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            JobSchedule::Every(d) => f.write_fmt(format_args!(
                "{}{}",
                JOB_SCHEDULE_EVERY_DEFINITION_PREFIX,
                humantime::format_duration(*d)
            )),
            JobSchedule::Cron(s) => {
                f.write_fmt(format_args!("{}{}", JOB_SCHEDULE_CRON_DEFINITION_PREFIX, s))
            }
        }
    }
}

impl FromStr for JobSchedule {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if let Some(rest) = s.strip_prefix(JOB_SCHEDULE_EVERY_DEFINITION_PREFIX) {
            let dur = humantime::parse_duration(rest)?;
            Ok(JobSchedule::Every(dur))
        } else if let Some(schedule) = s.strip_prefix(JOB_SCHEDULE_CRON_DEFINITION_PREFIX) {
            let schedule = Schedule::from_str(schedule).context("could not parse cron schedule")?;
            Ok(JobSchedule::Cron(Box::new(schedule)))
        } else {
            // Simple macros
            let dur = match s {
                "@hourly" => Duration::from_secs(3600),
                "@daily" | "@every 24h" => Duration::from_secs(24 * 3600),
                "@weekly" => Duration::from_secs(7 * 24 * 3600),
                "@monthly" => Duration::from_secs(30 * 24 * 3600),
                _ => {
                    // Try parsing as raw cron expression (for compatibility with ofelia)
                    if let Ok(schedule) = Schedule::from_str(s) {
                        return Ok(JobSchedule::Cron(Box::new(schedule)));
                    }
                    return Err(anyhow!("unsupported schedule: {}", s));
                }
            };
            Ok(JobSchedule::Every(dur))
        }
    }
}

pub async fn discover(
    docker: &Docker,
    container_filter_label: Option<Label>,
    prefixes: &[String],
) -> anyhow::Result<Vec<Job>> {
    let containers = docker
        .list_containers(Some(ListContainersOptions {
            all: true,
            ..Default::default()
        }))
        .await
        .context("list containers")?;

    let mut jobs = Vec::new();

    for c in containers {
        let labels = c.labels.clone().unwrap_or_default();

        // Check required container-level labels/values (AND)
        if let Some(container_filter_label) = &container_filter_label {
            let passes_filter = labels.iter().any(|(k, v)| {
                *k == container_filter_label.key && *v == container_filter_label.value
            });

            if !passes_filter {
                continue;
            }
        }

        let container_id = c.id.clone().unwrap_or_default();
        let mut found_prefix = None;

        for prefix in prefixes {
            let enabled_key = format!("{}.enabled", prefix);
            if labels
                .get(&enabled_key)
                .map(|s| s == "true")
                .unwrap_or(false)
            {
                found_prefix = Some(prefix);
                break;
            }
        }

        let Some(prefix) = found_prefix else {
            continue;
        };
        let container_name = c
            .names
            .as_ref()
            .and_then(|v| v.first())
            .cloned()
            .unwrap_or_else(|| container_id.chars().take(12).collect());

        // Group labels by job name
        let re = Regex::new(&format!(
            r"^{}{}([^.]+)\.(schedule|command|no-overlap)$",
            regex::escape(prefix),
            regex::escape(".job-exec.")
        ))
        .expect("valid regex");

        #[derive(Default)]
        struct PartialJobConfig {
            schedule: Option<String>,
            command: Option<String>,
            no_overlap: Option<String>,
        }

        let mut by_job: HashMap<String, PartialJobConfig> = HashMap::new();

        for (k, v) in &labels {
            if let Some(caps) = re.captures(k) {
                let job = caps[1].to_string();
                let kind = &caps[2];
                let acc = by_job.entry(job).or_default();
                match kind {
                    "schedule" => acc.schedule = Some(v.clone()),
                    "command" => acc.command = Some(v.clone()),
                    "no-overlap" => acc.no_overlap = Some(v.clone()),
                    _ => {}
                }
            }
        }

        for (jobname, acc) in by_job {
            let (schedule_opt, command_opt, no_overlap_opt) =
                (acc.schedule, acc.command, acc.no_overlap);
            let schedule_str = match schedule_opt {
                Some(s) => s,
                None => {
                    warn!(container=%container_name, job=%jobname, "missing schedule label");
                    continue;
                }
            };
            let command = match command_opt {
                Some(s) => s,
                None => {
                    warn!(container=%container_name, job=%jobname, "missing command label");
                    continue;
                }
            };

            let schedule = JobSchedule::from_str(&schedule_str)
                .with_context(|| format!("parse schedule '{}'", schedule_str))?;

            let overlap = match no_overlap_opt.as_deref().map(|s| s.trim()) {
                Some("true") => OverlapPolicy::Skip,
                _ => OverlapPolicy::Allow,
            };

            jobs.push(Job {
                container_id: container_id.clone(),
                container_name: container_name.clone(),
                name: jobname,
                schedule,
                command,
                overlap,
                gate: Semaphore::new(1),
            });
        }
    }

    Ok(jobs)
}

pub async fn run_loop(docker: Docker, job: Job) -> anyhow::Result<()> {
    info!(
        container = %job.container_name,
        job = %job.name,
        schedule = %job.schedule,
        overlap_policy = ?job.overlap,
        "job started"
    );
    let docker = Arc::new(docker);
    let job = Arc::new(job);

    match job.schedule.clone() {
        JobSchedule::Every(repeat_duration) => {
            let mut execution_interval = tokio::time::interval(repeat_duration);
            execution_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Burst);

            // Mirror ofelia logic: wait one tick before starting the first execution
            execution_interval.tick().await;

            loop {
                execution_interval.tick().await;
                match job.overlap {
                    OverlapPolicy::Allow => run_once_async(docker.clone(), job.clone()).await,
                    OverlapPolicy::Skip => {
                        if let Ok(permit) = job.gate.try_acquire() {
                            run_once_async(docker.clone(), job.clone()).await;
                            drop(permit);
                        } else {
                            info!(container=%job.container_name, job=%job.name, "skipping tick (policy={:?}: previous run still in progress)", job.overlap);
                        }
                    }
                }
            }
        }
        JobSchedule::Cron(schedule) => {
            let mut next = next_instant(*schedule.clone())?;
            loop {
                sleep_until(next).await;

                match job.overlap {
                    OverlapPolicy::Allow => run_once_async(docker.clone(), job.clone()).await,
                    OverlapPolicy::Skip => {
                        if let Ok(permit) = job.gate.try_acquire() {
                            run_once_async(docker.clone(), job.clone()).await;
                            drop(permit);
                        } else {
                            info!(container=%job.container_name, job=%job.name, "skipping tick (no-overlap: previous run still in progress)");
                        }
                    }
                }

                next = next_instant(*schedule.clone())?;
            }
        }
    }
}

async fn run_once_async(docker: Arc<Docker>, job: Arc<Job>) {
    let container_name = job.container_name.clone();
    let job_name = job.name.clone();

    let spawn_result = tokio::spawn(async move {
        if let Err(e) = run_once(docker, job.clone()).await {
            error!(container=%job.container_name, job=%job.name, error=?e, "execution failed");
        }
    })
    .await;

    if let Err(e) = spawn_result {
        error!(container=%container_name, job=%job_name, error=?e, "could not spawn execution task");
    }
}

async fn run_once(docker: Arc<Docker>, job: Arc<Job>) -> anyhow::Result<()> {
    info!(container=%job.container_name, job=%job.name, "exec starting");

    // Check if container is running
    match docker
        .inspect_container(&job.container_id, None::<InspectContainerOptions>)
        .await
    {
        Ok(details) => {
            let running = details
                .state
                .as_ref()
                .and_then(|s| s.running)
                .unwrap_or(false);
            if !running {
                warn!(container=%job.container_name, job=%job.name, "container is not running; skipping exec");
                return Ok(());
            }
        }
        Err(e) => {
            warn!(container=%job.container_name, job=%job.name, error=?e, "failed to inspect container; skipping exec");
            return Ok(());
        }
    }

    let cmd = match shlex::split(job.command.as_str()) {
        Some(args) if !args.is_empty() => args,
        _ => {
            warn!(container=%job.container_name, job=%job.name, "exec command is empty or erroneous");
            return Ok(());
        }
    };

    let exec = docker
        .create_exec(
            &job.container_id,
            CreateExecOptions {
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                tty: Some(false),
                cmd: Some(cmd),
                env: None,
                ..Default::default()
            },
        )
        .await?
        .id;

    match docker.start_exec(&exec, None).await? {
        StartExecResults::Attached { mut output, .. } => {
            while let Some(Ok(msg)) = output.next().await {
                use bollard::container::LogOutput::{StdErr, StdOut};

                let (message, channel) = match msg {
                    StdErr { message } => (message, "stderr"),
                    StdOut { message } => (message, "stdout"),
                    _ => {
                        unreachable!()
                    }
                };
                let line = String::from_utf8_lossy(&message).trim().to_string();
                if !line.is_empty() {
                    info!(container=%job.container_name, job=%job.name, channel=%channel, line=%line, "command log");
                }
            }
        }
        StartExecResults::Detached => {
            info!(container=%job.container_name, job=%job.name, "exec detached");
        }
    }

    info!(container=%job.container_name, job=%job.name, "exec finished");
    Ok(())
}

fn next_instant(schedule: Schedule) -> anyhow::Result<tokio::time::Instant> {
    let now: DateTime<Utc> = Utc::now();
    let next_dt = schedule
        .upcoming(Utc)
        .next()
        .ok_or_else(|| anyhow!("cron produced no next occurrence"))?;
    let dur = (next_dt - now).to_std().unwrap_or_default();
    Ok(tokio::time::Instant::now() + dur)
}
