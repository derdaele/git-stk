//! Export command - pushes commits and creates/updates PRs for the stack.

use crate::gh::{client, mutations, queries};
use crate::git::{notes, refs, reorder_detect, slots};
use crate::model::{CommitMetadata, Config, PrState, Stack};
use crate::stack::discover_stack;
use crate::ui::callout;
use anyhow::{bail, Context, Result};
use console::style;
use git2::Repository;
use octocrab::Octocrab;
use std::collections::HashMap;

// =============================================================================
// Options
// =============================================================================

#[derive(Debug, Clone, Default)]
pub struct ExportOptions {
    pub draft: bool,
    pub push_only: bool,
    pub pr_only: bool,
    pub open: bool,
    pub dry_run: bool,
    pub json: bool,
    pub verbose: bool,
}

// =============================================================================
// Export Plan - describes what actions will be taken
// =============================================================================

#[derive(Debug)]
struct ExportPlan {
    slot_assignments: Vec<SlotAssignment>,
    refs_to_push: Vec<RefToPush>,
    prs_to_create: Vec<PrToCreate>,
    prs_to_update: Vec<PrToUpdate>,
    phase1_base_updates: Vec<(u64, String)>,
    phase3_base_updates: Vec<(u64, String)>,
}

#[derive(Debug)]
struct SlotAssignment {
    oid: git2::Oid,
    slot: String,
    head_ref: String,
    is_new: bool,
}

#[derive(Debug)]
struct RefToPush {
    oid: git2::Oid,
    head_ref: String,
    needs_push: bool,
}

#[derive(Debug)]
struct PrToCreate {
    oid: git2::Oid,
    head_ref: String,
    base_ref: String,
    title: String,
    body: String,
}

#[derive(Debug)]
struct PrToUpdate {
    pr_number: u64,
    head_ref: String,
    base_ref: String,
    title: String,
    needs_base_update: bool,
    is_reordered: bool,
}

// =============================================================================
// Main Export Function
// =============================================================================

pub async fn export(options: ExportOptions) -> Result<()> {
    let git_repo = Repository::open(".").context("Failed to open git repository")?;
    let config = Config::load(&git_repo)?;
    let gh_client = client::create_client()?;

    Config::ensure_notes_rewrite_config(&git_repo, &config.notes_ref)?;

    let mut stack = discover_stack(&git_repo, &config, &gh_client).await?;

    if stack.is_empty() {
        if !options.json {
            println!("No commits to export.");
        }
        return Ok(());
    }

    let owner = stack.entries[0].repo_owner.clone().context("Missing repo owner")?;
    let repo_name = stack.entries[0].repo_name.clone().context("Missing repo name")?;

    // Build the plan
    let plan = build_export_plan(&git_repo, &config, &gh_client, &stack, &owner, &repo_name, &options).await?;

    // Display plan (always, but styled differently for dry-run)
    if options.dry_run {
        display_dry_run_plan(&plan, &options);
        return Ok(());
    }

    // Execute the plan
    execute_export_plan(
        &git_repo,
        &config,
        &gh_client,
        &mut stack,
        &owner,
        &repo_name,
        &plan,
        &options,
    ).await?;

    // Show final state
    if !options.json {
        println!("\n✨ Export complete!\n");
        crate::commands::view().await?;
    }

    Ok(())
}

// =============================================================================
// Plan Building
// =============================================================================

