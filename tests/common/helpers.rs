use anyhow::Result;
use std::path::Path;
use std::process::Command;

/// Run a git-stk command in the given directory
pub fn run_git_stk_command(repo_path: &Path, args: &[&str]) -> Result<String> {
    // Get the path to the project root (where Cargo.toml is)
    let project_root = std::env::current_dir()?;
    let manifest_path = project_root.join("Cargo.toml");

    let output = Command::new("cargo")
        .arg("run")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(manifest_path)
        .arg("--")
        .args(args)
        .current_dir(repo_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Command failed: {}", stderr);
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Clean up all branches with the given test ID prefix
pub fn cleanup_test_branches(repo_path: &Path, test_id: &str) -> Result<()> {
    // List all remote branches
    let output = Command::new("git")
        .current_dir(repo_path)
        .args(["branch", "-r"])
        .output()?;

    if !output.status.success() {
        eprintln!("  ⚠️  Warning: Failed to list remote branches");
        return Ok(());
    }

    let branches = String::from_utf8_lossy(&output.stdout);
    let test_branches: Vec<String> = branches
        .lines()
        .map(|line| line.trim())
        .filter(|line| line.contains(test_id))
        .filter_map(|line| {
            // Remove 'origin/' prefix
            line.strip_prefix("origin/").map(|s| s.to_string())
        })
        .collect();

    if test_branches.is_empty() {
        println!("  No test branches found to clean up");
        return Ok(());
    }

    println!("  Found {} branches to delete:", test_branches.len());
    for branch in &test_branches {
        println!("    - {}", branch);
    }

    // Delete all test branches
    for branch in test_branches {
        let result = Command::new("git")
            .current_dir(repo_path)
            .args(["push", "origin", "--delete", &branch])
            .output()?;

        if result.status.success() {
            println!("  ✓ Deleted: {}", branch);
        } else {
            eprintln!("  ⚠️  Failed to delete: {}", branch);
        }
    }

    Ok(())
}
