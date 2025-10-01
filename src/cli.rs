#[derive(Debug, clap::Parser)]
#[command(name = "dockcron", version, about = "Application CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, clap::Subcommand)]
pub enum Command {
    Run(RunArgs),
}

#[derive(Debug, Clone, clap::Parser)]
pub struct RunArgs {
    #[arg(long, env = "LABEL_PREFIXES", default_values = ["dockcron", "ofelia", "chadburn"], value_delimiter = ',')]
    pub label_prefixes: Vec<String>,
    #[arg(long, env = "CONTAINER_LABEL_SELECTOR")]
    pub container_label_selector: Option<String>,
    #[arg(
        long,
        env = "DOCKER_HOST",
        default_value = "unix:///var/run/docker.sock"
    )]
    pub docker_host: String,
}

impl Cli {
    /// Parse CLI from std::env and return the parsed structure.
    pub fn parse() -> Self {
        <Self as clap::Parser>::parse()
    }
}