async fn build_export_plan(
    git_repo: &Repository,
    config: &Config,
    gh_client: &Octocrab,
    stack: &Stack,
    owner: &str,
    repo_name: &str,
    options: &ExportOptions,
) -> Result<ExportPlan> {
    let current_branch = &stack.current_branch;

    // Build slot assignments
    let slot_assignments = build_slot_assignments(git_repo, stack)?;

    // Build metadata map for reorder detection
    let metadata_map = build_metadata_map(stack);

    // Detect reordering
    let reorder_info = reorder_detect::detect_reordering(
        git_repo,
        &config.remote,
        current_branch,
        &stack.entries,
        &metadata_map,
    );
    let (phase1_base_updates, phase3_base_updates) = reorder_detect::calculate_base_updates(
        current_branch,
        &stack.entries,
        &reorder_info,
        &metadata_map,
        &config.base,
    );

    // Build refs to push
    let refs_to_push = build_refs_to_push(git_repo, config, stack, &slot_assignments)?;

    // Build PR actions
    let (prs_to_create, prs_to_update) = build_pr_actions(
        git_repo,
        gh_client,
        stack,
        owner,
        repo_name,
        config,
        &slot_assignments,
        &phase1_base_updates,
        &phase3_base_updates,
        options,
    ).await?;

    Ok(ExportPlan {
        slot_assignments,
        refs_to_push,
        prs_to_create,
        prs_to_update,
        phase1_base_updates,
        phase3_base_updates,
    })
}

fn build_slot_assignments(git_repo: &Repository, stack: &Stack) -> Result<Vec<SlotAssignment>> {
    let mut slot_cache = slots::SlotCache::load(git_repo)?;
    let current_branch = &stack.current_branch;

    // Ensure cache knows about existing slots
    for entry in &stack.entries {
        if let Some(ref slot) = entry.slot {
            slot_cache.ensure_slot(current_branch, slot);
        }
    }

    let mut assignments = Vec::new();
    for entry in &stack.entries {
        let (slot, is_new) = if let Some(ref existing_slot) = entry.slot {
            (existing_slot.clone(), false)
        } else {
            (slot_cache.allocate(current_branch), true)
        };

        let head_ref = slots::generate_head_ref(current_branch, &slot);
        assignments.push(SlotAssignment {
            oid: entry.oid,
            slot,
            head_ref,
            is_new,
        });
    }

    Ok(assignments)
}

fn build_metadata_map(stack: &Stack) -> HashMap<git2::Oid, CommitMetadata> {
    let mut map = HashMap::new();
    for entry in &stack.entries {
        if let Some(ref slot) = entry.slot {
            map.insert(entry.oid, CommitMetadata {
                pr: entry.pr_number,
                slot: slot.clone(),
            });
        }
    }
    map
}

fn build_refs_to_push(
    git_repo: &Repository,
    config: &Config,
    stack: &Stack,
    slot_assignments: &[SlotAssignment],
) -> Result<Vec<RefToPush>> {
    let commits: Vec<(git2::Oid, String)> = stack.entries
        .iter()
        .zip(slot_assignments.iter())
        .map(|(entry, assignment)| (entry.oid, assignment.head_ref.clone()))
        .collect();

    let refs_status = refs::check_commits_to_push(git_repo, &config.remote, &commits)?;

    Ok(slot_assignments
        .iter()
        .map(|assignment| {
            let needs_push = refs_status
                .get(&assignment.head_ref)
                .map(|(needs, _)| *needs)
                .unwrap_or(true);
            RefToPush {
                oid: assignment.oid,
                head_ref: assignment.head_ref.clone(),
                needs_push,
            }
        })
        .collect())
}

async fn build_pr_actions(
    git_repo: &Repository,
    gh_client: &Octocrab,
    stack: &Stack,
    owner: &str,
    repo_name: &str,
    config: &Config,
    slot_assignments: &[SlotAssignment],
    phase1_updates: &[(u64, String)],
    phase3_updates: &[(u64, String)],
    _options: &ExportOptions,
) -> Result<(Vec<PrToCreate>, Vec<PrToUpdate>)> {
    let mut to_create = Vec::new();
    let mut to_update = Vec::new();

    for (i, entry) in stack.entries.iter().enumerate() {
        let assignment = &slot_assignments[i];
        let base_ref = if i == 0 {
            config.base.clone()
        } else {
            slot_assignments[i - 1].head_ref.clone()
        };

        let commit = git_repo.find_commit(entry.oid)?;
        let title = commit.summary().context("Failed to get commit summary")?.to_string();
        let body = extract_commit_body(commit.message().unwrap_or(""));

        // Check for existing PR
        let existing_pr = if let Some(pr_number) = entry.pr_number {
            queries::get_pr(gh_client, owner, repo_name, pr_number).await.ok()
        } else {
            queries::find_pr_by_head(gh_client, owner, repo_name, &assignment.head_ref).await?
        };

        if let Some(pr_info) = existing_pr {
            let is_reordered = phase1_updates.iter().any(|(pr, _)| *pr == pr_info.number)
                || phase3_updates.iter().any(|(pr, _)| *pr == pr_info.number);

            to_update.push(PrToUpdate {
                pr_number: pr_info.number,
                head_ref: assignment.head_ref.clone(),
                base_ref: base_ref.clone(),
                title,
                needs_base_update: pr_info.base_ref != base_ref && !is_reordered,
                is_reordered,
            });
        } else {
            to_create.push(PrToCreate {
                oid: entry.oid,
                head_ref: assignment.head_ref.clone(),
                base_ref,
                title,
                body,
            });
        }
    }

    Ok((to_create, to_update))
}

