#![allow(dead_code)]

use anyhow::Result;
use git_stk::gh::{client, queries};
use git_stk::gh::queries::PrInfo;

/// Entry point for GitHub PR assertions
pub struct GithubAssertion {
    owner: String,
    repo: String,
}

impl GithubAssertion {
    pub fn new(owner: &str, repo: &str) -> Self {
        Self {
            owner: owner.to_string(),
            repo: repo.to_string(),
        }
    }

    /// Select PR by head branch name
    pub fn pr_with_head(self, head: &str) -> PrSelectorBuilder {
        PrSelectorBuilder {
            owner: self.owner,
            repo: self.repo,
            selector: PrSelector::ByHead(head.to_string()),
        }
    }

    /// Select PR by PR number
    pub fn pr_with_number(self, number: u64) -> PrSelectorBuilder {
        PrSelectorBuilder {
            owner: self.owner,
            repo: self.repo,
            selector: PrSelector::ByNumber(number),
        }
    }

    /// Select PR by slot (convenience method that builds the head branch name)
    pub fn pr_with_slot(self, test_id: &str, slot: &str) -> PrSelectorBuilder {
        let head = format!("{}-feature--{}", test_id, slot);
        PrSelectorBuilder {
            owner: self.owner,
            repo: self.repo,
            selector: PrSelector::ByHead(head),
        }
    }
}

enum PrSelector {
    ByHead(String),
    ByNumber(u64),
}

/// Builder for selecting and fetching a PR
pub struct PrSelectorBuilder {
    owner: String,
    repo: String,
    selector: PrSelector,
}

impl PrSelectorBuilder {
    /// Fetch the PR from GitHub (async operation)
    pub async fn fetch(self) -> Result<PrAssertion> {
        let client = client::create_client()?;

        let pr_info = match self.selector {
            PrSelector::ByHead(head) => {
                queries::find_pr_by_head(&client, &self.owner, &self.repo, &head)
                    .await?
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "PR not found with head branch: {}",
                            head
                        )
                    })?
            }
            PrSelector::ByNumber(number) => {
                queries::get_pr(&client, &self.owner, &self.repo, number).await?
            }
        };

        Ok(PrAssertion { pr: pr_info })
    }
}

/// Fluent assertion builder for a GitHub PR
pub struct PrAssertion {
    pr: PrInfo,
}

impl PrAssertion {
    /// Assert PR has specific title
    pub fn has_title(&self, expected: &str) -> &Self {
        assert_eq!(
            self.pr.title, expected,
            "Expected PR #{} to have title '{}', but found '{}'",
            self.pr.number, expected, self.pr.title
        );
        self
    }

    /// Assert PR has specific base branch
    pub fn has_base(&self, expected: &str) -> &Self {
        assert_eq!(
            self.pr.base_ref, expected,
            "Expected PR #{} to have base '{}', but found '{}'",
            self.pr.number, expected, self.pr.base_ref
        );
        self
    }

    /// Assert PR description contains text
    pub fn description_contains(&self, text: &str) -> &Self {
        assert!(
            self.pr.body.contains(text),
            "Expected PR #{} description to contain '{}', but it didn't. Description:\n{}",
            self.pr.number, text, self.pr.body
        );
        self
    }

    /// Assert PR description matches regex
    pub fn description_matches(&self, pattern: &str) -> &Self {
        let re = regex::Regex::new(pattern).expect("Invalid regex pattern");
        assert!(
            re.is_match(&self.pr.body),
            "Expected PR #{} description to match pattern '{}', but it didn't. Description:\n{}",
            self.pr.number, pattern, self.pr.body
        );
        self
    }

    /// Assert PR is a draft
    pub fn is_draft(&self) -> &Self {
        use git_stk::model::PrState;
        assert_eq!(
            self.pr.state,
            PrState::Draft,
            "Expected PR #{} to be a draft, but state is {:?}",
            self.pr.number, self.pr.state
        );
        self
    }

    /// Assert PR has specific state
    pub fn has_state(&self, expected: git_stk::model::PrState) -> &Self {
        assert_eq!(
            self.pr.state, expected,
            "Expected PR #{} to have state {:?}, but found {:?}",
            self.pr.number, expected, self.pr.state
        );
        self
    }

    /// Assert PR is open
    pub fn is_open(&self) -> &Self {
        use git_stk::model::PrState;
        assert_eq!(
            self.pr.state,
            PrState::Open,
            "Expected PR #{} to be open, but state is {:?}",
            self.pr.number, self.pr.state
        );
        self
    }

    /// Assert PR is closed
    pub fn is_closed(&self) -> &Self {
        use git_stk::model::PrState;
        assert_eq!(
            self.pr.state,
            PrState::Closed,
            "Expected PR #{} to be closed, but state is {:?}",
            self.pr.number, self.pr.state
        );
        self
    }

    /// Get PR number (for further use)
    pub fn number(&self) -> u64 {
        self.pr.number
    }
}
