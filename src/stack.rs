//! Unified stack discovery with full remote and GitHub state hydration.

use anyhow::{anyhow, Context, Result};
use git2::{Oid, Repository};
use octocrab::Octocrab;
use std::collections::HashMap;

use crate::gh::{client, queries};
use crate::git::{notes, refs, slots};
use crate::model::{Config, Entry, PrState, Stack, UpdateStatus};

/// Discover the stack with full hydration from remote refs and GitHub PR state.
///
/// This is the canonical way to get a complete view of the stack state.
/// It fetches remote refs and PR states in parallel for efficiency.
pub async fn discover_stack(
    git_repo: &Repository,
    config: &Config,
    gh_client: &Octocrab,
) -> Result<Stack> {
    // Derive owner/repo from remote URL
    let (owner, repo_name) = get_repo_info(git_repo, config)?;

    // Phase 1: Walk commits and load metadata from git notes
    let mut stack = walk_commits(git_repo, config)?;

    if stack.entries.is_empty() {
        return Ok(stack);
    }

    // Phase 2: Fetch remote refs and PR states in parallel
    let pr_numbers: Vec<u64> = stack.entries.iter().filter_map(|e| e.pr_number).collect();

    let (remote_refs, pr_states) =
        fetch_remote_and_pr_states(git_repo, config, gh_client, &owner, &repo_name, &pr_numbers)
            .await?;

    // Phase 3: Hydrate entries with fetched data
    hydrate_entries(
        &mut stack,
        git_repo,
        &owner,
        &repo_name,
        &remote_refs,
        &pr_states,
    )?;

    // Phase 4: Set up PR chain (base_ref for each entry)
    setup_pr_chain(&mut stack, config);

    Ok(stack)
}

// =============================================================================
// Private helper functions
// =============================================================================

/// Extract owner and repo name from git remote URL
fn get_repo_info(git_repo: &Repository, config: &Config) -> Result<(String, String)> {
    let remote = git_repo
        .find_remote(&config.remote)
        .with_context(|| format!("Failed to find remote: {}", config.remote))?;
    let remote_url = remote.url().context("Remote URL is not valid UTF-8")?;
    client::parse_repo_from_url(remote_url)
}

/// Walk commits from HEAD to base and load metadata from git notes
fn walk_commits(repo: &Repository, config: &Config) -> Result<Stack> {
    let head = repo.head().context("Failed to get HEAD")?;
    let current_branch = head
        .shorthand()
        .context("Failed to get current branch name")?
        .to_string();

    let base_ref_name = format!("refs/heads/{}", config.base);
    let base_commit = repo
        .find_reference(&base_ref_name)
        .with_context(|| format!("Failed to find base branch: {}", config.base))?
        .peel_to_commit()
        .context("Failed to resolve base branch to commit")?;

    let head_commit = head.peel_to_commit().context("Failed to resolve HEAD")?;

    // Empty stack if on base branch
    if head_commit.id() == base_commit.id() {
        return Ok(Stack::new(config.base.clone(), current_branch));
    }

    // Walk commits
    let mut revwalk = repo.revwalk().context("Failed to create revwalk")?;
    revwalk.push(head_commit.id())?;
    revwalk.hide(base_commit.id())?;
    revwalk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::REVERSE)?;

    let mut stack = Stack::new(config.base.clone(), current_branch.clone());

    for (index, oid_result) in revwalk.enumerate() {
        let oid = oid_result.context("Failed to walk commit")?;
        let commit = repo.find_commit(oid).context("Failed to find commit")?;

        if commit.parent_count() > 1 {
            return Err(anyhow!(
                "Merge commits are not supported. Found: {}",
                oid
            ));
        }

        let entry = create_entry_from_commit(index, &commit, config);
        stack.add_entry(entry);
    }

    // Load metadata from git notes
    load_metadata_from_notes(repo, &mut stack, config)?;

    Ok(stack)
}

/// Create a stack entry from a git commit
fn create_entry_from_commit(index: usize, commit: &git2::Commit, config: &Config) -> Entry {
    Entry {
        index: index + 1,
        oid: commit.id(),
        short_sha: format!("{:.7}", commit.id()),
        subject: commit.summary().unwrap_or("<no subject>").to_string(),
        head_ref: None,
        pr_number: None,
        pr_state: None,
        status: UpdateStatus::CreatePr,
        base_ref: if index == 0 {
            config.base.clone()
        } else {
            "unknown".to_string()
        },
        remote_oid: None,
        slot: None,
        predicted_slot: None,
        remote_branch_exists: false,
        merged_into_main: false,
        repo_owner: None,
        repo_name: None,
    }
}

