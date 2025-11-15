#![allow(dead_code)]

use regex::Regex;

/// Represents the parsed output of `git stk view`
pub struct ViewAssertion {
    output: String,
    commits: Vec<ParsedCommit>,
}

#[derive(Debug, Clone)]
struct ParsedCommit {
    index: usize,
    sha: String,
    title: String,
    slot: Option<String>,
    slot_assigned: bool, // true for [01], false for [?→01]
    pr_number: Option<u64>,
    status: Option<CommitStatus>,
    remote_sha: Option<String>, // SHA shown when remote differs
}

#[derive(Debug, Clone, PartialEq)]
enum CommitStatus {
    Synced,
    ExportNeeded,
    Merged,
}

impl ViewAssertion {
    /// Create from the output of `git stk view`
    pub fn from_output(output: String) -> Self {
        let commits = Self::parse_commits(&output);
        Self { output, commits }
    }

    fn parse_commits(output: &str) -> Vec<ParsedCommit> {
        let mut commits = Vec::new();
        let lines: Vec<&str> = output.lines().collect();

        let mut i = 0;
        while i < lines.len() {
            // Look for commit number line: "  1. ├─●  4980ef6  feat: add authentication  [01]"
            if let Some(commit) = Self::try_parse_commit_line(lines[i]) {
                let mut parsed = commit;
                let mut lines_consumed = 1; // Start with the commit line

                // Next line should have PR URL or "<PR to be created>"
                if i + lines_consumed < lines.len() {
                    if let Some(pr_number) = Self::try_parse_pr_url_line(lines[i + lines_consumed]) {
                        parsed.pr_number = Some(pr_number);
                        lines_consumed += 1;
                    }
                }

                // Next line might have status (Synced, Export needed, Merged)
                // Only shown when remote exists or merged
                if i + lines_consumed < lines.len() {
                    if let Some((status, remote_sha)) = Self::try_parse_status_line(lines[i + lines_consumed]) {
                        parsed.status = Some(status);
                        parsed.remote_sha = remote_sha;
                        let _ = lines_consumed; // Acknowledge we're not using this to skip lines
                    }
                }

                commits.push(parsed);
            }
            i += 1;
        }

        commits
    }

    fn try_parse_commit_line(line: &str) -> Option<ParsedCommit> {
        // Match: "  1. ├─●  4980ef6  feat: add authentication  [01]"
        // Slot is optional and can be assigned [01] or predicted [?→01]
        // Slot names can contain alphanumerics, hyphens, and underscores
        let re = Regex::new(r"^\s*(\d+)\.\s+[├└]─●\s+([0-9a-f]+)\s+(.+?)(?:\s+\[(\?→)?([\w-]+)\])?\s*$").ok()?;
        let caps = re.captures(line)?;

        let slot = caps.get(5).map(|m| m.as_str().to_string());
        let slot_assigned = caps.get(4).is_none() && slot.is_some();

        Some(ParsedCommit {
            index: caps[1].parse().ok()?,
            sha: caps[2].to_string(),
            title: caps[3].trim().to_string(),
            slot,
            slot_assigned,
            pr_number: None,
            status: None,
            remote_sha: None,
        })
    }

    fn try_parse_pr_url_line(line: &str) -> Option<u64> {
        // Match: "     │  https://github.com/.../pull/16"
        let re = Regex::new(r"https://[^/]+/[^/]+/[^/]+/pull/(\d+)").ok()?;
        let caps = re.captures(line)?;
        caps[1].parse().ok()
    }

    fn try_parse_status_line(line: &str) -> Option<(CommitStatus, Option<String>)> {
        // Match: "     │  Synced"
        if line.contains("Synced") {
            return Some((CommitStatus::Synced, None));
        }

        // Match: "     │  Merged"
        if line.contains("Merged") {
            return Some((CommitStatus::Merged, None));
        }

        // Match: "     │  Export needed" or "     │  Export needed (remote: 7cc7289)"
        if line.contains("Export needed") {
            // Try to extract remote SHA if present
            let re = Regex::new(r"remote:\s*([0-9a-f]+)").ok()?;
            let remote_sha = re.captures(line).map(|c| c[1].to_string());
            return Some((CommitStatus::ExportNeeded, remote_sha));
        }

        None
    }

    /// Assert on total number of commits
    pub fn has_commits(&self, count: usize) -> &Self {
        assert_eq!(
            self.commits.len(),
            count,
            "Expected {} commits in view, but found {}",
            count,
            self.commits.len()
        );
        self
    }

    /// Get assertion builder for a specific commit (1-based indexing)
    pub fn commit(&self, index: usize) -> CommitAssertion {
        assert!(
            index > 0 && index <= self.commits.len(),
            "Commit index {} out of range (1-{})",
            index,
            self.commits.len()
        );

        CommitAssertion {
            commit: self.commits[index - 1].clone(),
            index,
        }
    }
}

