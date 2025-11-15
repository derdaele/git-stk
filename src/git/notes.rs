use crate::model::CommitMetadata;
use anyhow::{anyhow, Context, Result};
use git2::{Oid, Repository};
use std::process::Command;

/// Read metadata from git notes for a commit
pub fn read_note(repo: &Repository, oid: Oid, notes_ref: &str) -> Result<Option<CommitMetadata>> {
    match repo.find_note(Some(notes_ref), oid) {
        Ok(note) => {
            let message = note
                .message()
                .context("Failed to read note message")?;

            // Try to parse the note normally
            match serde_json::from_str::<CommitMetadata>(message) {
                Ok(metadata) => Ok(Some(metadata)),
                Err(_) => {
                    // If parsing fails, try to extract the first JSON object
                    // This handles corrupted notes from git rebase squashing
                    if let Some(first_json) = extract_first_json_object(message) {
                        let metadata: CommitMetadata = serde_json::from_str(&first_json)
                            .context("Failed to parse first JSON object from note")?;

                        // Only rewrite if there's extra content after the first JSON
                        // (i.e., corrupted from concatenation)
                        if first_json.trim() != message.trim() {
                            write_note(repo, oid, &metadata, notes_ref)?;
                        }

                        Ok(Some(metadata))
                    } else {
                        Err(anyhow!("Failed to parse note JSON"))
                    }
                }
            }
        }
        Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
        Err(e) => Err(e).context("Failed to read note"),
    }
}

/// Write metadata to git notes for a commit
pub fn write_note(
    repo: &Repository,
    oid: Oid,
    metadata: &CommitMetadata,
    notes_ref: &str,
) -> Result<()> {
    let json = serde_json::to_string_pretty(metadata)
        .context("Failed to serialize metadata")?;

    let signature = repo.signature()
        .context("Failed to create signature")?;

    repo.note(&signature, &signature, Some(notes_ref), oid, &json, true)
        .context("Failed to write note")?;

    Ok(())
}

/// Remove a note for a commit
pub fn remove_note(repo: &Repository, oid: Oid, notes_ref: &str) -> Result<()> {
    let signature = repo.signature()
        .context("Failed to create signature")?;

    repo.note_delete(oid, Some(notes_ref), &signature, &signature)
        .context("Failed to delete note")?;

    Ok(())
}

/// Push notes to remote to share metadata
pub fn push_notes(repo: &Repository, remote: &str, notes_ref: &str) -> Result<()> {
    let repo_path = repo
        .workdir()
        .ok_or_else(|| anyhow!("Repository has no working directory"))?;

    // Push notes using refspec with --force to handle deletions properly
    // This ensures that notes deleted locally (e.g., after reconciliation) are also deleted on the remote
    let refspec = format!("{}:{}", notes_ref, notes_ref);

    let output = Command::new("git")
        .current_dir(repo_path)
        .arg("push")
        .arg("--force")
        .arg(remote)
        .arg(&refspec)
        .output()
        .context("Failed to execute git push for notes")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Failed to push notes: {}", stderr));
    }

    Ok(())
}

/// Extract the first complete JSON object from a string
/// This is used to handle corrupted notes that have been concatenated during git rebase
fn extract_first_json_object(s: &str) -> Option<String> {
    let start = s.find('{')?;
    let mut depth = 0;
    let mut end = start;

    for (i, ch) in s[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }

    if depth == 0 && end > start {
        Some(s[start..end].to_string())
    } else {
        None
    }
}