// =============================================================================
// Plan Display (Dry Run)
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
enum PrAction {
    Create,
    Update,
    Reorder,
    Synced,
}

struct PrDisplayItem {
    action: PrAction,
    pr_number: Option<u64>,
    title: String,
    head_ref: String,
    base_ref: String,
    is_draft: bool,
}

fn display_pr_tree(items: &[PrDisplayItem], has_actions: &mut bool) {
    // Pre-calculate column data for alignment
    struct RowData {
        action_icon: &'static str,
        action_color: console::Color,
        pr_str: String,
        base: String,
        head: String,
        title: String,
        draft_str: String,
    }

    // Check if any items have actions
    *has_actions = items.iter().any(|item| {
        matches!(item.action, PrAction::Create | PrAction::Update | PrAction::Reorder)
    });

    let rows: Vec<RowData> = items
        .iter()
        .map(|item| {
            let (action_icon, action_color) = match item.action {
                PrAction::Create => ("+", console::Color::Magenta),
                PrAction::Update => ("~", console::Color::Yellow),
                PrAction::Reorder => ("↻", console::Color::Yellow),
                PrAction::Synced => ("✓", console::Color::Green),
            };

            let pr_str = match item.pr_number {
                Some(num) => format!("#{}", num),
                None => "new".to_string(),
            };

            let base = shorten_branch(&item.base_ref);
            let head = shorten_branch(&item.head_ref);

            let max_title = 35;
            let title = if item.title.len() > max_title {
                format!("{}…", &item.title[..max_title - 1])
            } else {
                item.title.clone()
            };

            let draft_str = if item.is_draft && item.action == PrAction::Create {
                " (draft)".to_string()
            } else {
                String::new()
            };

            RowData { action_icon, action_color, pr_str, base, head, title, draft_str }
        })
        .collect();

    // Calculate max widths
    let max_pr = rows.iter().map(|r| r.pr_str.len()).max().unwrap_or(3);
    let max_base = rows.iter().map(|r| r.base.len()).max().unwrap_or(4);
    let max_head = rows.iter().map(|r| r.head.len()).max().unwrap_or(4);

    // Print aligned rows
    for (i, row) in rows.iter().enumerate() {
        let pos = i + 1;
        println!(
            "    {}{} {} {:<pw$}  {:>bw$} {} {:<hw$}  {}{}",
            style(pos).cyan().bold(),
            style(".").dim(),
            style(row.action_icon).fg(row.action_color),
            style(&row.pr_str).fg(row.action_color),
            style(&row.base).dim(),
            style("←").dim(),
            style(&row.head).cyan(),
            style(&row.title).bold(),
            style(&row.draft_str).yellow(),
            pw = max_pr,
            bw = max_base,
            hw = max_head,
        );
    }
}

fn shorten_branch(branch: &str) -> String {
    // Remove common prefixes to make it shorter
    let short = branch
        .strip_prefix("refs/heads/")
        .unwrap_or(branch);

    // If still long, take last component after --
    if short.len() > 20 {
        if let Some(pos) = short.rfind("--") {
            return format!("…{}", &short[pos..]);
        }
    }
    short.to_string()
}

