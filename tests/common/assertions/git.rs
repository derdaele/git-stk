#![allow(dead_code)]

use anyhow::{Context, Result};
use git2::{Oid, Repository};
use std::path::Path;

/// Assertion builder for git branches
pub struct BranchAssertion {
    repo_path: std::path::PathBuf,
    branch: String,
}

impl BranchAssertion {
    pub fn new(repo_path: &Path, branch: &str) -> Self {
        Self {
            repo_path: repo_path.to_path_buf(),
            branch: branch.to_string(),
        }
    }

    /// Assert branch points to a specific commit
    pub fn points_to(&self, expected: &str) -> Result<&Self> {
        let repo = Repository::open(&self.repo_path)
            .context("Failed to open repository")?;

        let branch_ref = repo
            .find_reference(&format!("refs/heads/{}", self.branch))
            .with_context(|| format!("Branch '{}' not found", self.branch))?;

        let actual_oid = branch_ref
            .target()
            .context("Branch has no target")?;

        // Parse expected - could be full OID or short SHA
        let expected_oid = if expected.len() == 40 {
            // Full OID
            Oid::from_str(expected)
                .with_context(|| format!("Invalid OID: {}", expected))?
        } else {
            // Short SHA - need to find the full OID
            repo.revparse_single(expected)
                .with_context(|| format!("Cannot resolve commit: {}", expected))?
                .id()
        };

        assert_eq!(
            actual_oid, expected_oid,
            "Expected branch '{}' to point to {}, but found {}",
            self.branch,
            expected_oid,
            actual_oid
        );

        Ok(self)
    }

    /// Assert branch exists
    pub fn exists(&self) -> Result<&Self> {
        let repo = Repository::open(&self.repo_path)
            .context("Failed to open repository")?;

        let exists = repo
            .find_reference(&format!("refs/heads/{}", self.branch))
            .is_ok();

        assert!(
            exists,
            "Expected branch '{}' to exist, but it doesn't",
            self.branch
        );

        Ok(self)
    }
}

/// Assertion builder for commits (reverse of BranchAssertion)
pub struct CommitAssertion {
    repo_path: std::path::PathBuf,
    commit: String,
}

impl CommitAssertion {
    pub fn new(repo_path: &Path, commit: &str) -> Self {
        Self {
            repo_path: repo_path.to_path_buf(),
            commit: commit.to_string(),
        }
    }

    /// Assert commit has a specific branch pointing to it
    pub fn has_branch(&self, branch: &str) -> Result<&Self> {
        // Use BranchAssertion in reverse
        BranchAssertion::new(&self.repo_path, branch)
            .points_to(&self.commit)?;

        Ok(self)
    }
}
