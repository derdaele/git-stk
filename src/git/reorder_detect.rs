use crate::model::{CommitMetadata, Entry};
use git2::{Oid, Repository};
use std::collections::HashMap;

/// Information about commits that changed (rebased/reordered)
#[derive(Debug)]
pub struct ReorderInfo {
    /// Index of the highest stable commit (last commit where local SHA = remote SHA)
    /// None if all commits changed
    pub highest_stable_index: Option<usize>,
    /// Commits that changed (different SHA from remote)
    pub moved_commits: Vec<MovedCommit>,
}

#[derive(Debug)]
pub struct MovedCommit {
    /// Current index in stack (0-based)
    pub current_index: usize,
    /// Original slot identifier
    pub original_slot: String,
    /// Commit OID
    pub oid: Oid,
    /// PR number
    pub pr_number: u64,
    /// Current head ref
    pub head_ref: String,
}

/// Detect which commits changed by comparing local SHA vs remote SHA
/// This correctly identifies rebased/reordered commits without false positives
pub fn detect_reordering(
    repo: &Repository,
    remote_name: &str,
    current_branch: &str,
    entries: &[Entry],
    metadata_map: &HashMap<Oid, CommitMetadata>,
) -> ReorderInfo {
    let mut moved_commits = Vec::new();
    let mut highest_stable_index: Option<usize> = None;

    for (current_idx, entry) in entries.iter().enumerate() {
        if let Some(metadata) = metadata_map.get(&entry.oid) {
            // Derive head_ref from current_branch and slot
            let head_ref = crate::git::slots::generate_head_ref(current_branch, &metadata.slot);

            // Get the remote ref SHA for this commit's branch
            let remote_ref_name = format!("refs/remotes/{}/{}", remote_name, head_ref);
            let local_sha = entry.oid;
            let remote_sha = repo
                .find_reference(&remote_ref_name)
                .ok()
                .and_then(|r| r.target());

            let commit_changed = match remote_sha {
                Some(remote_oid) => local_sha != remote_oid, // SHAs differ = commit changed
                None => true, // No remote ref = new commit (treat as changed)
            };

            if commit_changed {
                // This commit changed (rebased/reordered/new)
                if let Some(pr_number) = metadata.pr {
                    moved_commits.push(MovedCommit {
                        current_index: current_idx,
                        original_slot: metadata.slot.clone(),
                        oid: entry.oid,
                        pr_number,
                        head_ref,
                    });
                }
            } else {
                // This commit unchanged (local SHA = remote SHA)
                highest_stable_index = Some(current_idx);
            }
        }
    }

    ReorderInfo {
        highest_stable_index,
        moved_commits,
    }
}

/// Calculate which PR base updates are needed for the 3-phase approach
pub fn calculate_base_updates(
    current_branch: &str,
    entries: &[Entry],
    reorder_info: &ReorderInfo,
    metadata_map: &HashMap<Oid, CommitMetadata>,
    base_branch: &str,
) -> (Vec<(u64, String)>, Vec<(u64, String)>) {
    let mut phase1_updates = Vec::new(); // Updates to stable nodes (before push)
    let mut phase3_updates = Vec::new(); // Updates to final chain (after push)

    // For each moved commit, determine stable base and final base
    for moved in &reorder_info.moved_commits {
        let current_idx = moved.current_index;

        // Find the stable node before this commit's current position
        let stable_base = if let Some(stable_idx) = reorder_info.highest_stable_index {
            if stable_idx < current_idx {
                // Use the stable commit's head ref derived from metadata
                if let Some(stable_entry) = entries.get(stable_idx) {
                    metadata_map
                        .get(&stable_entry.oid)
                        .map(|m| crate::git::slots::generate_head_ref(current_branch, &m.slot))
                } else {
                    None
                }
            } else {
                // No stable commit before this one, use base branch
                Some(base_branch.to_string())
            }
        } else {
            // No stable commits at all, use base branch
            Some(base_branch.to_string())
        };

        // Determine final base (the commit right before this one in the new order)
        let final_base = if current_idx == 0 {
            base_branch.to_string()
        } else if let Some(prev_entry) = entries.get(current_idx - 1) {
            // Derive head_ref from metadata
            metadata_map
                .get(&prev_entry.oid)
                .map(|m| crate::git::slots::generate_head_ref(current_branch, &m.slot))
                .unwrap_or_else(|| base_branch.to_string())
        } else {
            base_branch.to_string()
        };

        // Add Phase 1 update if stable base is different from current
        if let Some(stable) = stable_base {
            phase1_updates.push((moved.pr_number, stable.clone()));

            // Add Phase 3 update if final base is different from stable
            if final_base != stable {
                phase3_updates.push((moved.pr_number, final_base));
            }
        }
    }

    (phase1_updates, phase3_updates)
}
