use anyhow::Result;
use git2::Repository;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Helper for creating and managing temporary Git repositories for integration tests
pub struct TempGitRepo {
    _dir: TempDir,
    path: PathBuf,
}

/// Helper for creating a temporary git editor script
///
/// This is useful for git operations that need a custom editor (rebase, commit --amend, etc.)
/// The editor script will write the provided content to the file git passes as $1
struct GitEditor {
    content_path: PathBuf,
    script_path: PathBuf,
}

impl GitEditor {
    /// Create a new git editor that will write the given content
    fn new(repo_path: &Path, content: &str, prefix: &str) -> Result<Self> {
        // Write the content to a temporary file
        let content_path = repo_path.join(format!("{}-content.txt", prefix));
        std::fs::write(&content_path, content)?;

        // Create a shell script that copies the content to git's target file
        let script_path = repo_path.join(format!("{}-editor.sh", prefix));
        let script_content = format!(
            "#!/bin/sh\ncat {} > \"$1\"\n",
            content_path.display()
        );
        std::fs::write(&script_path, script_content)?;

        // Make it executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script_path, perms)?;
        }

        Ok(Self {
            content_path,
            script_path,
        })
    }

    /// Get the path to the editor script (for setting GIT_SEQUENCE_EDITOR or GIT_EDITOR)
    fn path(&self) -> &Path {
        &self.script_path
    }
}

impl Drop for GitEditor {
    fn drop(&mut self) {
        // Clean up temporary files
        let _ = std::fs::remove_file(&self.content_path);
        let _ = std::fs::remove_file(&self.script_path);
    }
}

impl TempGitRepo {
    /// Clone the test repository to a temporary directory
    pub fn clone_test_repo() -> Result<Self> {
        // Load .env file if it exists
        let _ = dotenvy::dotenv();

        // Get test repository URL from environment variable (required)
        let repo_url = std::env::var("TEST_REPO_URL")
            .expect("TEST_REPO_URL environment variable must be set to run integration tests");

        let dir = TempDir::new_in("/tmp")?;
        let path = dir.path().to_path_buf();

        // Set up authentication callbacks for private repos
        let mut callbacks = git2::RemoteCallbacks::new();
        callbacks.credentials(|_url, username_from_url, _allowed_types| {
            git2::Cred::credential_helper(
                &git2::Config::open_default()?,
                _url,
                username_from_url,
            )
        });

        let mut fetch_options = git2::FetchOptions::new();
        fetch_options.remote_callbacks(callbacks);

        let mut builder = git2::build::RepoBuilder::new();
        builder.fetch_options(fetch_options);

        // Clone the real test repository
        builder.clone(&repo_url, &path)?;

        Ok(Self { _dir: dir, path })
    }

    /// Get the repository path
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Get the repository object
    pub fn repo(&self) -> Result<Repository> {
        Ok(Repository::open(&self.path)?)
    }

    /// Create a commit with the given message
    /// Returns the short SHA of the created commit
    pub fn create_commit(&self, message: &str) -> Result<String> {
        let repo = self.repo()?;

        // Configure git user if not set
        let mut config = repo.config()?;
        if config.get_string("user.name").is_err() {
            config.set_str("user.name", "Test User")?;
        }
        if config.get_string("user.email").is_err() {
            config.set_str("user.email", "test@example.com")?;
        }

        // Create a dummy file change based on message hash for deterministic, non-conflicting files
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        message.hash(&mut hasher);
        let hash = hasher.finish();

        let file_name = format!("test-{:x}.txt", hash);
        let file_path = self.path.join(&file_name);
        std::fs::write(&file_path, format!("Test content for {}\n", message))?;

        // Stage the file
        let mut index = repo.index()?;
        index.add_path(Path::new(&file_name))?;
        index.write()?;

        // Create commit
        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        let parent = repo.head()?.peel_to_commit()?;
        let sig = repo.signature()?;

        let oid = repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            message,
            &tree,
            &[&parent],
        )?;

        // Return short SHA (7 characters)
        Ok(oid.to_string()[..7].to_string())
    }

    /// Set git config value
    pub fn set_config(&self, key: &str, value: &str) -> Result<()> {
        let repo = self.repo()?;
        let mut config = repo.config()?;
        config.set_str(key, value)?;
        Ok(())
    }

    /// Interactive rebase with custom operations
    /// Takes a list of (operation, sha) tuples where operation is "pick", "drop", "squash", etc.
    /// Optional commit_message is used when squashing (sets GIT_EDITOR)
    pub fn rebase(&self, operations: &[(&str, &str)], commit_message: Option<&str>) -> Result<()> {
        use std::process::Command;

        // Get the current HEAD and count commits
        let repo = self.repo()?;
        let head = repo.head()?.peel_to_commit()?;
        let num_commits = operations.len();

        // Find the base commit (parent of the first commit in our stack)
        let mut base_commit = head.clone();
        for _ in 0..num_commits {
            base_commit = base_commit.parent(0)?;
        }
        let base_oid = base_commit.id();

        // Build the rebase todo script with operations
        let mut todo_script = String::new();
        for (op, sha) in operations {
            // Find the full commit info by SHA prefix
            let full_oid = repo.revparse_single(sha)?.id();
            let commit = repo.find_commit(full_oid)?;
            let message = commit.message().unwrap_or("").trim();

            match *op {
                "pick" => todo_script.push_str(&format!("pick {} {}\n", sha, message)),
                "drop" => continue, // Just omit from the script
                "squash" => todo_script.push_str(&format!("squash {} {}\n", sha, message)),
                "fixup" => todo_script.push_str(&format!("fixup {} {}\n", sha, message)),
                "reword" => todo_script.push_str(&format!("reword {} {}\n", sha, message)),
                "edit" => todo_script.push_str(&format!("edit {} {}\n", sha, message)),
                _ => anyhow::bail!("Unknown rebase operation: {}", op),
            }
        }

        // Create a custom editor that will inject our todo script
        let sequence_editor = GitEditor::new(&self.path, &todo_script, "rebase-sequence")?;

        // Set up git command with sequence editor
        let mut git_command = Command::new("git");
        git_command
            .current_dir(&self.path)
            .env("GIT_SEQUENCE_EDITOR", sequence_editor.path().display().to_string());

        // Always set GIT_EDITOR to avoid interactive prompts
        // If commit message provided, use custom editor; otherwise use git's default combined message
        let _commit_editor = if let Some(msg) = commit_message {
            let editor = GitEditor::new(&self.path, msg, "rebase-commit")?;
            git_command.env("GIT_EDITOR", editor.path().display().to_string());
            Some(editor)
        } else {
            // Use 'true' as editor - accepts git's auto-generated message without opening editor
            git_command.env("GIT_EDITOR", "true");
            None
        };

        let output = git_command
            .arg("rebase")
            .arg("-i")
            .arg(base_oid.to_string())
            .output()?;


        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Git rebase failed: {}", stderr);
        }

        Ok(())
    }
}