/// Load PR numbers and slots from git notes into stack entries
fn load_metadata_from_notes(
    repo: &Repository,
    stack: &mut Stack,
    config: &Config,
) -> Result<()> {
    let current_branch = stack.current_branch.clone();

    for entry in &mut stack.entries {
        if let Some(metadata) = notes::read_note(repo, entry.oid, &config.notes_ref)? {
            entry.pr_number = metadata.pr;
            entry.slot = Some(metadata.slot.clone());
            entry.head_ref = Some(slots::generate_head_ref(&current_branch, &metadata.slot));
        }
    }

    Ok(())
}

/// Fetch remote refs and PR states in parallel
async fn fetch_remote_and_pr_states(
    git_repo: &Repository,
    config: &Config,
    gh_client: &Octocrab,
    owner: &str,
    repo_name: &str,
    pr_numbers: &[u64],
) -> Result<(HashMap<String, Oid>, HashMap<u64, queries::PrInfo>)> {
    let remote_name = config.remote.clone();
    let git_repo_path = git_repo.path().to_path_buf();

    // Spawn remote refs fetch as blocking task
    let remote_refs_task = tokio::task::spawn_blocking(move || {
        let repo = Repository::open(&git_repo_path)?;
        refs::get_all_remote_refs(&repo, &remote_name)
    });

    // Fetch PR states from GitHub
    let owner_clone = owner.to_string();
    let repo_name_clone = repo_name.to_string();
    let pr_numbers_clone = pr_numbers.to_vec();

    let pr_states_task = async move {
        if pr_numbers_clone.is_empty() {
            Ok(HashMap::new())
        } else {
            queries::get_prs_batch(gh_client, &owner_clone, &repo_name_clone, &pr_numbers_clone)
                .await
        }
    };

    let (remote_refs_result, pr_states_result) = tokio::join!(remote_refs_task, pr_states_task);

    let remote_refs = remote_refs_result
        .context("Remote refs task panicked")?
        .context("Failed to fetch remote refs")?;

    let pr_states = pr_states_result.context("Failed to fetch PR states from GitHub")?;

    Ok((remote_refs, pr_states))
}

/// Hydrate stack entries with remote and PR state information
fn hydrate_entries(
    stack: &mut Stack,
    git_repo: &Repository,
    owner: &str,
    repo_name: &str,
    remote_refs: &HashMap<String, Oid>,
    pr_states: &HashMap<u64, queries::PrInfo>,
) -> Result<()> {
    let mut slot_cache = slots::SlotCache::load(git_repo)?;

    for entry in &mut stack.entries {
        entry.repo_owner = Some(owner.to_string());
        entry.repo_name = Some(repo_name.to_string());

        // Hydrate remote ref status
        hydrate_remote_status(entry, remote_refs, &mut slot_cache, &stack.current_branch);

        // Hydrate PR state from GitHub
        hydrate_pr_state(entry, pr_states);
    }

    Ok(())
}

/// Update entry with remote branch status
fn hydrate_remote_status(
    entry: &mut Entry,
    remote_refs: &HashMap<String, Oid>,
    slot_cache: &mut slots::SlotCache,
    current_branch: &str,
) {
    if let Some(head_ref) = &entry.head_ref {
        if let Some(&remote_oid) = remote_refs.get(head_ref.as_str()) {
            entry.remote_branch_exists = true;
            entry.remote_oid = Some(remote_oid);
            entry.status = if remote_oid == entry.oid {
                UpdateStatus::UpToDate
            } else {
                UpdateStatus::NeedsUpdate
            };
        } else {
            entry.remote_branch_exists = false;
            entry.status = UpdateStatus::CreatePr;
        }
    } else {
        // No metadata - predict slot
        entry.predicted_slot = Some(slot_cache.allocate(current_branch));
        entry.status = UpdateStatus::CreatePr;
        entry.remote_branch_exists = false;
    }
}

/// Update entry with PR state from GitHub
fn hydrate_pr_state(entry: &mut Entry, pr_states: &HashMap<u64, queries::PrInfo>) {
    if let Some(pr_number) = entry.pr_number {
        if let Some(pr_info) = pr_states.get(&pr_number) {
            entry.pr_state = Some(pr_info.state.clone());

            if pr_info.state == PrState::Merged {
                entry.merged_into_main = true;
            }
        }
    }
}

/// Set up PR chain by updating base_ref for each entry
fn setup_pr_chain(stack: &mut Stack, config: &Config) {
    for i in 1..stack.entries.len() {
        let prev_head_ref = stack.entries[i - 1].head_ref.clone();
        stack.entries[i].base_ref = prev_head_ref.unwrap_or_else(|| config.base.clone());
    }
}
