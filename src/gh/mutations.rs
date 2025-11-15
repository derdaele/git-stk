use crate::ui::callout;
use anyhow::{Context, Result};
use octocrab::Octocrab;
use serde_json::json;
use std::time::Duration;

/// Create a new pull request with retry logic for race conditions
pub async fn create_pull_request(
    client: &Octocrab,
    owner: &str,
    repo: &str,
    head: &str,
    base: &str,
    title: &str,
    body: &str,
    draft: bool,
) -> Result<u64> {
    // Retry up to 3 times with exponential backoff to handle race conditions
    // where GitHub hasn't fully processed the pushed branch yet
    let max_retries = 3;
    let mut last_error_msg = String::new();

    for attempt in 0..max_retries {
        // Add a small delay on retries to give GitHub time to process the push
        if attempt > 0 {
            let delay_ms = 1000 * (2_u64.pow(attempt - 1)); // 1s, 2s
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }

        // Use REST API since octocrab's GraphQL support is limited
        match client
            .pulls(owner, repo)
            .create(title, head, base)
            .body(body)
            .draft(draft)
            .send()
            .await
        {
            Ok(pr) => return Ok(pr.number),
            Err(e) => {
                // Capture detailed error information
                last_error_msg = format!("{:#?}", e);

                // Check if it's a retryable error
                let error_str = format!("{:?}", e);
                if error_str.contains("422") || error_str.contains("Validation Failed") {
                    // 422 is validation error - not retryable
                    break;
                }
                // Continue to next retry for other errors
            }
        }
    }

    // All retries failed - provide detailed error info
    Err(anyhow::anyhow!(
        "Failed to create pull request for {}/{}. Head: {}, Base: {}.\n\n\
        GitHub API error:\n{}\n\n\
        Make sure the head branch has been pushed to the remote and you have write access to the repository.",
        owner, repo, head, base, last_error_msg
    ))
}

/// Update an existing pull request
pub async fn update_pull_request(
    client: &Octocrab,
    owner: &str,
    repo: &str,
    pr_number: u64,
    base: Option<&str>,
    title: Option<&str>,
    body: Option<&str>,
) -> Result<()> {
    // Use REST API to update PR
    let pulls = client.pulls(owner, repo);
    let mut update = pulls.update(pr_number);

    if let Some(base_ref) = base {
        update = update.base(base_ref);
    }

    if let Some(pr_title) = title {
        update = update.title(pr_title);
    }

    if let Some(pr_body) = body {
        update = update.body(pr_body);
    }

    update
        .send()
        .await
        .context("Failed to update pull request")?;

    Ok(())
}

/// Batch update multiple PR bases in a single GraphQL mutation
/// Updates are executed in the order provided (important for maintaining chain integrity)
pub async fn batch_update_pr_bases(
    client: &Octocrab,
    owner: &str,
    repo: &str,
    updates: &[(u64, String)], // Vec of (pr_number, new_base)
) -> Result<()> {
    if updates.is_empty() {
        return Ok(());
    }

    // First, get PR node IDs for all PRs (required for GraphQL mutations)
    let pr_node_ids = get_pr_node_ids(client, owner, repo, updates).await?;

    // Build GraphQL mutation with all updates
    let mut mutation_parts = Vec::new();
    for (idx, (pr_number, new_base)) in updates.iter().enumerate() {
        let node_id = pr_node_ids
            .get(pr_number)
            .ok_or_else(|| anyhow::anyhow!("Failed to get node ID for PR #{}", pr_number))?;

        mutation_parts.push(format!(
            r#"
            update{}: updatePullRequest(input: {{
                pullRequestId: "{}"
                baseRefName: "{}"
            }}) {{
                pullRequest {{
                    number
                }}
            }}
            "#,
            idx, node_id, new_base
        ));
    }

    let mutation = format!("mutation {{ {} }}", mutation_parts.join("\n"));

    // Execute the batched mutation
    let response: serde_json::Value = client
        .graphql(&json!({ "query": mutation }))
        .await
        .context("Failed to execute batched PR base updates")?;

    // Check for errors in the response
    if let Some(errors) = response.get("errors") {
        return Err(anyhow::anyhow!(
            "GraphQL mutation failed: {}",
            serde_json::to_string_pretty(errors)?
        ));
    }

    Ok(())
}

