use crate::gh::{client, mutations, queries};
use crate::model::{Config, PrState};
use crate::stack::discover_stack;
use crate::ui::callout;
use crate::workflows;
use anyhow::{bail, Context, Result};
use console::style;
use git2::Repository;
use std::time::Duration;

pub async fn land(skip_wait: bool) -> Result<()> {
    let git_repo = Repository::open(".")
        .context("Failed to open git repository. Are you in a git repository?")?;

    // Check for uncommitted changes (excluding ignored files)
    if git_repo.statuses(None)?.iter().any(|s| {
        let status = s.status();
        status.intersects(
            git2::Status::INDEX_NEW
                | git2::Status::INDEX_MODIFIED
                | git2::Status::INDEX_DELETED
                | git2::Status::INDEX_RENAMED
                | git2::Status::INDEX_TYPECHANGE
                | git2::Status::WT_NEW
                | git2::Status::WT_MODIFIED
                | git2::Status::WT_DELETED
                | git2::Status::WT_RENAMED
                | git2::Status::WT_TYPECHANGE
                | git2::Status::CONFLICTED,
        )
    }) {
        bail!("You have uncommitted changes. Please commit or stash them before landing.");
    }

    let config = Config::load(&git_repo)?;
    let gh_client = client::create_client()?;

    // Discover the stack with full state (includes remote refs and PR state)
    let stack = discover_stack(&git_repo, &config, &gh_client).await?;

    if stack.is_empty() {
        bail!("No commits in stack to land.");
    }

    // Get the bottom commit (first in the stack)
    let bottom_entry = &stack.entries[0];

    println!(
        "\n🚀 Landing commit: {} {}",
        style(&bottom_entry.short_sha).yellow(),
        style(&bottom_entry.subject).bold()
    );

    // Check if commit has a PR
    let pr_number = bottom_entry
        .pr_number
        .context("Bottom commit doesn't have a PR. Run 'git stk export' first.")?;

    // Get the head ref for the bottom commit
    let head_ref = bottom_entry
        .head_ref
        .as_ref()
        .context("Bottom commit doesn't have metadata. Run 'git stk export' first.")?;

    // Verify remote state (already fetched during discovery)
    println!("📍 Verifying remote state...");

    if !bottom_entry.remote_branch_exists {
        bail!(
            "Remote branch {} not found. Run 'git stk export' to push the branch.",
            head_ref
        );
    }

    if let Some(remote_oid) = bottom_entry.remote_oid {
        if remote_oid != bottom_entry.oid {
            bail!(
                "Remote branch {} points to {} but expected {}. Run 'git stk export' to push your changes.",
                head_ref,
                &remote_oid.to_string()[..7],
                &bottom_entry.oid.to_string()[..7]
            );
        }
    }

    println!("  {} Remote branch is in sync", style("✓").green());

    // Get owner/repo from stack
    let owner = bottom_entry
        .repo_owner
        .as_ref()
        .context("Missing repo owner")?;
    let repo_name = bottom_entry
        .repo_name
        .as_ref()
        .context("Missing repo name")?;

    // Check PR status (already fetched during discovery)
    println!("📋 Checking PR status...");

    match &bottom_entry.pr_state {
        Some(PrState::Merged) => {
            println!(
                "  {} PR #{} is already merged!",
                style("✓").green(),
                pr_number
            );
        }
        Some(PrState::Closed) => {
            bail!("PR #{} is closed. Cannot land a closed PR.", pr_number);
        }
        Some(PrState::Draft) => {
            bail!(
                "PR #{} is a draft. Mark it as ready for review before landing.",
                pr_number
            );
        }
        Some(PrState::Open) | None => {
            println!(
                "  {} PR #{} is open and ready to merge",
                style("✓").green(),
                pr_number
            );

            // Fetch PR body for cleanup (we need the full body, not just state)
            let pr_info = queries::get_pr(&gh_client, owner, repo_name, pr_number).await?;

            // Strip stack callout from PR body
            let clean_body = callout::strip_callout(&pr_info.body);

            // Merge the PR with cleaned body
            println!("\n🔀 Merging PR #{}...", pr_number);
            mutations::merge_pull_request(
                &gh_client,
                owner,
                repo_name,
                pr_number,
                None, // Use default title
                if clean_body.is_empty() {
                    None
                } else {
                    Some(&clean_body)
                },
            )
            .await?;
            println!("  {} Merge initiated", style("✓").green());

            if !skip_wait {
                // Poll until merged
                println!(
                    "\n⏳ Waiting for merge to complete (timeout: {} minutes)...",
                    config.land_timeout_minutes
                );
                let mut attempts = 0;
                let max_attempts = config.land_timeout_minutes * 12; // 5 second intervals

                loop {
                    attempts += 1;
                    if attempts > max_attempts {
                        bail!("Timeout waiting for PR to merge. Check GitHub for status.");
                    }

                    tokio::time::sleep(Duration::from_secs(5)).await;

                    let pr_status =
                        queries::get_pr(&gh_client, owner, repo_name, pr_number).await?;

                    if pr_status.state == PrState::Merged {
                        println!("  {} PR merged successfully!", style("✓").green());
                        break;
                    }

                    print!(".");
                    use std::io::Write;
                    std::io::stdout().flush().ok();
                }
            } else {
                println!(
                    "\n{} Skipping merge wait. Run 'git stk landed' after the PR is merged.",
                    style("ℹ").blue()
                );
                return Ok(());
            }
        }
    }

    // Run post-merge operations with the landed commit OID
    workflows::run_post_merge_operations(&git_repo, &config, bottom_entry.oid).await?;

    println!("\n{} Successfully landed!", style("🎉").green());

    Ok(())
}
