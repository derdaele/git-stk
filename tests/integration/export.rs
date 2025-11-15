use anyhow::Result;
use crate::common::TestEnv;

/// Incremental Export Workflow
/// Tests the complete workflow of building a stack incrementally
#[tokio::test]
#[ignore] // Run with: cargo test --test export_tests -- --ignored
async fn test_incremental_export_workflow() -> Result<()> {
    println!("\n=== Incremental Export Workflow ===\n");

    // Setup test environment
    let env = TestEnv::setup()?;

    // Step 1: Assert empty stack
    println!("Step 1: Verify empty stack...");
    env.assert_view()?
        .has_commits(0);
    println!("  ✓ Empty stack verified\n");

    // Step 2: Add first commit
    println!("Step 2: Add first commit...");
    let _sha1 = env.repo.create_commit("feat: add authentication")?;
    println!("  ✓ Created commit\n");

    // Step 3: Assert view shows PR to be created with predicted slot 01
    println!("Step 3: Verify view shows PR to be created...");
    env.assert_view()?
        .has_commits(1)
        .commit(1)
            .has_title("feat: add authentication")
            .slot_to_be_assigned("01")
            .no_pr();
    println!("  ✓ Predicted slot: 01");
    println!("  ✓ Status: PR to be created\n");

    // Step 4: Export
    println!("Step 4: Export stack...");
    let export_output = env.export_default()?;
    println!("{}", export_output);

    // Step 5: Assert view shows synced
    println!("\nStep 5: Verify first commit is synced...");
    env.assert_view()?
        .has_commits(1)
        .commit(1)
            .has_slot("01")
            .has_title("feat: add authentication")
            .is_synced();
    println!("  ✓ Slot assigned: 01");
    println!("  ✓ Status: Synced\n");

    // Step 6: Verify PR on GitHub
    println!("Step 6: Verify PR on GitHub...");
    let base_branch = format!("{}-base", env.test_id);
    let head_1 = format!("{}-feature--01", env.test_id);

    env.assert_github()
        .pr_with_head(&head_1)
        .fetch()
        .await?
        .has_title("feat: add authentication")
        .has_base(&base_branch);
    println!("  ✓ PR title: feat: add authentication");
    println!("  ✓ PR base: {}\n", base_branch);

    // Step 7: Add second commit
    println!("Step 7: Add second commit...");
    let _sha2 = env.repo.create_commit("feat: add user profile")?;
    println!("  ✓ Created commit\n");

    // Step 8: Assert view shows first synced, second predicted
    println!("Step 8: Verify mixed state (first synced, second predicted)...");
    let view = env.assert_view()?;
    view.has_commits(2)
        .commit(1)
            .has_slot("01")
            .has_title("feat: add authentication")
            .is_synced();

    view.commit(2)
        .has_title("feat: add user profile")
        .slot_to_be_assigned("02")
        .no_pr()
        .has_no_status();
    println!("  ✓ First commit: Synced (slot 01)");
    println!("  ✓ Second commit: PR to be created (predicted slot 02)\n");

    // Step 9: Export again
    println!("Step 9: Export updated stack...");
    let export_output = env.export_default()?;
    println!("{}", export_output);

    // Step 10: Final assertion - both commits synced
    println!("\nStep 10: Verify both commits are synced...");
    let view = env.assert_view()?;
    view.has_commits(2)
        .commit(1)
            .has_slot("01")
            .has_title("feat: add authentication")
            .is_synced();

    view.commit(2)
        .has_slot("02")
        .has_title("feat: add user profile")
        .is_synced();
    println!("  ✓ Both commits synced\n");

    // Verify second PR on GitHub
    println!("Step 11: Verify second PR on GitHub...");
    let head_2 = format!("{}-feature--02", env.test_id);

    env.assert_github()
        .pr_with_head(&head_2)
        .fetch()
        .await?
        .has_title("feat: add user profile")
        .has_base(&head_1);  // Stacked on first PR
    println!("  ✓ PR title: feat: add user profile");
    println!("  ✓ PR base: {} (stacked)\n", head_1);

    println!("=== ✅ PASSED ===\n");
    Ok(())
}

/// Draft PR Export
/// Tests exporting commits as draft PRs
#[tokio::test]
#[ignore] // Run with: cargo test --test export_tests -- --ignored
async fn test_export_draft_pr() -> Result<()> {
    println!("\n=== Draft PR Export ===\n");

    // Setup test environment
    let env = TestEnv::setup()?;

    // Step 1: Create commit
    println!("Step 1: Create commit...");
    let _sha = env.repo.create_commit("feat: add search feature")?;
    println!("  ✓ Created commit\n");

    // Step 2: Export as draft
    println!("Step 2: Export as draft...");
    let export_output = env.export(true)?;  // draft = true
    println!("{}", export_output);

    // Step 3: Verify PR is created as draft on GitHub
    println!("\nStep 3: Verify PR is draft on GitHub...");
    let head = format!("{}-feature--01", env.test_id);

    env.assert_github()
        .pr_with_head(&head)
        .fetch()
        .await?
        .has_title("feat: add search feature")
        .is_draft();
    println!("  ✓ PR created as draft\n");

    println!("=== ✅ PASSED ===\n");
    Ok(())
}

/// Remote Branch Divergence Detection
/// Tests that view correctly detects when a remote branch has been modified externally
#[tokio::test]
#[ignore] // Run with: cargo test --test integration -- --ignored
async fn test_remote_branch_divergence() -> Result<()> {
    println!("\n=== Remote Branch Divergence Detection ===\n");

    // Setup test environment
    let env = TestEnv::setup()?;

    // Step 1: Create two commits
    println!("Step 1: Creating 2 commits...");
    let _sha1 = env.repo.create_commit("feat: first feature")?;
    let _sha2 = env.repo.create_commit("feat: second feature")?;
    println!("  ✓ Created 2 commits\n");

    // Step 2: Export
    println!("Step 2: Export stack...");
    let export_output = env.export_default()?;
    println!("{}", export_output);

    // Step 3: Verify both commits are synced
    println!("\nStep 3: Verify both commits are synced...");
    let view = env.assert_view()?;
    view.has_commits(2)
        .commit(1)
            .has_slot("01")
            .is_synced();
    view.commit(2)
        .has_slot("02")
        .is_synced();
    println!("  ✓ Both commits synced\n");

    // Step 4: Modify a file on the remote branch for the first commit
    println!("Step 4: Modifying remote branch externally...");
    env.modify_remote_branch("01", "README.md", "# Modified externally\n")?;
    println!();

    // Step 5: Assert view shows first commit needs export, second commit synced
    println!("Step 5: Verify divergence detection...");
    let view = env.assert_view()?;
    view.has_commits(2)
        .commit(1)
            .has_slot("01")
            .is_export_needed();
    view.commit(2)
        .has_slot("02")
        .is_synced();
    println!("  ✓ First commit: Export needed");
    println!("  ✓ Second commit: Synced\n");

    println!("=== ✅ PASSED ===\n");
    Ok(())
}
