use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "pan-pipe")]
#[command(about = "AI-assisted development workflow installer for multiple coding agents")]
#[command(version)]
pub struct Cli {
    /// Enable verbose output
    #[arg(long, global = true)]
    pub verbose: bool,
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize pan-pipe in the current directory
    Init {
        /// Git ref (branch, tag, or commit) to fetch templates from
        #[arg(long)]
        r#ref: Option<String>,
        /// Install for the given coding agent (repeatable; skips interactive tool selection)
        #[arg(long = "tool", value_name = "TOOL")]
        tools: Vec<String>,
        /// Select all optional components without prompting
        #[arg(long, conflicts_with = "no_components")]
        all_components: bool,
        /// Select no optional components without prompting
        #[arg(long)]
        no_components: bool,
    },
    /// Update installed components
    Update {
        /// Git ref (branch, tag, or commit) to fetch templates from
        #[arg(long)]
        r#ref: Option<String>,
    },
    /// Manage optional components
    Components,
    /// Show status of installed files
    Status,
    /// Manage tool adapters
    #[command(subcommand)]
    Tool(ToolCommands),
}

#[derive(Subcommand)]
pub enum ToolCommands {
    /// Add a tool adapter
    Add {
        #[arg(num_args = 0..)]
        names: Vec<String>,
    },
    /// Remove a tool adapter
    Remove {
        #[arg(num_args = 1..)]
        names: Vec<String>,
    },
    /// List installed tool adapters
    List,
}