/// Fluent assertion builder for a specific commit
pub struct CommitAssertion {
    commit: ParsedCommit,
    index: usize,
}

impl CommitAssertion {
    /// Assert commit has an assigned slot
    pub fn has_slot(&self, slot: &str) -> &Self {
        assert!(
            self.commit.slot_assigned,
            "Expected commit {} to have assigned slot [{}], but found [?→{}]",
            self.index,
            slot,
            self.commit.slot.as_ref().unwrap_or(&"none".to_string())
        );
        assert_eq!(
            self.commit.slot.as_ref().map(|s| s.as_str()),
            Some(slot),
            "Expected commit {} to have slot [{}], but found [{}]",
            self.index,
            slot,
            self.commit.slot.as_ref().unwrap_or(&"none".to_string())
        );
        self
    }

    /// Assert commit has a slot to be assigned
    pub fn slot_to_be_assigned(&self, slot: &str) -> &Self {
        assert!(
            !self.commit.slot_assigned,
            "Expected commit {} to have slot to be assigned [?→{}], but found assigned slot [{}]",
            self.index,
            slot,
            self.commit.slot.as_ref().unwrap_or(&"none".to_string())
        );
        assert_eq!(
            self.commit.slot.as_ref().map(|s| s.as_str()),
            Some(slot),
            "Expected commit {} to have slot [?→{}], but found [?→{}]",
            self.index,
            slot,
            self.commit.slot.as_ref().unwrap_or(&"none".to_string())
        );
        self
    }

    /// Assert commit has specific title
    pub fn has_title(&self, title: &str) -> &Self {
        assert_eq!(
            self.commit.title,
            title,
            "Expected commit {} to have title '{}', but found '{}'",
            self.index,
            title,
            self.commit.title
        );
        self
    }

    /// Assert commit has no status line (no remote ref)
    pub fn has_no_status(&self) -> &Self {
        assert!(
            self.commit.status.is_none(),
            "Expected commit {} to have no status, but found {:?}",
            self.index,
            self.commit.status
        );
        self
    }

    /// Assert commit has a PR
    pub fn has_pr(&self, pr_number: u64) -> &Self {
        assert_eq!(
            self.commit.pr_number,
            Some(pr_number),
            "Expected commit {} to have PR #{}, but found {:?}",
            self.index,
            pr_number,
            self.commit.pr_number
        );
        self
    }

    /// Assert commit has no PR
    pub fn no_pr(&self) -> &Self {
        assert!(
            self.commit.pr_number.is_none(),
            "Expected commit {} to have no PR, but found PR #{}",
            self.index,
            self.commit.pr_number.unwrap()
        );
        self
    }

    /// Assert commit has a PR (any PR number)
    pub fn has_pr_number(&self) -> &Self {
        assert!(
            self.commit.pr_number.is_some(),
            "Expected commit {} to have a PR, but found none",
            self.index
        );
        self
    }

    /// Get PR number (returns Option<u64>)
    pub fn pr_number(&self) -> Option<u64> {
        self.commit.pr_number
    }

    /// Assert commit status is Synced
    pub fn is_synced(&self) -> &Self {
        assert_eq!(
            self.commit.status,
            Some(CommitStatus::Synced),
            "Expected commit {} to be Synced, but found {:?}",
            self.index,
            self.commit.status
        );
        self
    }

    /// Assert commit status is Merged
    pub fn is_merged(&self) -> &Self {
        assert_eq!(
            self.commit.status,
            Some(CommitStatus::Merged),
            "Expected commit {} to be Merged, but found {:?}",
            self.index,
            self.commit.status
        );
        self
    }

    /// Assert commit status is Export needed
    pub fn is_export_needed(&self) -> &Self {
        assert_eq!(
            self.commit.status,
            Some(CommitStatus::ExportNeeded),
            "Expected commit {} to be Export needed, but found {:?}",
            self.index,
            self.commit.status
        );
        self
    }

    /// Assert commit status is Export needed with a specific remote SHA
    pub fn is_export_needed_with_remote(&self, remote_sha_prefix: &str) -> &Self {
        assert_eq!(
            self.commit.status,
            Some(CommitStatus::ExportNeeded),
            "Expected commit {} to be Export needed, but found {:?}",
            self.index,
            self.commit.status
        );
        assert!(
            self.commit.remote_sha.as_ref().map(|s| s.starts_with(remote_sha_prefix)).unwrap_or(false),
            "Expected commit {} to have remote SHA starting with '{}', but found {:?}",
            self.index,
            remote_sha_prefix,
            self.commit.remote_sha
        );
        self
    }
}
