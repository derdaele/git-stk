use clap::{Parser, Subcommand};
use git_stk::commands;

#[derive(Parser)]
#[command(name = "git-stk")]
#[command(about = "Manage stacked pull requests with ease", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// View the current stack of commits and their PR status
    View,
    /// Export the stack to GitHub by creating/updating branches and PRs
    Export {
        /// Create PRs as drafts
        #[arg(long)]
        draft: bool,
        /// Mark PRs as ready for review (opposite of --draft)
        #[arg(long, conflicts_with = "draft")]
        ready: bool,
        /// Only push branches, skip PR creation/updates
        #[arg(long)]
        push_only: bool,
        /// Only create/update PRs, skip pushing branches
        #[arg(long, conflicts_with = "push_only")]
        pr_only: bool,
        /// Open created/updated PRs in browser
        #[arg(long)]
        open: bool,
        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
        /// Output results as JSON
        #[arg(long)]
        json: bool,
        /// Show verbose output including reconciliation details
        #[arg(long, short)]
        verbose: bool,
    },
    /// Land the bottom PR in the stack (merge, pull, rebase, re-export)
    Land {
        /// Skip waiting for merge to complete
        #[arg(long)]
        skip_wait: bool,
    },
    /// Run post-merge operations after a PR has been manually merged
    Landed,
    /// Set commit metadata (PR, slot, etc.)
    Set {
        #[command(subcommand)]
        command: SetCommands,
    },
}

#[derive(Subcommand)]
enum SetCommands {
    /// Manually assign a slot to a commit
    Slot {
        /// Commit reference: SHA (abc123), stack index (1, 2, 3...), "last", or git ref (HEAD, branch name)
        commit: String,
        /// Slot identifier (e.g., "01", "02", or custom like "add-tests")
        slot: String,
        /// Skip confirmation prompts (automatically answer yes)
        #[arg(long, short)]
        yes: bool,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::View => commands::view().await,
        Commands::Export {
            draft,
            ready: _,
            push_only,
            pr_only,
            open,
            dry_run,
            json,
            verbose,
        } => {
            let options = commands::ExportOptions {
                draft,
                push_only,
                pr_only,
                open,
                dry_run,
                json,
                verbose,
            };
            commands::export(options).await
        }
        Commands::Land { skip_wait } => commands::land(skip_wait).await,
        Commands::Landed => commands::landed().await,
        Commands::Set { command } => match command {
            SetCommands::Slot { commit, slot, yes } => commands::set_slot(commit.as_str(), slot.as_str(), yes).await,
        },
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