fn display_dry_run_plan(plan: &ExportPlan, options: &ExportOptions) {
    println!();
    println!("{}", style("╔══════════════════════════════════════════════════════════════╗").cyan());
    println!("{}  {}  {}",
        style("║").cyan(),
        style("DRY RUN").cyan().bold(),
        style("No changes will be made                            ║").cyan()
    );
    println!("{}", style("╚══════════════════════════════════════════════════════════════╝").cyan());
    println!();

    let mut has_actions = false;

    // Section 1: Slot Assignments
    let new_slots: Vec<_> = plan.slot_assignments.iter().filter(|s| s.is_new).collect();
    if !new_slots.is_empty() {
        has_actions = true;
        println!("  {} {}",
            style("📦").cyan(),
            style("Slot Assignments").bold()
        );
        println!("  {}", style("─".repeat(50)).dim());
        for slot in &new_slots {
            println!("    {} {} {} {}",
                style("NEW").magenta().bold(),
                style(&slot.slot).yellow().bold(),
                style("→").dim(),
                style(&slot.head_ref).cyan()
            );
        }
        println!();
    }

    // Section 2: Phase 1 Base Updates (Pre-push reorder)
    if !plan.phase1_base_updates.is_empty() {
        has_actions = true;
        println!("  {} {}",
            style("🔄").cyan(),
            style("Pre-Push Base Updates (Reorder Safety)").bold()
        );
        println!("  {}", style("─".repeat(50)).dim());
        for (pr_num, new_base) in &plan.phase1_base_updates {
            println!("    PR {} {} {}",
                style(format!("#{}", pr_num)).yellow().bold(),
                style("→ base:").dim(),
                style(new_base).cyan()
            );
        }
        println!();
    }

    // Section 3: Refs to Push
    if !options.pr_only {
        let refs_needing_push: Vec<_> = plan.refs_to_push.iter().filter(|r| r.needs_push).collect();
        let refs_up_to_date: Vec<_> = plan.refs_to_push.iter().filter(|r| !r.needs_push).collect();

        println!("  {} {}",
            style("🚀").cyan(),
            style("Push Refs").bold()
        );
        println!("  {}", style("─".repeat(50)).dim());

        if refs_needing_push.is_empty() {
            println!("    {} {}",
                style("✓").green(),
                style("All refs up-to-date").dim()
            );
        } else {
            has_actions = true;
            for r in &refs_needing_push {
                println!("    {} {} {}",
                    style("PUSH").green().bold(),
                    style(&r.head_ref).cyan(),
                    style("--force").yellow().dim()
                );
            }
        }

        if !refs_up_to_date.is_empty() && options.verbose {
            for r in &refs_up_to_date {
                println!("    {} {} {}",
                    style("SKIP").dim(),
                    style(&r.head_ref).dim(),
                    style("(up-to-date)").dim()
                );
            }
        }
        println!();
    }

    // Section 4: Pull Requests
    if !options.push_only {
        let total_prs = plan.prs_to_create.len() + plan.prs_to_update.len();

        if total_prs > 0 {
            println!("  {} {} {}",
                style("📝").cyan(),
                style("Pull Requests").bold(),
                style(format!("({} total)", total_prs)).dim()
            );
            println!("  {}", style("─".repeat(50)).dim());
            println!();

            // Build ordered list matching stack order
            let mut pr_items: Vec<PrDisplayItem> = Vec::new();

            for assignment in plan.slot_assignments.iter() {
                // Check if this is a create or update
                if let Some(create) = plan.prs_to_create.iter().find(|p| p.oid == assignment.oid) {
                    pr_items.push(PrDisplayItem {
                        action: PrAction::Create,
                        pr_number: None,
                        title: create.title.clone(),
                        head_ref: create.head_ref.clone(),
                        base_ref: create.base_ref.clone(),
                        is_draft: options.draft,
                    });
                } else if let Some(update) = plan.prs_to_update.iter().find(|p| p.head_ref == assignment.head_ref) {
                    let action = if update.is_reordered {
                        PrAction::Reorder
                    } else if update.needs_base_update {
                        PrAction::Update
                    } else {
                        PrAction::Synced
                    };

                    pr_items.push(PrDisplayItem {
                        action,
                        pr_number: Some(update.pr_number),
                        title: update.title.clone(),
                        head_ref: update.head_ref.clone(),
                        base_ref: update.base_ref.clone(),
                        is_draft: false,
                    });
                }
            }

            // Display as visual tree
            display_pr_tree(&pr_items, &mut has_actions);
            println!();
        }
    }

    // Section 5: Phase 3 Base Updates (Post-push reorder)
    if !plan.phase3_base_updates.is_empty() {
        has_actions = true;
        println!("  {} {}",
            style("🔗").cyan(),
            style("Post-Push Base Updates (Final Chain)").bold()
        );
        println!("  {}", style("─".repeat(50)).dim());
        for (pr_num, new_base) in &plan.phase3_base_updates {
            println!("    PR {} {} {}",
                style(format!("#{}", pr_num)).yellow().bold(),
                style("→ base:").dim(),
                style(new_base).cyan()
            );
        }
        println!();
    }

    // Section 6: Callout Updates (only for multi-PR stacks)
    let total_prs = plan.prs_to_create.len() + plan.prs_to_update.len();
    if !options.push_only && total_prs > 1 {
        println!("  {} {}",
            style("💬").cyan(),
            style("PR Description Updates").bold()
        );
        println!("  {}", style("─".repeat(50)).dim());
        println!("    {} {} PR descriptions with stack callout",
            style("UPDATE").blue().bold(),
            total_prs
        );
        println!();
    }

    // Summary
    println!("{}", style("══════════════════════════════════════════════════════════════").cyan());

    if has_actions {
        println!();
        println!("  {} Run {} to execute these changes.",
            style("→").dim(),
            style("git stk export").green().bold()
        );
    } else {
        println!();
        println!("  {} {}",
            style("✓").green(),
            style("Everything is up-to-date!").green()
        );
    }
    println!();
}

