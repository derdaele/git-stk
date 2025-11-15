use crate::gh::client;
use crate::model::Config;
use crate::stack::discover_stack;
use crate::ui::render_timeline;
use anyhow::{Context, Result};
use git2::Repository;

pub async fn view() -> Result<()> {
    let git_repo = Repository::open(".").context("Failed to open git repository")?;
    let config = Config::load(&git_repo)?;
    let gh_client = client::create_client()?;

    let stack = discover_stack(&git_repo, &config, &gh_client).await?;

    render_timeline(&stack);

    Ok(())
}
