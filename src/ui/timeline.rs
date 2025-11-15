use crate::model::{Entry, Stack, UpdateStatus};
use console::style;

/// Render a beautiful timeline view of the stack
pub fn render_timeline(stack: &Stack) {
    if stack.is_empty() {
        println!("{}", style("No commits in stack").dim());
        println!(
            "  {} Current branch is up to date with {}",
            style("ℹ").blue(),
            style(&stack.base_branch).cyan()
        );
        return;
    }

    // Calculate maximum width for index alignment
    let max_index_width = stack.entries.len().to_string().len();

    // Base branch indicator
    let padding = " ".repeat(max_index_width + 1);
    println!("  {} {} {}", padding, style("┌─").dim(), style(&stack.base_branch).yellow().dim());
    println!("  {} {}", padding, style("│").dim());

    // Render each entry
    for (idx, entry) in stack.entries.iter().enumerate() {
        let is_last = idx == stack.entries.len() - 1;
        let index = idx + 1; // Start from 1
        render_entry(entry, is_last, index, max_index_width);

        if !is_last {
            let padding = " ".repeat(max_index_width + 1); // +1 for the dot
            println!("  {} {}", padding, style("│").dim());
        }
    }

    println!();
}

fn render_entry(
    entry: &Entry,
    is_last: bool,
    index: usize,
    max_index_width: usize,
) {
    let connector = if is_last { "└─" } else { "├─" };
    let indent = if is_last { " " } else { "│" };

    // Bullet color based on status (merged takes priority)
    let bullet = if entry.merged_into_main {
        style("●").magenta()
    } else {
        match entry.status {
            UpdateStatus::UpToDate => style("●").green(),
            UpdateStatus::NeedsUpdate => style("●").yellow(),
            UpdateStatus::CreatePr => style("●").blue(),
        }
    };

    // Line 1: Commit with index, truncated subject, and slot
    let index_str = format!("{:>width$}.", index, width = max_index_width);

    // Truncate subject to 80 chars
    let subject = if entry.subject.len() > 80 {
        format!("{}...", &entry.subject[..77])
    } else {
        entry.subject.clone()
    };

    // Format slot for line 1
    let slot_display = if let Some(ref slot) = entry.slot {
        format!("  [{}]", style(slot).yellow())
    } else if let Some(ref predicted) = entry.predicted_slot {
        format!("  [{}]", style(format!("?→{}", predicted)).yellow().dim())
    } else {
        String::new()
    };

    println!(
        "  {} {}{}  {}  {}{}",
        style(&index_str).dim(),
        style(connector).dim(),
        bullet,
        style(&entry.short_sha).black().bright(),
        style(&subject).bold(),
        slot_display
    );

    let padding = " ".repeat(max_index_width + 1); // +1 for the dot

    // Line 2: PR link (no slot)
    let pr_line = format_pr_link(entry);
    println!(
        "  {} {}  {}",
        padding,
        style(indent).dim(),
        pr_line
    );

    // Line 3: Status (only show if remote branch exists or merged)
    if entry.remote_branch_exists || entry.merged_into_main {
        let status_line = format_status_line(entry);
        println!(
            "  {} {}  {}",
            padding,
            style(indent).dim(),
            status_line
        );
    }
}

/// Line 2: PR link (or <PR to be created>)
fn format_pr_link(entry: &Entry) -> String {
    if let Some(pr_number) = entry.pr_number {
        if let (Some(owner), Some(repo)) = (&entry.repo_owner, &entry.repo_name) {
            let pr_url = format!("https://github.com/{}/{}/pull/{}", owner, repo, pr_number);
            style(&pr_url).cyan().underlined().to_string()
        } else {
            format!("#{}", pr_number)
        }
    } else {
        style("<PR to be created>").dim().to_string()
    }
}

/// Line 3: Status (Synced | Export needed | Merged)
/// Only called when remote_branch_exists || merged_into_main
fn format_status_line(entry: &Entry) -> String {
    // Priority 1: Merged
    if entry.merged_into_main {
        return style("Merged").magenta().to_string();
    }

    // Check remote status
    if let Some(remote_oid) = entry.remote_oid {
        if remote_oid == entry.oid {
            // Remote matches commit - synced
            return style("Synced").green().to_string();
        } else {
            // Remote exists but differs
            let remote_short = remote_oid.to_string()[..7].to_string();
            return format!(
                "{} (remote: {})",
                style("Export needed").yellow(),
                style(&remote_short).red().dim()
            );
        }
    }

    // Remote exists but no OID info - assume synced
    style("Synced").green().to_string()
}