// =============================================================================
// Plan Execution
// =============================================================================

async fn execute_export_plan(
    git_repo: &Repository,
    config: &Config,
    gh_client: &Octocrab,
    stack: &mut Stack,
    owner: &str,
    repo_name: &str,
    plan: &ExportPlan,
    options: &ExportOptions,
) -> Result<()> {
    // Step 1: Save slot assignments
    save_slot_assignments(git_repo, config, plan, stack, options)?;

    // Step 2: Phase 1 base updates (pre-push, for reordering)
    if !options.pr_only && !plan.phase1_base_updates.is_empty() {
        execute_phase1_updates(gh_client, owner, repo_name, plan, options).await?;
    }

    // Step 3: Push refs
    if !options.pr_only {
        execute_push_refs(git_repo, config, plan, options)?;
    }

    // Step 4: Create/update PRs
    if !options.push_only {
        let pr_urls = execute_pr_operations(git_repo, config, gh_client, stack, owner, repo_name, plan, options).await?;

        // Step 5: Base updates (regular + phase3 reorder finalization)
        execute_base_updates(gh_client, owner, repo_name, plan, options).await?;

        // Step 6: Update PR descriptions with callouts
        execute_callout_updates(git_repo, gh_client, stack, owner, repo_name, options).await?;

        // Step 7: Push notes
        push_notes_to_remote(git_repo, config, options)?;

        // Step 8: Open URLs if requested
        if options.open {
            open_pr_urls(&pr_urls);
        }
    }

    Ok(())
}

fn save_slot_assignments(
    git_repo: &Repository,
    config: &Config,
    plan: &ExportPlan,
    stack: &Stack,
    options: &ExportOptions,
) -> Result<()> {
    let new_slots: Vec<_> = plan.slot_assignments.iter().filter(|s| s.is_new).collect();
    if !new_slots.is_empty() && !options.json {
        println!("📦 Assigning {} new slot{}...", new_slots.len(), if new_slots.len() == 1 { "" } else { "s" });
    }

    let mut slot_cache = slots::SlotCache::load(git_repo)?;
    let current_branch = &stack.current_branch;

    for assignment in &plan.slot_assignments {
        slot_cache.ensure_slot(current_branch, &assignment.slot);
    }
    slot_cache.save(git_repo)?;

    // Write notes for slot assignments
    if !options.push_only {
        for (entry, assignment) in stack.entries.iter().zip(plan.slot_assignments.iter()) {
            let metadata = CommitMetadata {
                pr: entry.pr_number,
                slot: assignment.slot.clone(),
            };
            notes::write_note(git_repo, entry.oid, &metadata, &config.notes_ref)?;
        }
    }

    Ok(())
}

