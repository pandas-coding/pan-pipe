use anyhow::Result;
use clap::Parser;
use std::sync::atomic::{AtomicBool, Ordering};

mod adapters;
mod cli;
mod commands;
mod core;

use cli::{Cli, Commands, ToolCommands};

static VERBOSE: AtomicBool = AtomicBool::new(false);

pub fn is_verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

pub fn set_verbose(v: bool) {
    VERBOSE.store(v, Ordering::Relaxed);
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    set_verbose(cli.verbose);

    if is_verbose() {
        eprintln!("[verbose] pan-pipe starting");
    }

    match cli.command {
        Commands::Init {
            r#ref,
            tools,
            all_components,
            no_components,
        } => commands::init::run(r#ref, tools, all_components, no_components).await,
        Commands::Update { r#ref } => commands::update::run(r#ref).await,
        Commands::Components => commands::components::run().await,
        Commands::Status => commands::status::run().await,
        Commands::Tool(tool) => match tool {
            ToolCommands::Add { names } => commands::tool::add(names, None).await,
            ToolCommands::Remove { names } => commands::tool::remove(names).await,
            ToolCommands::List => commands::tool::list().await,
        },
    }
}
