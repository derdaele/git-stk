use anyhow::{Context, Result};
use git2::Repository;
use std::path::PathBuf;

/// Configuration for git-stk
#[derive(Debug, Clone)]
pub struct Config {
    /// Base branch (e.g., "main")
    pub base: String,
    /// Remote name (e.g., "origin")
    pub remote: String,
    /// Notes ref
    pub notes_ref: String,
    /// Timeout in minutes when waiting for PR merge (default: 20)
    pub land_timeout_minutes: u64,
}

impl Config {
    /// Load configuration from git config and defaults
    pub fn load(repo: &Repository) -> Result<Self> {
        let git_config = repo.config().context("Failed to load git config")?;

        // Try to get base from git config, fallback to repo default branch
        let base = git_config
            .get_string("git-stk.base")
            .ok()
            .or_else(|| {
                // Try to get the default branch from origin/HEAD
                repo.find_reference("refs/remotes/origin/HEAD")
                    .ok()
                    .and_then(|r| {
                        r.symbolic_target()
                            .and_then(|t| t.strip_prefix("refs/remotes/origin/"))
                            .map(String::from)
                    })
            })
            .unwrap_or_else(|| "main".to_string());

        let remote = git_config
            .get_string("git-stk.remote")
            .unwrap_or_else(|_| "origin".to_string());

        let notes_ref = git_config
            .get_string("git-stk.notesRef")
            .unwrap_or_else(|_| "refs/notes/git-stk".to_string());

        let land_timeout_minutes = git_config
            .get_i64("git-stk.landTimeoutMinutes")
            .ok()
            .map(|v| v as u64)
            .unwrap_or(20);

        Ok(Self {
            base,
            remote,
            notes_ref,
            land_timeout_minutes,
        })
    }

    /// Get the git-stk state directory path
    pub fn git_stack_dir(repo: &Repository) -> Result<PathBuf> {
        let git_dir = repo
            .path()
            .parent()
            .context("Failed to get git dir parent")?
            .join(".git")
            .join("git-stk");

        Ok(git_dir)
    }

    /// Get the slots cache file path
    pub fn slots_cache_path(repo: &Repository) -> Result<PathBuf> {
        let dir = Self::git_stack_dir(repo)?;
        Ok(dir.join("slots.json"))
    }

    /// Ensure git notes rewriting is configured for the repository
    /// This allows notes to follow commits during rebase, amend, and reorder operations
    pub fn ensure_notes_rewrite_config(repo: &Repository, notes_ref: &str) -> Result<()> {
        let config = repo.config().context("Failed to load git config")?;
        let mut local_config = config
            .open_level(git2::ConfigLevel::Local)
            .context("Failed to open local git config for writing")?;

        // Set notes.rewriteRef to our notes ref if not already set
        let rewrite_ref_key = "notes.rewriteRef";

        // Collect all existing values to check if ours is already there
        let mut existing_values = Vec::new();
        if let Ok(mut entries) = local_config.entries(Some(rewrite_ref_key)) {
            while let Some(entry) = entries.next() {
                if let Ok(entry) = entry {
                    if let Some(value) = entry.value() {
                        existing_values.push(value.to_string());
                    }
                }
            }
        }

        // Only add if not already present
        if !existing_values.contains(&notes_ref.to_string()) {
            // Use set_multivar with "^$" pattern to add a new entry without matching existing ones
            local_config
                .set_multivar(rewrite_ref_key, "^$", notes_ref)
                .context("Failed to set notes.rewriteRef")?;
        }

        // Enable rebase note rewriting
        if local_config.get_bool("notes.rewrite.rebase").unwrap_or(false) == false {
            local_config
                .set_bool("notes.rewrite.rebase", true)
                .context("Failed to set notes.rewrite.rebase")?;
        }

        // Enable amend note rewriting
        if local_config.get_bool("notes.rewrite.amend").unwrap_or(false) == false {
            local_config
                .set_bool("notes.rewrite.amend", true)
                .context("Failed to set notes.rewrite.amend")?;
        }

        Ok(())
    }
}
