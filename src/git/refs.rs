use anyhow::{anyhow, Context, Result};
use git2::{Oid, Repository};
use std::collections::HashMap;
use std::process::{Command, Stdio};

/// Get all remote refs in a single connection
/// Returns a map of ref_name -> oid (without "refs/heads/" prefix)
pub fn get_all_remote_refs(repo: &Repository, remote_name: &str) -> Result<HashMap<String, Oid>> {
    let repo_path = repo
        .workdir()
        .ok_or_else(|| anyhow!("Repository has no working directory"))?;

    // Use git ls-remote to list all remote refs
    let output = Command::new("git")
        .current_dir(repo_path)
        .arg("ls-remote")
        .arg("--heads")
        .arg(remote_name)
        .output()
        .context("Failed to execute git ls-remote")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Failed to list remote refs: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut remote_refs: HashMap<String, Oid> = HashMap::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let oid_str = parts[0];
            let ref_name = parts[1];

            if let Some(branch_name) = ref_name.strip_prefix("refs/heads/") {
                if let Ok(oid) = Oid::from_str(oid_str) {
                    remote_refs.insert(branch_name.to_string(), oid);
                }
            }
        }
    }

    Ok(remote_refs)
}

/// Result of pushing refs to remote
#[derive(Debug, Clone)]
pub struct PushResult {
    /// Whether the push succeeded
    pub success: bool,
    /// Refs that were successfully pushed
    pub pushed_refs: Vec<String>,
    /// Error message if any
    pub error: Option<String>,
}

/// Capability cache for remote features
#[derive(Debug, Clone, Default)]
pub struct RemoteCapabilities {
    /// Whether the remote supports atomic pushes
    pub supports_atomic: Option<bool>,
}

impl RemoteCapabilities {
    /// Detect if remote supports atomic push
    pub fn detect_atomic_support(
        repo: &Repository,
        remote: &str,
        refspecs: &[String],
    ) -> Result<bool> {
        if refspecs.is_empty() {
            return Ok(false);
        }

        let repo_path = repo
            .workdir()
            .ok_or_else(|| anyhow!("Repository has no working directory"))?;

        // Try a dry-run atomic push
        let mut cmd = Command::new("git");
        cmd.current_dir(repo_path)
            .arg("push")
            .arg("--atomic")
            .arg("--dry-run")
            .arg("--porcelain")
            .arg(remote)
            .arg(&refspecs[0]) // Just test with one refspec
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = cmd.output().context("Failed to execute git push dry-run")?;

        // Check stderr for atomic support error
        let stderr = String::from_utf8_lossy(&output.stderr);

        if stderr.contains("does not support --atomic")
            || stderr.contains("support for the --atomic option")
            || stderr.contains("atomic push") {
            Ok(false)
        } else {
            // If no error about atomic, assume it's supported
            Ok(true)
        }
    }
}

/// Push refs to remote with atomic support (preferred) or fallback
/// Always uses --force for pushing
pub fn push_refs(
    repo: &Repository,
    remote: &str,
    refspecs: &[String],
    capabilities: &mut RemoteCapabilities,
) -> Result<PushResult> {
    if refspecs.is_empty() {
        return Ok(PushResult {
            success: true,
            pushed_refs: vec![],
            error: None,
        });
    }

    // Detect atomic support if not cached
    if capabilities.supports_atomic.is_none() {
        let supports = RemoteCapabilities::detect_atomic_support(repo, remote, refspecs)?;
        capabilities.supports_atomic = Some(supports);
    }

    let supports_atomic = capabilities.supports_atomic.unwrap_or(false);

    if supports_atomic {
        // Try atomic push
        push_atomic(repo, remote, refspecs)
    } else {
        // Fallback to top-down individual pushes
        push_top_down(repo, remote, refspecs)
    }
}

