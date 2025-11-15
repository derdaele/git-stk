use anyhow::{anyhow, Context, Result};
use octocrab::Octocrab;
use std::process::Command;

/// Get GitHub token from gh CLI
fn get_gh_token() -> Result<String> {
    let output = Command::new("gh")
        .args(["auth", "token"])
        .output()
        .context("Failed to execute 'gh auth token'. Is GitHub CLI installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "gh auth token failed: {}. Please run 'gh auth login'",
            stderr
        ));
    }

    let token = String::from_utf8(output.stdout)
        .context("Failed to parse gh token output")?
        .trim()
        .to_string();

    if token.is_empty() {
        return Err(anyhow!(
            "gh auth token returned empty token. Please run 'gh auth login'"
        ));
    }

    Ok(token)
}

/// Create an authenticated Octocrab instance
pub fn create_client() -> Result<Octocrab> {
    let token = get_gh_token()?;

    Octocrab::builder()
        .personal_token(token)
        .build()
        .context("Failed to create GitHub client")
}

/// Parse owner and repo from a remote URL
pub fn parse_repo_from_url(url: &str) -> Result<(String, String)> {
    // Handle both HTTPS and SSH URLs
    // HTTPS: https://github.com/owner/repo.git
    // SSH: git@github.com:owner/repo.git
    // file:// URLs are used in tests and return default test values

    let url = url.trim();

    let (owner, repo) = if url.starts_with("file://") {
        // Test URL - return default test owner/repo
        ("test-owner".to_string(), "test-repo".to_string())
    } else if url.starts_with("https://") || url.starts_with("http://") {
        // HTTPS URL
        let parts: Vec<&str> = url.split('/').collect();
        if parts.len() < 5 {
            return Err(anyhow!("Invalid GitHub URL: {}", url));
        }
        let owner = parts[parts.len() - 2];
        let repo = parts[parts.len() - 1].trim_end_matches(".git");
        (owner.to_string(), repo.to_string())
    } else if url.starts_with("git@") {
        // SSH URL: git@<host>:owner/repo.git
        // Extract everything after the colon to support custom GitHub hosts
        let colon_pos = url.find(':')
            .context("Invalid SSH URL format: missing ':'")?;
        let after_colon = &url[colon_pos + 1..];
        let parts: Vec<&str> = after_colon.split('/').collect();
        if parts.len() < 2 {
            return Err(anyhow!("Invalid GitHub SSH URL: {}", url));
        }
        let owner = parts[0];
        let repo = parts[parts.len() - 1].trim_end_matches(".git");
        (owner.to_string(), repo.to_string())
    } else {
        return Err(anyhow!("Unsupported remote URL format: {}", url));
    };

    Ok((owner, repo))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ssh_url() {
        let url = "git@github.com:cohere-ai/north.git";
        let (owner, repo) = parse_repo_from_url(url).unwrap();
        assert_eq!(owner, "cohere-ai");
        assert_eq!(repo, "north");
    }

    #[test]
    fn test_parse_ssh_url_custom_host() {
        let url = "git@github-work.com:my-org/my-repo.git";
        let (owner, repo) = parse_repo_from_url(url).unwrap();
        assert_eq!(owner, "my-org");
        assert_eq!(repo, "my-repo");
    }

    #[test]
    fn test_parse_https_url() {
        let url = "https://github.com/cohere-ai/north.git";
        let (owner, repo) = parse_repo_from_url(url).unwrap();
        assert_eq!(owner, "cohere-ai");
        assert_eq!(repo, "north");
    }

    #[test]
    fn test_parse_file_url() {
        let url = "file:///path/to/repo";
        let (owner, repo) = parse_repo_from_url(url).unwrap();
        assert_eq!(owner, "test-owner");
        assert_eq!(repo, "test-repo");
    }
}
