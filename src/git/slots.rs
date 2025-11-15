use anyhow::{bail, Context, Result};
use git2::Repository;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;

use crate::model::Config;

/// Slot counter cache - tracks used slots and counters per branch
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SlotCache {
    /// Map from branch name to next numeric slot counter
    counters: HashMap<String, u32>,
    /// Map from branch name to set of all used slots (for uniqueness checking)
    #[serde(default)]
    used_slots: HashMap<String, HashSet<String>>,
}

impl SlotCache {
    /// Load the slot cache from disk
    pub fn load(repo: &Repository) -> Result<Self> {
        let path = Config::slots_cache_path(repo)?;

        if !path.exists() {
            return Ok(Self::default());
        }

        let contents = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read slot cache from {:?}", path))?;

        let cache: SlotCache = serde_json::from_str(&contents)
            .context("Failed to parse slot cache JSON")?;

        Ok(cache)
    }

    /// Save the slot cache to disk
    pub fn save(&self, repo: &Repository) -> Result<()> {
        let path = Config::slots_cache_path(repo)?;

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {:?}", parent))?;
        }

        let json = serde_json::to_string_pretty(self)
            .context("Failed to serialize slot cache")?;

        fs::write(&path, json)
            .with_context(|| format!("Failed to write slot cache to {:?}", path))?;

        Ok(())
    }

    /// Get the current counter for a branch
    fn get_counter(&self, branch: &str) -> u32 {
        self.counters.get(branch).copied().unwrap_or(0)
    }

    /// Get the set of used slots for a branch
    fn get_used_slots(&self, branch: &str) -> HashSet<String> {
        self.used_slots
            .get(branch)
            .cloned()
            .unwrap_or_default()
    }

    /// Allocate a new numeric slot for a branch
    /// Returns a two-digit string like "01", "02", etc.
    pub fn allocate(&mut self, branch: &str) -> String {
        let current = self.get_counter(branch);
        let next = current + 1;
        self.counters.insert(branch.to_string(), next);

        // Format as two-digit string
        let slot = format!("{:02}", next);

        // Mark as used
        self.used_slots
            .entry(branch.to_string())
            .or_insert_with(HashSet::new)
            .insert(slot.clone());

        slot
    }

    /// Check if a slot is available for a branch
    pub fn is_slot_available(&self, branch: &str, slot: &str) -> bool {
        let used = self.get_used_slots(branch);
        !used.contains(slot)
    }

    /// Mark a slot as used for a branch
    pub fn mark_slot_used(&mut self, branch: &str, slot: &str) {
        self.used_slots
            .entry(branch.to_string())
            .or_insert_with(HashSet::new)
            .insert(slot.to_string());

        // If it's a numeric slot, update counter
        if let Ok(num) = slot.parse::<u32>() {
            let current = self.get_counter(branch);
            if num > current {
                self.counters.insert(branch.to_string(), num);
            }
        }
    }

    /// Ensure a slot is tracked (called during reconciliation)
    pub fn ensure_slot(&mut self, branch: &str, slot: &str) {
        self.mark_slot_used(branch, slot);
    }
}

/// Validate a slot name for branch compatibility
/// Slots must:
/// - Not be empty
/// - Contain only alphanumeric, hyphens, and underscores
/// - Not start or end with a hyphen
pub fn validate_slot_name(slot: &str) -> Result<()> {
    if slot.is_empty() {
        bail!("Slot name cannot be empty");
    }

    if slot.starts_with('-') || slot.ends_with('-') {
        bail!("Slot name cannot start or end with a hyphen");
    }

    // Check for valid characters
    for c in slot.chars() {
        if !c.is_alphanumeric() && c != '-' && c != '_' {
            bail!("Slot name can only contain alphanumeric characters, hyphens, and underscores. Invalid character: '{}'", c);
        }
    }

    Ok(())
}