/// Batch update multiple PR bodies in a single GraphQL mutation
pub async fn batch_update_pr_bodies(
    client: &Octocrab,
    owner: &str,
    repo: &str,
    updates: &[(u64, String)], // Vec of (pr_number, new_body)
) -> Result<()> {
    if updates.is_empty() {
        return Ok(());
    }

    // First, get PR node IDs for all PRs (required for GraphQL mutations)
    let pr_node_ids = get_pr_node_ids(client, owner, repo, updates).await?;

    // Build GraphQL mutation with all updates
    let mut mutation_parts = Vec::new();
    for (idx, (pr_number, new_body)) in updates.iter().enumerate() {
        let node_id = pr_node_ids
            .get(pr_number)
            .ok_or_else(|| anyhow::anyhow!("Failed to get node ID for PR #{}", pr_number))?;

        // Escape the body for GraphQL
        let escaped_body = escape_graphql_string(new_body);

        mutation_parts.push(format!(
            r#"
            update{}: updatePullRequest(input: {{
                pullRequestId: "{}"
                body: "{}"
            }}) {{
                pullRequest {{
                    number
                }}
            }}
            "#,
            idx, node_id, escaped_body
        ));
    }

    let mutation = format!("mutation {{ {} }}", mutation_parts.join("\n"));

    // Execute the batched mutation
    let response: serde_json::Value = client
        .graphql(&json!({ "query": mutation }))
        .await
        .context("Failed to execute batched PR body updates")?;

    // Check for errors in the response
    if let Some(errors) = response.get("errors") {
        return Err(anyhow::anyhow!(
            "GraphQL mutation failed: {}",
            serde_json::to_string_pretty(errors)?
        ));
    }

    Ok(())
}

/// Escape a string for use in a GraphQL query
fn escape_graphql_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// Close a pull request
/// Add a comment to a pull request
pub async fn add_pr_comment(
    client: &Octocrab,
    owner: &str,
    repo: &str,
    pr_number: u64,
    body: &str,
) -> Result<()> {
    client
        .issues(owner, repo)
        .create_comment(pr_number, body)
        .await
        .context("Failed to add comment to pull request")?;

    Ok(())
}

pub async fn close_pull_request(
    client: &Octocrab,
    owner: &str,
    repo: &str,
    pr_number: u64,
) -> Result<()> {
    // Get current PR to retrieve the body
    let pr = client
        .pulls(owner, repo)
        .get(pr_number)
        .await
        .context("Failed to get pull request")?;

    // Strip the callout from the body
    let body = pr.body.unwrap_or_default();
    let clean_body = callout::strip_callout(&body);

    // Close the PR and update body to remove callout
    let pulls = client.pulls(owner, repo);
    let mut update = pulls.update(pr_number).state(octocrab::params::pulls::State::Closed);

    if !clean_body.is_empty() {
        update = update.body(&clean_body);
    }

    update
        .send()
        .await
        .context("Failed to close pull request")?;

    Ok(())
}

/// Merge a pull request with optional custom commit message
pub async fn merge_pull_request(
    client: &Octocrab,
    owner: &str,
    repo: &str,
    pr_number: u64,
    commit_title: Option<&str>,
    commit_message: Option<&str>,
) -> Result<()> {
    // Use REST API to merge the PR
    let pulls = client.pulls(owner, repo);
    let mut merge_builder = pulls.merge(pr_number);

    if let Some(title) = commit_title {
        merge_builder = merge_builder.title(title);
    }

    if let Some(message) = commit_message {
        merge_builder = merge_builder.message(message);
    }

    merge_builder
        .send()
        .await
        .context("Failed to merge pull request")?;

    Ok(())
}

/// Helper function to get PR node IDs for GraphQL mutations
async fn get_pr_node_ids(
    client: &Octocrab,
    owner: &str,
    repo: &str,
    updates: &[(u64, String)],
) -> Result<std::collections::HashMap<u64, String>> {
    use std::collections::HashMap;

    // Build a query to fetch all PR node IDs at once
    let pr_numbers: Vec<u64> = updates.iter().map(|(num, _)| *num).collect();
    let mut pr_queries = Vec::new();

    for (idx, pr_number) in pr_numbers.iter().enumerate() {
        pr_queries.push(format!(
            r#"
            pr{}: pullRequest(number: {}) {{
                id
                number
            }}
            "#,
            idx, pr_number
        ));
    }

    let query = format!(
        r#"
        query {{
            repository(owner: "{}", name: "{}") {{
                {}
            }}
        }}
        "#,
        owner,
        repo,
        pr_queries.join("\n")
    );

    let response: serde_json::Value = client
        .graphql(&json!({ "query": query }))
        .await
        .context("Failed to fetch PR node IDs")?;

    // Parse the response to extract node IDs
    let mut node_ids = HashMap::new();
    if let Some(repo_data) = response.get("data").and_then(|d| d.get("repository")) {
        for (idx, pr_number) in pr_numbers.iter().enumerate() {
            let pr_key = format!("pr{}", idx);
            if let Some(pr_data) = repo_data.get(&pr_key) {
                if let Some(node_id) = pr_data.get("id").and_then(|v| v.as_str()) {
                    node_ids.insert(*pr_number, node_id.to_string());
                }
            }
        }
    }

    Ok(node_ids)
}
