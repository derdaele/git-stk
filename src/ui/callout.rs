use crate::model::{Entry, PrState};

const CALLOUT_BEGIN: &str = "<!-- git-stk:begin (do not edit) -->";
const CALLOUT_END: &str = "<!-- git-stk:end -->";

/// Generate the stack callout markdown for a PR
pub fn generate_callout(
    entries: &[Entry],
    current_index: usize,
    owner: &str,
    repo: &str,
) -> String {
    let mut lines = vec![CALLOUT_BEGIN.to_string()];

    // Use collapsible details element, expanded by default
    lines.push("<details open>".to_string());
    lines.push(format!("<summary>📚 Stack ({} of {})</summary>", current_index, entries.len()));
    lines.push(String::new()); // Empty line for markdown parsing

    // Add each PR in the stack
    for (idx, entry) in entries.iter().enumerate() {
        let line = format_stack_item(entry, entry.index == current_index, idx + 1, owner, repo);
        lines.push(line);
    }

    lines.push(String::new()); // Empty line before closing tag
    lines.push("</details>".to_string());
    lines.push(CALLOUT_END.to_string());

    lines.join("\n")
}

/// Format a single stack item with inline PR reference
fn format_stack_item(entry: &Entry, is_current: bool, position: usize, owner: &str, repo: &str) -> String {
    let state_emoji = if let Some(state) = &entry.pr_state {
        match state {
            PrState::Draft => " 🟡",
            _ => "",
        }
    } else {
        ""
    };

    match &entry.pr_number {
        Some(pr_number) => {
            if is_current {
                // Current PR - bold with indicator
                format!("{}. **{}/{}#{}** ← current{}", position, owner, repo, pr_number, state_emoji)
            } else {
                // Other PRs - clickable reference (GitHub auto-renders title)
                format!("{}. {}/{}#{}{}", position, owner, repo, pr_number, state_emoji)
            }
        }
        None => {
            // PR not created yet
            if is_current {
                format!("{}. **{}** ← current", position, entry.subject)
            } else {
                format!("{}. {} _(pending)_", position, entry.subject)
            }
        }
    }
}


/// Inject or replace the stack callout in a PR body
pub fn inject_callout(existing_body: &str, callout: &str) -> String {
    // Find existing callout markers
    if let Some(start) = existing_body.find(CALLOUT_BEGIN) {
        if let Some(end) = existing_body[start..].find(CALLOUT_END) {
            // Replace existing callout
            let end_pos = start + end + CALLOUT_END.len();

            // Get content after the callout
            let after_callout = existing_body[end_pos..].trim_start();

            // Combine new callout with existing content
            if after_callout.is_empty() {
                callout.to_string()
            } else {
                format!("{}\n\n{}", callout, after_callout)
            }
        } else {
            // Malformed: has begin but no end, just prepend
            format!("{}\n\n{}", callout, existing_body)
        }
    } else {
        // No existing callout, prepend to existing content
        if existing_body.trim().is_empty() {
            callout.to_string()
        } else {
            format!("{}\n\n{}", callout, existing_body)
        }
    }
}

/// Strip the callout from a PR body, keeping all other content
pub fn strip_callout(body: &str) -> String {
    if let Some(start) = body.find(CALLOUT_BEGIN) {
        if let Some(end) = body[start..].find(CALLOUT_END) {
            let end_pos = start + end + CALLOUT_END.len();

            // Get content before and after the callout
            let before = body[..start].trim_end();
            let after = body[end_pos..].trim_start();

            // Combine them, adding spacing if both exist
            match (before.is_empty(), after.is_empty()) {
                (true, true) => String::new(),
                (true, false) => after.to_string(),
                (false, true) => before.to_string(),
                (false, false) => format!("{}\n\n{}", before, after),
            }
        } else {
            // Malformed: has begin but no end, return as-is
            body.to_string()
        }
    } else {
        // No callout found, return as-is
        body.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inject_callout_new() {
        let body = "This is my PR description.";
        let callout = "<!-- git-stk:begin (do not edit) -->\nStack info\n<!-- git-stk:end -->";

        let result = inject_callout(body, callout);

        assert!(result.starts_with("<!-- git-stk:begin"));
        assert!(result.contains("This is my PR description."));
    }

    #[test]
    fn test_inject_callout_replace() {
        let body = "<!-- git-stk:begin (do not edit) -->\nOld stack\n<!-- git-stk:end -->\n\nMy description.";
        let callout = "<!-- git-stk:begin (do not edit) -->\nNew stack\n<!-- git-stk:end -->";

        let result = inject_callout(body, callout);

        assert!(result.contains("New stack"));
        assert!(!result.contains("Old stack"));
        assert!(result.contains("My description."));
    }

    #[test]
    fn test_strip_callout_at_beginning() {
        let body = "<!-- git-stk:begin (do not edit) -->\nStack\n<!-- git-stk:end -->\n\nUser content here.";

        let result = strip_callout(body);

        assert_eq!(result, "User content here.");
    }

    #[test]
    fn test_strip_callout_in_middle() {
        let body = "Some intro text.\n\n<!-- git-stk:begin (do not edit) -->\nStack\n<!-- git-stk:end -->\n\nMore content after.";

        let result = strip_callout(body);

        assert_eq!(result, "Some intro text.\n\nMore content after.");
    }

    #[test]
    fn test_strip_callout_at_end() {
        let body = "User content here.\n\n<!-- git-stk:begin (do not edit) -->\nStack\n<!-- git-stk:end -->";

        let result = strip_callout(body);

        assert_eq!(result, "User content here.");
    }

    #[test]
    fn test_strip_callout_no_callout() {
        let body = "Just user content.";

        let result = strip_callout(body);

        assert_eq!(result, "Just user content.");
    }

    #[test]
    fn test_strip_callout_only_callout() {
        let body = "<!-- git-stk:begin (do not edit) -->\nStack\n<!-- git-stk:end -->";

        let result = strip_callout(body);

        assert_eq!(result, "");
    }
}