async fn execute_phase1_updates(
    gh_client: &Octocrab,
    owner: &str,
    repo_name: &str,
    plan: &ExportPlan,
    options: &ExportOptions,
) -> Result<()> {
    if !options.json {
        println!("🔄 Preparing {} PR{} for reorder...", plan.phase1_base_updates.len(), if plan.phase1_base_updates.len() == 1 { "" } else { "s" });
    }
    mutations::batch_update_pr_bases(gh_client, owner, repo_name, &plan.phase1_base_updates).await?;
    if !options.json {
        println!("   ✓ Ready");
    }
    Ok(())
}

fn execute_push_refs(
    git_repo: &Repository,
    config: &Config,
    plan: &ExportPlan,
    options: &ExportOptions,
) -> Result<()> {
    let refs_to_push: Vec<_> = plan.refs_to_push.iter().filter(|r| r.needs_push).collect();

    if refs_to_push.is_empty() {
        return Ok(());
    }

    if !options.json {
        println!("🚀 Pushing {} ref{}...", refs_to_push.len(), if refs_to_push.len() == 1 { "" } else { "s" });
    }

    let commits: Vec<(git2::Oid, String)> = refs_to_push
        .iter()
        .map(|r| (r.oid, r.head_ref.clone()))
        .collect();
    let refspecs = refs::build_refspecs_from_oids(&commits);

    let mut capabilities = refs::RemoteCapabilities::default();
    let result = refs::push_refs(git_repo, &config.remote, &refspecs, &mut capabilities)?;

    if !result.success {
        bail!("Failed to push refs: {}", result.error.unwrap_or_default());
    }

    if !options.json {
        println!("   ✓ Pushed");
    }

    Ok(())
}

async fn execute_pr_operations(
    git_repo: &Repository,
    config: &Config,
    gh_client: &Octocrab,
    stack: &mut Stack,
    owner: &str,
    repo_name: &str,
    plan: &ExportPlan,
    options: &ExportOptions,
) -> Result<Vec<String>> {
    let mut pr_urls = Vec::new();
    let mut created_pr_nums = Vec::new();

    // Track existing PRs
    for pr_update in &plan.prs_to_update {
        pr_urls.push(format!("https://github.com/{}/{}/pull/{}", owner, repo_name, pr_update.pr_number));

        // Update stack entry
        if let Some(entry) = stack.entries.iter_mut().find(|e| {
            plan.slot_assignments.iter().any(|a| a.oid == e.oid && a.head_ref == pr_update.head_ref)
        }) {
            entry.pr_number = Some(pr_update.pr_number);
            entry.head_ref = Some(pr_update.head_ref.clone());
        }
    }

    // Process creates (new PRs)
    if !plan.prs_to_create.is_empty() {
        if !options.json {
            println!("📝 Creating {} PR{}...", plan.prs_to_create.len(), if plan.prs_to_create.len() == 1 { "" } else { "s" });
        }

        for pr_create in &plan.prs_to_create {
            let initial_body = if pr_create.body.is_empty() { " ".to_string() } else { pr_create.body.clone() };

            let pr_num = mutations::create_pull_request(
                gh_client, owner, repo_name,
                &pr_create.head_ref, &pr_create.base_ref,
                &pr_create.title, &initial_body,
                options.draft,
            ).await?;

            created_pr_nums.push(pr_num);
            pr_urls.push(format!("https://github.com/{}/{}/pull/{}", owner, repo_name, pr_num));

            // Update stack entry and write note
            if let Some(entry) = stack.entries.iter_mut().find(|e| e.oid == pr_create.oid) {
                entry.pr_number = Some(pr_num);
                entry.head_ref = Some(pr_create.head_ref.clone());
                entry.pr_state = Some(if options.draft { PrState::Draft } else { PrState::Open });

                let slot = plan.slot_assignments.iter().find(|a| a.oid == entry.oid).unwrap();
                let metadata = CommitMetadata { pr: Some(pr_num), slot: slot.slot.clone() };
                notes::write_note(git_repo, entry.oid, &metadata, &config.notes_ref)?;
            }
        }

        if !options.json {
            let pr_list: Vec<String> = created_pr_nums.iter().map(|n| format!("#{}", n)).collect();
            println!("   ✓ Created {}", pr_list.join(", "));
        }
    }

    Ok(pr_urls)
}

