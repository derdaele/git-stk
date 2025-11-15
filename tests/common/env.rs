use super::assertions::{BranchAssertion, CommitAssertion, GithubAssertion, ViewAssertion};
use super::helpers::{cleanup_test_branches, run_git_stk_command};
use super::repo::TempGitRepo;
use anyhow::{Context, Result};
use git2::Repository;
use std::path::Path;
use std::process::Command;

/// Test environment with full integration test setup
pub struct TestEnv {
    pub test_id: String,
    pub repo: TempGitRepo,
    owner: String,
    repo_name: String,
}

impl TestEnv {
    /// Set up a complete test environment
    pub fn setup() -> Result<Self> {
        // Generate unique test identifier using random hex
        // This ensures uniqueness even when tests run in parallel
        let random: u32 = rand::random();
        let test_id = format!("test-{:08x}", random);

        println!("Test ID: {}\n", test_id);

        // Clone repository
        println!("Setting up test environment...");
        let repo = TempGitRepo::clone_test_repo()?;
        println!("  ✓ Cloned repository");

        // Create base branch from main
        let base_branch = format!("{}-base", test_id);
        Command::new("git")
            .current_dir(repo.path())
            .args(["checkout", "main"])
            .output()?;

        Command::new("git")
            .current_dir(repo.path())
            .args(["checkout", "-b", &base_branch])
            .output()?;

        Command::new("git")
            .current_dir(repo.path())
            .args(["push", "origin", &base_branch])
            .output()?;
        println!("  ✓ Created base branch: {}", base_branch);

        // Configure git-stack to use test base
        repo.set_config("git-stk.remote", "origin")?;
        repo.set_config("git-stk.base", &base_branch)?;
        println!("  ✓ Configured git-stk.base = {}", base_branch);

        // Create feature branch from base
        let feature_branch = format!("{}-feature", test_id);
        Command::new("git")
            .current_dir(repo.path())
            .args(["checkout", "-b", &feature_branch])
            .output()?;
        println!("  ✓ Created feature branch: {}\n", feature_branch);

        // Get owner and repo name from remote URL
        let (owner, repo_name) = Self::get_repo_info(repo.path())?;

        Ok(Self {
            test_id,
            repo,
            owner,
            repo_name,
        })
    }

    /// Get repository owner and name from git remote URL
    fn get_repo_info(repo_path: &Path) -> Result<(String, String)> {
        let git_repo = Repository::open(repo_path)?;
        let remote = git_repo.find_remote("origin")?;
        let url = remote.url().context("Remote URL is not valid UTF-8")?;

        // Parse GitHub URL (e.g., "https://github.com/owner/repo.git")
        git_stk::gh::client::parse_repo_from_url(url)
    }

    /// Clean up all test branches (called automatically on Drop)
    #[allow(dead_code)]
    pub fn teardown(&self) -> Result<()> {
        println!("\nCleaning up test environment...");
        cleanup_test_branches(self.repo.path(), &self.test_id)?;
        Ok(())
    }

    /// Internal cleanup that doesn't return Result (for Drop)
    fn cleanup(&self) {
        println!("\nCleaning up test environment...");
        if let Err(e) = cleanup_test_branches(self.repo.path(), &self.test_id) {
            eprintln!("⚠️  Warning: Failed to cleanup test branches: {}", e);
        }
    }

    /// Get repository path
    pub fn path(&self) -> &Path {
        self.repo.path()
    }

    // === Assertion Methods ===

    /// Assert on git stk view output
    pub fn assert_view(&self) -> Result<ViewAssertion> {
        let output = run_git_stk_command(self.path(), &["view"])?;
        Ok(ViewAssertion::from_output(output))
    }

    /// Assert on GitHub PR state
    pub fn assert_github(&self) -> GithubAssertion {
        GithubAssertion::new(&self.owner, &self.repo_name)
    }

    /// Assert on git branch
    #[allow(dead_code)]
    pub fn assert_branch(&self, branch: &str) -> BranchAssertion {
        BranchAssertion::new(self.path(), branch)
    }

