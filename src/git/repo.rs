use anyhow::{anyhow, Context, Result};
use git2::Repository;

/// Get the current branch name
pub fn current_branch(repo: &Repository) -> Result<String> {
    let head = repo.head().context("Failed to get HEAD")?;

    if head.is_branch() {
        head.shorthand()
            .context("Failed to get branch name")
            .map(String::from)
    } else {
        Err(anyhow!("HEAD is not pointing to a branch (detached HEAD?)"))
    }
}