/// Sanitize a branch name for use in a ref
/// - Collapse multiple slashes to single
/// - Replace illegal characters
/// - Trim to ≤250 bytes
/// - Strip trailing dots
pub fn sanitize_branch_name(name: &str) -> String {
    let mut result = name.to_string();

    // Collapse multiple slashes
    while result.contains("//") {
        result = result.replace("//", "/");
    }

    // Replace illegal git ref characters
    // Git ref names cannot contain: \x00-\x1F, \x7F, ~, ^, :, ?, *, [, \, space, .., @{
    let illegal_chars = [
        '\0', '\x01', '\x02', '\x03', '\x04', '\x05', '\x06', '\x07',
        '\x08', '\x09', '\x0A', '\x0B', '\x0C', '\x0D', '\x0E', '\x0F',
        '\x10', '\x11', '\x12', '\x13', '\x14', '\x15', '\x16', '\x17',
        '\x18', '\x19', '\x1A', '\x1B', '\x1C', '\x1D', '\x1E', '\x1F',
        '\x7F', '~', '^', ':', '?', '*', '[', '\\', ' ',
    ];

    for c in illegal_chars {
        result = result.replace(c, "-");
    }

    // Replace ".." with "-"
    result = result.replace("..", "-");

    // Replace "@{" with "-"
    result = result.replace("@{", "-");

    // Trim to 250 bytes
    if result.len() > 250 {
        result.truncate(250);
    }

    // Strip trailing dots
    result = result.trim_end_matches('.').to_string();

    // Strip leading/trailing slashes
    result = result.trim_matches('/').to_string();

    result
}

/// Generate a head ref name for a commit
/// Format: {branch}--{slot}
/// Examples:
///   - "feature/foo--01" (numeric slot)
///   - "feature/foo--add-tests" (custom slot)
/// Uses -- separator to avoid directory conflicts with current branch
pub fn generate_head_ref(branch: &str, slot: &str) -> String {
    let sanitized = sanitize_branch_name(branch);
    format!("{}--{}", sanitized, slot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_slot_name() {
        assert!(validate_slot_name("01").is_ok());
        assert!(validate_slot_name("add-tests").is_ok());
        assert!(validate_slot_name("feature_123").is_ok());
        assert!(validate_slot_name("FIX-bug").is_ok());

        assert!(validate_slot_name("").is_err());
        assert!(validate_slot_name("-start").is_err());
        assert!(validate_slot_name("end-").is_err());
        assert!(validate_slot_name("has space").is_err());
        assert!(validate_slot_name("has/slash").is_err());
        assert!(validate_slot_name("has.dot").is_err());
    }

    #[test]
    fn test_sanitize_branch_name() {
        assert_eq!(sanitize_branch_name("feature/foo"), "feature/foo");
        assert_eq!(sanitize_branch_name("feature//foo"), "feature/foo");
        assert_eq!(sanitize_branch_name("feature///foo"), "feature/foo");
        assert_eq!(sanitize_branch_name("feature:foo"), "feature-foo");
        assert_eq!(sanitize_branch_name("feature foo"), "feature-foo");
        assert_eq!(sanitize_branch_name("feature..foo"), "feature-foo");
        assert_eq!(sanitize_branch_name("feature@{foo"), "feature-foo");
        assert_eq!(sanitize_branch_name("feature/foo."), "feature/foo");
        assert_eq!(sanitize_branch_name("/feature/foo/"), "feature/foo");
    }

    #[test]
    fn test_generate_head_ref() {
        assert_eq!(generate_head_ref("feature/foo", "01"), "feature/foo--01");
        assert_eq!(generate_head_ref("feature/foo", "42"), "feature/foo--42");
        assert_eq!(generate_head_ref("feature/foo", "add-tests"), "feature/foo--add-tests");
        assert_eq!(generate_head_ref("feature//foo", "01"), "feature/foo--01");
    }

    #[test]
    fn test_slot_cache_allocation() {
        let mut cache = SlotCache::default();

        assert_eq!(cache.allocate("main"), "01");
        assert_eq!(cache.allocate("main"), "02");
        assert_eq!(cache.allocate("feature"), "01");
        assert_eq!(cache.allocate("main"), "03");
    }

    #[test]
    fn test_slot_cache_availability() {
        let mut cache = SlotCache::default();

        cache.mark_slot_used("main", "01");
        cache.mark_slot_used("main", "custom-slot");

        assert!(!cache.is_slot_available("main", "01"));
        assert!(!cache.is_slot_available("main", "custom-slot"));
        assert!(cache.is_slot_available("main", "02"));
        assert!(cache.is_slot_available("main", "other-custom"));
    }
}
