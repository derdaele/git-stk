use anyhow::{bail, Context, Result};
use git2::{Oid, Repository};
use crate::model::Stack;

/// Resolve a commit reference to an OID using the stack
///
/// Supports:
/// - SHA (full or short): "abc123" or full SHA
/// - Stack index: "1", "2", "3", ... (1-indexed, where 1 is the bottom of the stack)
/// - "last": the last (top) commit in the stack
pub fn resolve_commit_ref(git_repo: &Repository, stack: &Stack, commit_ref: &str) -> Result<Oid> {
    if commit_ref == "last" {
        if stack.is_empty() {
            bail!("No commits in stack");
        }
        return Ok(stack.entries.last().unwrap().oid);
    }

    if let Ok(index) = commit_ref.parse::<usize>() {
        if index == 0 {
            bail!("Stack index must be 1 or greater (1 is the bottom of the stack)");
        }
        if stack.is_empty() {
            bail!("No commits in stack");
        }
        if index > stack.entries.len() {
            bail!(
                "Stack index {} is out of range. Stack has {} commit{}",
                index,
                stack.entries.len(),
                if stack.entries.len() == 1 { "" } else { "s" }
            );
        }
        return Ok(stack.entries[index - 1].oid);
    }

    // Try to resolve as a git reference (SHA, branch name, HEAD, etc.)
    Ok(git_repo
        .revparse_single(commit_ref)
        .with_context(|| format!("Failed to resolve commit reference: {}", commit_ref))?
        .id())
}