/// Push all refs atomically (all succeed or all fail)
/// Always uses --force
fn push_atomic(
    repo: &Repository,
    remote: &str,
    refspecs: &[String],
) -> Result<PushResult> {
    let repo_path = repo
        .workdir()
        .ok_or_else(|| anyhow!("Repository has no working directory"))?;

    let mut cmd = Command::new("git");
    cmd.current_dir(repo_path)
        .arg("push")
        .arg("--atomic")
        .arg("--force")
        .arg("--porcelain");

    cmd.arg(remote);

    for refspec in refspecs {
        cmd.arg(refspec);
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let output = cmd
        .output()
        .context("Failed to execute git push --atomic")?;

    if output.status.success() {
        Ok(PushResult {
            success: true,
            pushed_refs: refspecs.to_vec(),
            error: None,
        })
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Ok(PushResult {
            success: false,
            pushed_refs: vec![],
            error: Some(stderr.to_string()),
        })
    }
}

/// Push refs one at a time, top-down (highest slot first)
/// Always uses --force
fn push_top_down(
    repo: &Repository,
    remote: &str,
    refspecs: &[String],
) -> Result<PushResult> {
    let repo_path = repo
        .workdir()
        .ok_or_else(|| anyhow!("Repository has no working directory"))?;

    let mut pushed = Vec::new();
    let mut errors = Vec::new();

    // Sort refspecs in reverse order (highest slot first)
    // This ensures parent PRs are pushed before children
    let mut sorted_refspecs = refspecs.to_vec();
    sorted_refspecs.sort_by(|a, b| b.cmp(a));

    for refspec in sorted_refspecs {
        let mut cmd = Command::new("git");
        cmd.current_dir(repo_path)
            .arg("push")
            .arg("--force")
            .arg("--porcelain");

        cmd.arg(remote).arg(&refspec);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let output = cmd
            .output()
            .with_context(|| format!("Failed to execute git push for {}", refspec))?;

        if output.status.success() {
            pushed.push(refspec.clone());
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            errors.push(format!("{}: {}", refspec, stderr));
        }
    }

    if errors.is_empty() {
        Ok(PushResult {
            success: true,
            pushed_refs: pushed,
            error: None,
        })
    } else {
        Ok(PushResult {
            success: false,
            pushed_refs: pushed,
            error: Some(errors.join("\n")),
        })
    }
}

/// Build refspecs for pushing commits directly to remote refs (no local branch needed)
/// Format: {oid}:refs/heads/{head_ref}
pub fn build_refspecs_from_oids(commits: &[(Oid, String)]) -> Vec<String> {
    commits
        .iter()
        .map(|(oid, head_ref)| format!("{}:refs/heads/{}", oid, head_ref))
        .collect()
}

/// Check which commits need to be pushed by comparing commit OID vs remote ref
/// Returns a map of head_ref -> (needs_push, remote_oid)
pub fn check_commits_to_push(
    repo: &Repository,
    remote_name: &str,
    commits: &[(Oid, String)], // (commit_oid, head_ref)
) -> Result<HashMap<String, (bool, Option<Oid>)>> {
    let mut result = HashMap::new();

    // Get all remote refs using git ls-remote
    let remote_refs = get_all_remote_refs(repo, remote_name)?;

    for (commit_oid, head_ref) in commits {
        let remote_oid = remote_refs.get(head_ref).copied();

        let needs_push = match remote_oid {
            None => true, // Remote ref doesn't exist
            Some(remote) => *commit_oid != remote, // Commit differs from remote
        };

        result.insert(head_ref.clone(), (needs_push, remote_oid));
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_refspecs_from_oids() {
        let oid1 = Oid::from_str("1234567890abcdef1234567890abcdef12345678").unwrap();
        let oid2 = Oid::from_str("abcdef1234567890abcdef1234567890abcdef12").unwrap();

        let commits = vec![
            (oid1, "feature/foo/s001".to_string()),
            (oid2, "feature/foo/s002".to_string()),
        ];

        let refspecs = build_refspecs_from_oids(&commits);

        assert_eq!(refspecs.len(), 2);
        assert_eq!(
            refspecs[0],
            "1234567890abcdef1234567890abcdef12345678:refs/heads/feature/foo/s001"
        );
        assert_eq!(
            refspecs[1],
            "abcdef1234567890abcdef1234567890abcdef12:refs/heads/feature/foo/s002"
        );
    }
}
