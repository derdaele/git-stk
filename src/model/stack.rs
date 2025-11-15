use git2::Oid;
use serde::{Deserialize, Serialize};

/// Status of a stack entry relative to GitHub
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateStatus {
    /// Entry is up-to-date with remote
    UpToDate,
    /// Entry exists but needs to be updated
    NeedsUpdate,
    /// PR needs to be created for this entry
    CreatePr,
}

/// PR state from GitHub
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrState {
    Open,
    Closed,
    Merged,
    Draft,
}

/// Metadata stored in git notes for each commit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitMetadata {
    /// PR number
    pub pr: Option<u64>,
    /// Slot identifier (e.g., "01", "02", or custom like "add-tests")
    /// The head ref name can be derived as {current_branch}--{slot}
    pub slot: String,
}

/// A single entry in the stack
#[derive(Debug, Clone)]
pub struct Entry {
    /// Position in the stack (1-indexed)
    pub index: usize,
    /// Git commit OID
    pub oid: Oid,
    /// Commit short SHA (for display)
    pub short_sha: String,
    /// Commit subject (first line of message)
    pub subject: String,
    /// Head ref name (branch name for this PR)
    pub head_ref: Option<String>,
    /// PR number if it exists
    pub pr_number: Option<u64>,
    /// PR state if PR exists
    pub pr_state: Option<PrState>,
    /// Update status
    pub status: UpdateStatus,
    /// Base branch for this PR (previous PR's head or repo base)
    pub base_ref: String,
    /// Remote OID if different from local (indicates divergence)
    pub remote_oid: Option<Oid>,
    /// Assigned slot from metadata
    pub slot: Option<String>,
    /// Predicted slot if no metadata exists
    pub predicted_slot: Option<String>,
    /// Whether remote branch exists
    pub remote_branch_exists: bool,
    /// Whether commit is merged into main
    pub merged_into_main: bool,
    /// Repository owner (for PR links)
    pub repo_owner: Option<String>,
    /// Repository name (for PR links)
    pub repo_name: Option<String>,
}

/// The complete stack of commits
#[derive(Debug, Clone)]
pub struct Stack {
    /// Base branch name (e.g., "main")
    pub base_branch: String,
    /// Current working branch name
    pub current_branch: String,
    /// All entries in order (bottom to top)
    pub entries: Vec<Entry>,
}

impl Stack {
    pub fn new(base_branch: String, current_branch: String) -> Self {
        Self {
            base_branch,
            current_branch,
            entries: Vec::new(),
        }
    }

    pub fn add_entry(&mut self, entry: Entry) {
        self.entries.push(entry);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
