use crate::gh::client;
use crate::model::Config;
use crate::stack::discover_stack;
use crate::workflows;
use anyhow::{bail, Context};
use console::style;
use git2::Repository;

pub async fn landed() -> anyhow::Result<()> {
    println!("\n{} Checking for merged commits...", style("🔧").cyan());

    let git_repo = Repository::open(".").context("Failed to open git repository")?;
    let config = Config::load(&git_repo)?;
    let gh_client = client::create_client()?;

    // Discover stack with full state (includes PR merged status from GitHub)
    let stack = discover_stack(&git_repo, &config, &gh_client).await?;

    if stack.is_empty() {
        bail!("No commits in stack - nothing to clean up after landing");
    }

    // Find the first merged commit (should be at the bottom of the stack)
    let merged_entry = stack.entries.iter().find(|e| e.merged_into_main);

    let landed_commit_oid = if let Some(entry) = merged_entry {
        println!(
            "  {} Found merged commit: {} {}",
            style("✓").green(),
            style(&entry.short_sha).yellow(),
            style(&entry.subject).dim()
        );
        entry.oid
    } else {
        // Fallback: assume bottom commit was landed (for backwards compatibility)
        println!(
            "  {} No merged PR found, assuming bottom commit was landed",
            style("ℹ").blue()
        );
        stack.entries[0].oid
    };

    // Run post-merge operations with the landed commit OID
    workflows::run_post_merge_operations(&git_repo, &config, landed_commit_oid).await?;

    println!("\n{} Post-merge operations completed!", style("✓").green());

    Ok(())
}
