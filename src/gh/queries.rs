use crate::model::PrState;
use anyhow::{Context, Result};
use octocrab::Octocrab;
use octocrab::models::pulls::PullRequest;
use serde_json::json;
use std::collections::HashMap;

/// Information about a pull request
#[derive(Debug, Clone)]
pub struct PrInfo {
    pub number: u64,
    pub state: PrState,
    pub title: String,
    pub body: String,
    pub base_ref: String,
    pub head_ref: String,
    pub head_sha: String,
}

/// Look up a PR by head ref name
pub async fn find_pr_by_head(
    client: &Octocrab,
    owner: &str,
    repo: &str,
    head_ref: &str,
) -> Result<Option<PrInfo>> {
    // Search for PRs with this head ref
    let pulls = client
        .pulls(owner, repo)
        .list()
        .state(octocrab::params::State::All)
        .head(format!("{}:{}", owner, head_ref))
        .per_page(1)
        .send()
        .await
        .context("Failed to query GitHub for pull requests")?;

    if let Some(pr) = pulls.items.first() {
        Ok(Some(pr_info_from_octocrab(pr)))
    } else {
        Ok(None)
    }
}

/// Get PR information by PR number
pub async fn get_pr(
    client: &Octocrab,
    owner: &str,
    repo: &str,
    pr_number: u64,
) -> Result<PrInfo> {
    let pr = client
        .pulls(owner, repo)
        .get(pr_number)
        .await
        .with_context(|| format!("Failed to get PR #{}", pr_number))?;

    Ok(pr_info_from_octocrab(&pr))
}

/// Batch fetch multiple PRs using GraphQL
pub async fn get_prs_batch(
    client: &Octocrab,
    owner: &str,
    repo: &str,
    pr_numbers: &[u64],
) -> Result<HashMap<u64, PrInfo>> {
    if pr_numbers.is_empty() {
        return Ok(HashMap::new());
    }

    // Build GraphQL query with aliases for each PR
    let mut query_parts = Vec::new();
    for (idx, pr_number) in pr_numbers.iter().enumerate() {
        query_parts.push(format!(
            r#"pr{}: pullRequest(number: {}) {{
                number
                title
                body
                state
                isDraft
                merged
                baseRefName
                headRefName
                headRefOid
            }}"#,
            idx, pr_number
        ));
    }

    let query = format!(
        r#"query {{
            repository(owner: "{}", name: "{}") {{
                {}
            }}
        }}"#,
        owner,
        repo,
        query_parts.join("\n                ")
    );

    // Execute GraphQL query
    let response: serde_json::Value = client
        .graphql(&json!({ "query": query }))
        .await
        .context("Failed to execute GraphQL query for batch PR fetch")?;

    // Parse results
    let mut results = HashMap::new();

    if let Some(repository) = response.get("data").and_then(|d| d.get("repository")) {
        for (idx, pr_number) in pr_numbers.iter().enumerate() {
            let pr_key = format!("pr{}", idx);
            if let Some(pr_data) = repository.get(&pr_key) {
                if !pr_data.is_null() {
                    if let Ok(pr_info) = parse_graphql_pr(pr_data, *pr_number) {
                        results.insert(*pr_number, pr_info);
                    }
                }
            }
        }
    }

    Ok(results)
}

/// Parse PR info from GraphQL response
fn parse_graphql_pr(data: &serde_json::Value, pr_number: u64) -> Result<PrInfo> {
    let state_str = data
        .get("state")
        .and_then(|s| s.as_str())
        .unwrap_or("CLOSED");
    let merged = data
        .get("merged")
        .and_then(|m| m.as_bool())
        .unwrap_or(false);
    let is_draft = data
        .get("isDraft")
        .and_then(|d| d.as_bool())
        .unwrap_or(false);

    let state = if merged {
        PrState::Merged
    } else if is_draft {
        PrState::Draft
    } else if state_str == "OPEN" {
        PrState::Open
    } else {
        PrState::Closed
    };

    Ok(PrInfo {
        number: pr_number,
        state,
        title: data
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string(),
        body: data
            .get("body")
            .and_then(|b| b.as_str())
            .unwrap_or("")
            .to_string(),
        base_ref: data
            .get("baseRefName")
            .and_then(|r| r.as_str())
            .unwrap_or("")
            .to_string(),
        head_ref: data
            .get("headRefName")
            .and_then(|r| r.as_str())
            .unwrap_or("")
            .to_string(),
        head_sha: data
            .get("headRefOid")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
    })
}

/// Convert octocrab PullRequest to our PrInfo
fn pr_info_from_octocrab(pr: &PullRequest) -> PrInfo {
    use octocrab::models::IssueState;

    let state = if pr.merged_at.is_some() {
        PrState::Merged
    } else if pr.draft.unwrap_or(false) {
        PrState::Draft
    } else {
        match pr.state.as_ref() {
            Some(IssueState::Open) => PrState::Open,
            Some(IssueState::Closed) => PrState::Closed,
            Some(_) => PrState::Closed, // Handle any other state
            None => PrState::Closed,
        }
    };

    PrInfo {
        number: pr.number,
        state,
        title: pr.title.clone().unwrap_or_default(),
        body: pr.body.clone().unwrap_or_default(),
        base_ref: pr.base.ref_field.clone(),
        head_ref: pr.head.ref_field.clone(),
        head_sha: pr.head.sha.clone(),
    }
}
