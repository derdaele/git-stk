use crate::commands::export;
use crate::gh::{client, mutations, queries};
use crate::git::notes;
use crate::model::Config;
use crate::stack::discover_stack;
use crate::ui::callout;
use anyhow::{Context, Result};
use console::style;
use git2::Repository;
use std::process::Command;

/// Run post-merge operations: pull main, rebase feature branch, re-export stack
pub async fn run_post_merge_operations(
    git_repo: &Repository,
    config: &Config,
    landed_commit_oid: git2::Oid,
) -> Result<()> {
    let repo_path = git_repo
        .workdir()
        .context("Repository has no working directory")?;

    // Get current branch name
    let head = git_repo.head()?;
    let current_branch = head
        .shorthand()
        .context("Could not get current branch name")?
        .to_string();

    println!("\n📥 Updating {} branch...", config.base);

    // Fetch the base branch and update local tracking branch
    // Using refspec syntax: <remote-ref>:<local-ref> to update local main
    let refspec = format!("{}:{}", config.base, config.base);
    let output = Command::new("git")
        .current_dir(repo_path)
        .arg("fetch")
        .arg(&config.remote)
        .arg(&refspec)
        .output()
        .context("Failed to fetch and update base branch")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);

        // If fetch fails (e.g., local main has diverged), try force update
        if stderr.contains("non-fast-forward") || stderr.contains("rejected") {
            eprintln!("{} Local {} has diverged from remote. Force updating...",
                style("⚠").yellow(),
                config.base
            );

            let force_refspec = format!("+{}:{}", config.base, config.base);
            let force_output = Command::new("git")
                .current_dir(repo_path)
                .arg("fetch")
                .arg(&config.remote)
                .arg(&force_refspec)
                .output()
                .context("Failed to force update base branch")?;

            if !force_output.status.success() {
                let force_stderr = String::from_utf8_lossy(&force_output.stderr);
                eprintln!("{} Failed to update {}: {}",
                    style("✗").red(),
                    config.base,
                    force_stderr
                );
                eprintln!("\n{} Try running:", style("💡").yellow());
                eprintln!("  git fetch {} +{}:{}", config.remote, config.base, config.base);
                return Err(anyhow::anyhow!("Failed to update base branch"));
            }
        } else {
            eprintln!("{} Failed to fetch {}: {}",
                style("✗").red(),
                config.base,
                stderr
            );
            eprintln!("\n{} Try running:", style("💡").yellow());
            eprintln!("  git fetch {} {}:{}", config.remote, config.base, config.base);
            return Err(anyhow::anyhow!("Failed to fetch base branch"));
        }
    }

    println!("  {} Updated local {} to match remote", style("✓").green(), config.base);

    // Rebase current branch on top of the updated base
    println!("\n🔄 Rebasing {} on {}...", current_branch, config.base);

    let remote_base = format!("{}/{}", config.remote, config.base);
    let output = Command::new("git")
        .current_dir(repo_path)
        .arg("rebase")
        .arg(&remote_base)
        .output()
        .context("Failed to rebase")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);

        eprintln!("{} Rebase failed!", style("✗").red());
        eprintln!("\n{}", stderr);
        eprintln!("{}", stdout);

        eprintln!("\n{} The rebase encountered conflicts or errors.", style("💡").yellow());
        eprintln!("  You have a few options:");
        eprintln!("  1. Resolve conflicts and continue:");
        eprintln!("     git rebase --continue");
        eprintln!("     git stk export");
        eprintln!("  2. Abort the rebase:");
        eprintln!("     git rebase --abort");
        eprintln!("  3. Skip the problematic commit (if it's already merged):");
        eprintln!("     git rebase --skip");

        return Err(anyhow::anyhow!("Rebase failed - see guidance above"));
    }

    println!("  {} Rebased successfully", style("✓").green());

    // Check if the bottom commit changed after rebase (indicates successful landing)
    let gh_client = client::create_client()?;
    let stack_after = discover_stack(git_repo, config, &gh_client).await?;
    let bottom_changed = stack_after.is_empty() ||
                         (stack_after.entries.first().map(|e| e.oid) != Some(landed_commit_oid));

    if bottom_changed {
        // The landed commit is no longer at the bottom - it was successfully merged and rebased away
        // Now we can clean up the metadata

        // Get metadata for the landed commit to find its PR number
        let metadata = notes::read_note(git_repo, landed_commit_oid, &config.notes_ref).ok().flatten();

        // Clean up PR description to remove stale stack callout
        if let Some(meta) = metadata {
            if let Some(pr_number) = meta.pr {
                println!("\n🔄 Cleaning up PR #{} description...", pr_number);

                // Helper to clean up PR description
                let cleanup_result = async {
                    let remote = git_repo.find_remote(&config.remote)
                        .context("Failed to find remote")?;
                    let remote_url = remote.url()
                        .context("Remote URL is not valid UTF-8")?;
                    let (owner, repo_name) = client::parse_repo_from_url(remote_url)?;
                    let gh_client = client::create_client()?;

                    // Get current PR body
                    let pr_info = queries::get_pr(&gh_client, &owner, &repo_name, pr_number).await?;

                    // Strip the callout
                    let clean_body = callout::strip_callout(&pr_info.body);

                    // Always update PR with cleaned body (even if empty, to handle callout-only descriptions)
                    mutations::update_pull_request(
                        &gh_client,
                        &owner,
                        &repo_name,
                        pr_number,
                        None,
                        None,
                        Some(&clean_body),
                    ).await?;

                    Ok::<_, anyhow::Error>(())
                };

                match cleanup_result.await {
                    Ok(_) => println!("  {} Removed stack callout from PR description", style("✓").green()),
                    Err(e) => eprintln!("  {} Warning: Failed to update PR description: {}", style("⚠").yellow(), e),
                }
            }
        }

        // Clean up note for the landed commit (it's not in the stack anymore after being merged to main)
        println!("\n🧹 Cleaning up note for landed commit...");
        match notes::remove_note(git_repo, landed_commit_oid, &config.notes_ref) {
            Ok(_) => {
                let short_sha = format!("{:.7}", landed_commit_oid);
                println!("  {} Removed note for {}", style("✓").green(), short_sha);
                // Note: Note deletion will be pushed by export() at the end
            }
            Err(e) => {
                // Don't fail if note doesn't exist (might have been cleaned already)
                if !e.to_string().contains("not found") {
                    eprintln!("  {} Warning: Failed to remove note for landed commit: {}",
                        style("⚠").yellow(), e);
                }
            }
        }
    } else {
        println!("\n{} Bottom commit unchanged - skipping cleanup (already ran?)", style("ℹ").blue());
    }

    // Re-export the stack
    println!("\n📤 Re-exporting stack...");

    let export_options = export::ExportOptions {
        draft: false,
        push_only: false,
        pr_only: false,
        open: false,
        dry_run: false,
        json: false,
        verbose: false,
    };

    if let Err(e) = export::export(export_options).await {
        eprintln!("\n{} Failed to re-export stack after landing:", style("✗").red());
        eprintln!("  {}", e);
        return Err(e);
    }

    Ok(())
}
