use anyhow::Result;
use crate::common::TestEnv;

/// Rebase Reordering Test
/// Tests that reordering commits via rebase updates slot assignments correctly
#[tokio::test]
#[ignore] // Run with: cargo test --test rebase_tests -- --ignored
async fn test_rebase_reorder() -> Result<()> {
    println!("\n=== Rebase Reordering Test ===\n");

    let env = TestEnv::setup()?;

    // Step 1: Add 3 commits
    println!("Step 1: Creating 3 commits...");
    let sha1 = env.repo.create_commit("feat: first feature")?;
    let sha2 = env.repo.create_commit("feat: second feature")?;
    let sha3 = env.repo.create_commit("feat: third feature")?;
    println!("  ✓ Created commits: {}, {}, {}\n", sha1, sha2, sha3);

    // Step 2: Export
    println!("Step 2: Exporting initial stack...");
    let export_output = env.export_default()?;
    println!("{}", export_output);

    // Step 3: Verify initial state - slots assigned 01, 02, 03
    println!("\nStep 3: Verifying initial slot assignments...");
    let view = env.assert_view()?;
    view.has_commits(3)
        .commit(1)
            .has_title("feat: first feature")
            .has_slot("01")
            .is_synced();
    view.commit(2)
        .has_title("feat: second feature")
        .has_slot("02")
        .is_synced();
    view.commit(3)
        .has_title("feat: third feature")
        .has_slot("03")
        .is_synced();
    println!("  ✓ Slots: 01, 02, 03\n");

    // Step 4: Rebase - move commit 3 to position 1
    println!("Step 4: Reordering commits (moving {} to first)...", sha3);
    env.repo.rebase(&[
        ("pick", &sha3),
        ("pick", &sha1),
        ("pick", &sha2),
    ], None)?;
    println!("  ✓ Rebase complete\n");

    // Step 4b: View should show Export needed before exporting
    println!("Step 4b: Verifying export needed after rebase...");
    let view = env.assert_view()?;
    view.has_commits(3)
        .commit(1)
            .has_title("feat: third feature")
            .has_slot("03")
            .is_export_needed();
    view.commit(2)
        .has_title("feat: first feature")
        .has_slot("01")
        .is_export_needed();
    view.commit(3)
        .has_title("feat: second feature")
        .has_slot("02")
        .is_export_needed();
    println!("  ✓ All commits need export\n");

    // Step 5: Export after rebase
    println!("Step 5: Exporting after rebase...");
    let export_output = env.export_default()?;
    println!("{}", export_output);

    // Step 6: Verify all PRs are still open and slots are preserved with commits
    println!("\nStep 6: Verifying PRs and slots (slots stay with commits)...");
    let view = env.assert_view()?;
    view.has_commits(3)
        .commit(1)
            .has_title("feat: third feature")
            .has_slot("03")  // Slot stays with commit, not position
            .is_synced();
    view.commit(2)
        .has_title("feat: first feature")
        .has_slot("01")  // Slot stays with commit
        .is_synced();
    view.commit(3)
        .has_title("feat: second feature")
        .has_slot("02")  // Slot stays with commit
        .is_synced();
    println!("  ✓ All commits reordered, slots preserved with commits\n");

    // Step 7: Verify PRs on GitHub match the slots and have correct bases
    println!("Step 7: Verifying all PRs are open with correct slots and bases...");

    let base_branch = format!("{}-base", env.test_id);
    let head_03 = format!("{}-feature--03", env.test_id);
    let head_01 = format!("{}-feature--01", env.test_id);

    // Slot 03 (position 1): "third feature", base = main base branch
    env.assert_github()
        .pr_with_slot(&env.test_id, "03")
        .fetch()
        .await?
        .has_title("feat: third feature")
        .has_base(&base_branch)
        .is_open();

    // Slot 01 (position 2): "first feature", base = slot 03 (previous in stack)
    env.assert_github()
        .pr_with_slot(&env.test_id, "01")
        .fetch()
        .await?
        .has_title("feat: first feature")
        .has_base(&head_03)
        .is_open();

    // Slot 02 (position 3): "second feature", base = slot 01 (previous in stack)
    env.assert_github()
        .pr_with_slot(&env.test_id, "02")
        .fetch()
        .await?
        .has_title("feat: second feature")
        .has_base(&head_01)
        .is_open();

    println!("  ✓ All 3 PRs are open with correct stacking\n");

    println!("=== ✅ PASSED ===\n");
    Ok(())
}