    /// Assert on git commit
    #[allow(dead_code)]
    pub fn assert_commit(&self, commit: &str) -> CommitAssertion {
        CommitAssertion::new(self.path(), commit)
    }

    // === Command Methods ===

    /// Run git stk export with options
    pub fn export(&self, draft: bool) -> Result<String> {
        let mut args = vec!["export"];
        if draft {
            args.push("--draft");
        }
        run_git_stk_command(self.path(), &args)
    }

    /// Run git stk export with default options
    pub fn export_default(&self) -> Result<String> {
        self.export(false)
    }

    /// Run git stk land to merge PRs
    pub fn land(&self) -> Result<String> {
        run_git_stk_command(self.path(), &["land"])
    }

    /// Run git stk landed to detect and clean up externally merged commits
    pub fn landed(&self) -> Result<String> {
        run_git_stk_command(self.path(), &["landed"])
    }

    /// Run git stk set slot to assign a slot to a commit
    pub fn set_slot(&self, commit: &str, slot: &str) -> Result<String> {
        run_git_stk_command(self.path(), &["set", "slot", commit, slot])
    }

    /// Run git stk set slot with --yes flag to skip confirmation
    pub fn set_slot_yes(&self, commit: &str, slot: &str) -> Result<String> {
        run_git_stk_command(self.path(), &["set", "slot", commit, slot, "--yes"])
    }

    /// Run git stk set slot without --yes flag (will fail in non-interactive mode)
    /// This is used to test that the command respects the user saying "no"
    pub fn set_slot_no_confirm(&self, commit: &str, slot: &str) -> Result<String> {
        run_git_stk_command(self.path(), &["set", "slot", commit, slot])
    }

    /// Modify a file on a remote branch using gh cli (simulates external commit)
    pub fn modify_remote_branch(&self, slot: &str, filename: &str, content: &str) -> Result<()> {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        use std::process::Command;

        let branch = format!("{}-feature--{}", self.test_id, slot);

        // Get the current file SHA (needed for update)
        let get_output = Command::new("gh")
            .args([
                "api",
                &format!("/repos/{}/{}/contents/{}", self.owner, self.repo_name, filename),
                "-q", ".sha",
                "-H", &format!("ref: {}", branch),
            ])
            .output()?;

        let file_sha = String::from_utf8_lossy(&get_output.stdout).trim().to_string();

        // Update the file
        let encoded_content = STANDARD.encode(content);

        let output = Command::new("gh")
            .args([
                "api",
                "--method", "PUT",
                &format!("/repos/{}/{}/contents/{}", self.owner, self.repo_name, filename),
                "-f", &format!("message=External modification"),
                "-f", &format!("content={}", encoded_content),
                "-f", &format!("branch={}", branch),
                "-f", &format!("sha={}", file_sha),
            ])
            .output()?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to modify remote branch: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        println!("  ✓ Modified {} on remote branch {}", filename, branch);
        Ok(())
    }

    /// Merge a PR on GitHub using the GitHub API (simulates external merge)
    pub async fn merge_pr_on_github(&self, slot: &str) -> Result<()> {
        use git_stk::gh::client;

        let client = client::create_client()?;
        let head_branch = format!("{}-feature--{}", self.test_id, slot);

        // Find the PR by head branch
        let pulls = client
            .pulls(&self.owner, &self.repo_name)
            .list()
            .state(octocrab::params::State::Open)
            .head(format!("{}:{}", self.owner, head_branch))
            .per_page(1)
            .send()
            .await?;

        if let Some(pr) = pulls.items.first() {
            // Merge the PR
            client
                .pulls(&self.owner, &self.repo_name)
                .merge(pr.number)
                .send()
                .await?;

            println!("  ✓ Merged PR #{} on GitHub", pr.number);

            // Wait a bit for GitHub to process the merge
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

            Ok(())
        } else {
            Err(anyhow::anyhow!("PR with head branch {} not found", head_branch))
        }
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        // Always cleanup, even if test panics
        self.cleanup();
    }
}