/// Execute all post-push base updates (regular base changes + phase3 reorder updates)
async fn execute_base_updates(
    gh_client: &Octocrab,
    owner: &str,
    repo_name: &str,
    plan: &ExportPlan,
    options: &ExportOptions,
) -> Result<()> {
    // Collect all base updates: regular (needs_base_update) + phase3 (reorder finalization)
    let mut all_updates: Vec<(u64, String)> = plan.prs_to_update
        .iter()
        .filter(|u| u.needs_base_update)
        .map(|u| (u.pr_number, u.base_ref.clone()))
        .collect();

    all_updates.extend(plan.phase3_base_updates.clone());

    if all_updates.is_empty() {
        return Ok(());
    }

    if !options.json {
        println!("🔗 Updating {} PR base{}...", all_updates.len(), if all_updates.len() == 1 { "" } else { "s" });
    }

    mutations::batch_update_pr_bases(gh_client, owner, repo_name, &all_updates).await?;

    if !options.json {
        println!("   ✓ Updated");
    }

    Ok(())
}

async fn execute_callout_updates(
    git_repo: &Repository,
    gh_client: &Octocrab,
    stack: &Stack,
    owner: &str,
    repo_name: &str,
    options: &ExportOptions,
) -> Result<()> {
    // Skip callout updates for single-PR stacks (no stack navigation needed)
    if stack.entries.len() <= 1 {
        return Ok(());
    }

    if !options.json {
        println!("💬 Syncing {} PR descriptions...", stack.entries.len());
    }

    // Build all body updates
    let mut body_updates: Vec<(u64, String)> = Vec::new();
    for (i, entry) in stack.entries.iter().enumerate() {
        let pr_number = entry.pr_number.expect("PR number should exist");

        let commit = git_repo.find_commit(entry.oid)?;
        let body_text = extract_commit_body(commit.message().unwrap_or(""));

        let callout_text = callout::generate_callout(&stack.entries, i + 1, owner, repo_name);
        let full_body = if body_text.is_empty() {
            callout_text
        } else {
            callout::inject_callout(&body_text, &callout_text)
        };

        body_updates.push((pr_number, full_body));
    }

    // Execute all updates in a single GraphQL mutation
    mutations::batch_update_pr_bodies(gh_client, owner, repo_name, &body_updates).await?;

    if !options.json {
        println!("   ✓ Synced");
    }

    Ok(())
}

fn push_notes_to_remote(git_repo: &Repository, config: &Config, options: &ExportOptions) -> Result<()> {
    if !options.json {
        println!("☁️  Pushing metadata...");
    }
    if let Err(e) = notes::push_notes(git_repo, &config.remote, &config.notes_ref) {
        if !options.json {
            eprintln!("   ⚠ Failed: {}", e);
        }
    } else if !options.json {
        println!("   ✓ Done");
    }
    Ok(())
}

fn open_pr_urls(pr_urls: &[String]) {
    for url in pr_urls {
        if let Err(e) = open::that(url) {
            eprintln!("Warning: Failed to open {}: {}", url, e);
        }
    }
}

// =============================================================================
// Helpers
// =============================================================================

fn extract_commit_body(message: &str) -> String {
    let lines: Vec<&str> = message.lines().collect();
    if lines.len() <= 1 {
        return String::new();
    }
    lines.into_iter()
        .skip(1)
        .skip_while(|l| l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}