/// Squash Metadata Reconciliation Test
/// Tests that squashing commits correctly reconciles metadata from either commit
#[tokio::test]
#[ignore] // Run with: cargo test --test rebase_tests -- --ignored
async fn test_squash_metadata_reconciliation() -> Result<()> {
    println!("\n=== Squash Metadata Reconciliation Test ===\n");

    let env = TestEnv::setup()?;

    // Step 1: Create two commits A and B
    println!("Step 1: Creating 2 commits (A and B)...");
    let sha_a = env.repo.create_commit("feat: commit A")?;
    let sha_b = env.repo.create_commit("feat: commit B")?;
    println!("  ✓ Created A: {}, B: {}\n", sha_a, sha_b);

    // Step 2: Export
    println!("Step 2: Exporting initial stack...");
    let export_output = env.export_default()?;
    println!("{}", export_output);

    // Step 3: Assert view shows both commits synced
    println!("\nStep 3: Verifying both commits synced...");
    let view = env.assert_view()?;
    view.has_commits(2)
        .commit(1)
            .has_title("feat: commit A")
            .has_slot("01")
            .is_synced();
    view.commit(2)
        .has_title("feat: commit B")
        .has_slot("02")
        .is_synced();
    println!("  ✓ A has slot 01, B has slot 02\n");

    // Step 4: Save HEAD position for later reset
    println!("Step 4: Saving HEAD position...");
    let saved_head = env.repo.repo()?.head()?.peel_to_commit()?.id().to_string();
    println!("  ✓ Saved: {}\n", &saved_head[..7]);

    // Step 5: Squash B into A (keep A's message)
    println!("Step 5: Squashing B into A...");
    env.repo.rebase(&[
        ("pick", &sha_a),
        ("squash", &sha_b),
    ], None)?;  // No custom message, use default
    println!("  ✓ Squash complete\n");

    // Step 6: Assert view shows 1 commit with slot from A (01)
    println!("Step 6: Verifying squashed commit has A's slot...");
    let view = env.assert_view()?;
    view.has_commits(1)
        .commit(1)
            .has_title("feat: commit A")
            .has_slot("01")
            .is_export_needed();
    println!("  ✓ Squashed commit inherited slot 01 from A\n");

    // Step 7: Reset hard to saved position
    println!("Step 7: Resetting to saved position...");
    std::process::Command::new("git")
        .current_dir(env.path())
        .args(["reset", "--hard", &saved_head])
        .output()?;
    println!("  ✓ Reset to 2 commits\n");

    // Step 8: Squash A into B with custom message
    println!("Step 8: Reordering and squashing A into B with custom message...");
    env.repo.rebase(&[
        ("pick", &sha_b),
        ("squash", &sha_a),
    ], Some("feat: combined commit A and B"))?;
    println!("  ✓ Squash complete with custom message\n");

    // Step 9: Assert view shows 1 commit with slot from B and custom message
    println!("Step 9: Verifying squashed commit has B's slot and custom message...");
    let view = env.assert_view()?;
    view.has_commits(1)
        .commit(1)
            .has_title("feat: combined commit A and B")
            .has_slot("02")
            .is_export_needed();
    println!("  ✓ Squashed commit inherited slot 02 from B with custom message\n");

    // Step 10: Export
    println!("Step 10: Exporting squashed commit...");
    let export_output = env.export_default()?;
    println!("{}", export_output);

    // Step 11: Assert view shows synced with custom message
    println!("\nStep 11: Verifying commit is synced...");
    let view = env.assert_view()?;
    view.has_commits(1)
        .commit(1)
            .has_title("feat: combined commit A and B")
            .has_slot("02")
            .is_synced();
    println!("  ✓ Commit synced with slot 02 and custom message\n");

    // Step 12: Assert GitHub PR
    // Note: PR title is NOT automatically updated when commit message changes.
    // This is by design - PR titles may be manually customized and shouldn't auto-sync.
    // TODO: Consider adding --update-pr-titles flag in the future if this behavior is desired.
    println!("Step 12: Verifying PR on GitHub...");
    let base_branch = format!("{}-base", env.test_id);
    env.assert_github()
        .pr_with_slot(&env.test_id, "02")
        .fetch()
        .await?
        .has_title("feat: commit B")  // Original title, not updated
        .has_base(&base_branch)
        .is_open();
    println!("  ✓ PR verified on GitHub (title unchanged)\n");

    println!("=== ✅ PASSED ===\n");
    Ok(())
}
