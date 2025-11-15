use crate::gh::{client, mutations};
use crate::git::{commit_ref, notes, slots};
use crate::model::{CommitMetadata, Config};
use crate::stack::discover_stack;
use anyhow::{Context, Result};
use console::style;
use git2::Repository;

/// Manually assign a slot to a commit
pub async fn set_slot(commit_ref: &str, slot: &str, skip_confirm: bool) -> Result<()> {
    let git_repo = Repository::open(".").context("Failed to open git repository")?;
    let config = Config::load(&git_repo)?;
    let gh_client = client::create_client()?;

    println!("🔍 Looking up commit and validating slot...\n");

    // Validate slot name
    slots::validate_slot_name(slot)?;

    // Discover stack with full state
    let stack = discover_stack(&git_repo, &config, &gh_client).await?;

    // Get owner/repo from stack (already derived during discovery)
    let owner = stack
        .entries
        .first()
        .and_then(|e| e.repo_owner.clone())
        .unwrap_or_default();
    let repo_name = stack
        .entries
        .first()
        .and_then(|e| e.repo_name.clone())
        .unwrap_or_default();

    // Resolve commit reference to OID using the stack
    let commit_oid = commit_ref::resolve_commit_ref(&git_repo, &stack, commit_ref)?;

    let commit = git_repo.find_commit(commit_oid)?;
    let short_sha = &commit_oid.to_string()[..7];
    let commit_subject = commit.summary().unwrap_or("<no subject>").to_string();

    println!(
        "  {} Commit: {} ({})",
        style("→").dim(),
        style(short_sha).yellow(),
        style(&commit_subject).dim()
    );

    let current_branch = &stack.current_branch;

    // Load slot cache
    let mut slot_cache = slots::SlotCache::load(&git_repo)?;

    // Check if slot is available
    if !slot_cache.is_slot_available(current_branch, slot) {
        eprintln!("\n{}", style("⚠️  Warning").yellow().bold());
        eprintln!(
            "  Slot '{}' is already in use on branch '{}'",
            slot, current_branch
        );
        eprintln!("  This may cause conflicts with existing commits.");
    }

    // Generate head_ref for this slot
    let head_ref = slots::generate_head_ref(current_branch, slot);

    println!(
        "  {} Slot: {} → {}",
        style("→").dim(),
        style(slot).yellow().bold(),
        style(&head_ref).cyan()
    );

    // Find existing entry in stack for this commit
    let existing_entry = stack.entries.iter().find(|e| e.oid == commit_oid);

    let preserve_pr = if let Some(entry) = existing_entry {
        if let Some(existing_slot) = &entry.slot {
            let existing_head_ref = slots::generate_head_ref(current_branch, existing_slot);
            let slot_changed = existing_slot != slot;
            let has_pr = entry.pr_number.is_some();

            if slot_changed && has_pr {
                let pr_number = entry.pr_number.unwrap();
                println!(
                    "\n{}",
                    style("⚠️  Warning: Slot Change Detected").yellow().bold()
                );
                println!("  Commit {} already has:", short_sha);
                println!("    PR: #{}", pr_number);
                println!("    Slot: {} ({})", existing_slot, existing_head_ref);
                println!(
                    "\n  Changing the slot will change the branch name to: {}",
                    style(&head_ref).cyan()
                );
                println!(
                    "  {}",
                    style("The existing PR cannot be updated to use the new branch.").yellow()
                );
                println!(
                    "  {}",
                    style("The PR will be closed and a new PR will be created on export.").yellow()
                );

                let confirmed = if skip_confirm {
                    true
                } else {
                    use dialoguer::Confirm;
                    Confirm::new()
                        .with_prompt(format!(
                            "Do you want to change slot from {} to {} and close PR #{}?",
                            existing_slot, slot, pr_number
                        ))
                        .default(false)
                        .interact()?
                };

                if !confirmed {
                    println!("\n{}", style("✗ Operation cancelled").red());
                    return Ok(());
                }

                println!();

                // Close the PR on GitHub
                println!("🔒 Closing PR #{}...", pr_number);
                let comment = format!(
                    "This PR is being closed because the commit slot was changed from `{}` to `{}`.\n\n\
                     The branch name will change from `{}` to `{}`.\n\n\
                     A new PR will be created with the updated branch name.",
                    existing_slot, slot, existing_head_ref, head_ref
                );

                if let Err(e) =
                    mutations::add_pr_comment(&gh_client, &owner, &repo_name, pr_number, &comment)
                        .await
                {
                    eprintln!("  Warning: Failed to add comment to PR: {}", e);
                } else {
                    println!("  ✓ Added comment to PR");
                }

                match mutations::close_pull_request(&gh_client, &owner, &repo_name, pr_number).await
                {
                    Ok(_) => println!("  ✓ PR #{} closed", pr_number),
                    Err(e) => {
                        eprintln!("  Warning: Failed to close PR: {}", e);
                        eprintln!("  You may need to close it manually.");
                    }
                }

                println!();
                false // Don't preserve PR when slot changes
            } else if slot_changed && entry.remote_branch_exists {
                println!(
                    "\n{}",
                    style("⚠️  Warning: Slot Change Detected").yellow().bold()
                );
                println!("  Commit {} already has:", short_sha);
                println!("    Slot: {} ({})", existing_slot, existing_head_ref);
                println!(
                    "\n  Changing the slot will change the branch name to: {}",
                    style(&head_ref).cyan()
                );

                let confirmed = if skip_confirm {
                    true
                } else {
                    use dialoguer::Confirm;
                    Confirm::new()
                        .with_prompt(format!(
                            "Do you want to change slot from {} to {}?",
                            existing_slot, slot
                        ))
                        .default(false)
                        .interact()?
                };

                if !confirmed {
                    println!("\n{}", style("✗ Operation cancelled").red());
                    return Ok(());
                }

                println!();
                true
            } else {
                true
            }
        } else {
            true
        }
    } else {
        true
    };

    // Get existing PR number if preserving
    let existing_pr = if preserve_pr {
        existing_entry.and_then(|e| e.pr_number)
    } else {
        None
    };

    // Create metadata with the specified slot
    let metadata = CommitMetadata {
        pr: existing_pr,
        slot: slot.to_string(),
    };

    // Mark slot as used in cache
    slot_cache.mark_slot_used(current_branch, slot);

    // Write metadata to the commit
    println!("\n📝 Assigning slot {} to commit {}...", slot, short_sha);
    notes::write_note(&git_repo, commit_oid, &metadata, &config.notes_ref)
        .context("Failed to write note to commit")?;

    println!("  ✓ Updated local metadata");

    // Save slot cache
    slot_cache.save(&git_repo)?;

    println!(
        "\n{} Commit {} is now using slot {}!",
        style("✨").green(),
        short_sha,
        slot
    );
    println!(
        "\n{}",
        style("Run 'git-stk export' to push the commit and create/update PRs.").dim()
    );

    Ok(())
}
