use anyhow::Result;
use crate::common::TestEnv;

/// Slot Assignment Test
/// Tests manual slot assignment and predicted slot reconciliation
#[tokio::test]
#[ignore] // Run with: cargo test --test slot_tests -- --ignored
async fn test_slot_assignment() -> Result<()> {
    println!("\n=== Slot Assignment Test ===\n");

    let env = TestEnv::setup()?;

    // Step 1: Create 3 commits
    println!("Step 1: Creating 3 commits...");
    let sha1 = env.repo.create_commit("feat: first commit")?;
    let sha2 = env.repo.create_commit("feat: second commit")?;
    let sha3 = env.repo.create_commit("feat: third commit")?;
    println!("  ✓ Created commits: {}, {}, {}\n", sha1, sha2, sha3);

    // Step 2: Assert view shows predicted slots
    println!("Step 2: Verifying predicted slots...");
    let view = env.assert_view()?;
    view.has_commits(3)
        .commit(1)
            .has_title("feat: first commit")
            .slot_to_be_assigned("01")
            .no_pr();
    view.commit(2)
        .has_title("feat: second commit")
        .slot_to_be_assigned("02")
        .no_pr();
    view.commit(3)
        .has_title("feat: third commit")
        .slot_to_be_assigned("03")
        .no_pr();
    println!("  ✓ Predicted slots: 01, 02, 03\n");

    // Step 3: Set slot for last commit using "last" keyword
    println!("Step 3: Setting slot for last commit...");
    env.set_slot("last", "adding-test")?;
    println!("  ✓ Set last commit to slot 'adding-test'\n");

    // Step 4: Set slot by commit index on first commit
    println!("Step 4: Setting slot for first commit (index 1)...");
    env.set_slot("1", "initial-commit")?;
    println!("  ✓ Set commit 1 to slot 'initial-commit'\n");

    // Step 5: Assert view shows assigned and predicted slots
    println!("Step 5: Verifying slot assignments...");
    let view = env.assert_view()?;
    view.has_commits(3)
        .commit(1)
            .has_title("feat: first commit")
            .has_slot("initial-commit")
            .no_pr();
    view.commit(2)
        .has_title("feat: second commit")
            .slot_to_be_assigned("01")  // First available numeric slot
            .no_pr();
    view.commit(3)
        .has_title("feat: third commit")
        .has_slot("adding-test")
        .no_pr();
    println!("  ✓ First: slot 'initial-commit', Second: predicted '01', Third: slot 'adding-test'\n");

    println!("=== ✅ PASSED ===\n");
    Ok(())
}

/// Slot Change with PR Test
/// Tests changing slots on commits that have PRs, with prompt handling
#[tokio::test]
#[ignore] // Run with: cargo test --test slot_tests -- --ignored
async fn test_slot_change_with_pr() -> Result<()> {
    println!("\n=== Slot Change with PR Test ===\n");

    let env = TestEnv::setup()?;

    // Step 1: Create a commit
    println!("Step 1: Creating a commit...");
    let sha1 = env.repo.create_commit("feat: test slot change")?;
    println!("  ✓ Created commit: {}\n", sha1);

    // Step 2: Export the commit
    println!("Step 2: Exporting commit...");
    let export_output = env.export_default()?;
    println!("{}", export_output);

    // Step 3: Assert view and retrieve PR number
    println!("\nStep 3: Verifying commit synced and getting PR number...");
    let view = env.assert_view()?;
    view.has_commits(1)
        .commit(1)
            .has_title("feat: test slot change")
            .has_slot("01")
            .is_synced()
            .has_pr_number();

    let pr_number = view.commit(1).pr_number().unwrap();
    println!("  ✓ Commit synced with slot '01', PR #{}\n", pr_number);

    // Step 4: Try to set slot without --yes flag (simulates user declining)
    println!("Step 4: Attempting to change slot to 'new-slot' (without --yes flag)...");
    let result = env.set_slot_no_confirm("1", "new-slot");

    // Command should fail because we're not in an interactive terminal
    assert!(result.is_err(), "Expected command to fail in non-interactive mode");
    println!("  ✓ Command failed as expected (no terminal)\n");

    // Step 5: Assert view shows slot didn't change
    println!("Step 5: Verifying slot unchanged...");
    let view = env.assert_view()?;
    view.has_commits(1)
        .commit(1)
            .has_title("feat: test slot change")
            .has_slot("01")  // Should still be 01
            .is_synced();
    println!("  ✓ Slot remains '01'\n");

    // Step 6: Assert PR is still open
    println!("Step 6: Verifying PR still open...");
    env.assert_github()
        .pr_with_number(pr_number)
        .fetch()
        .await?
        .is_open();
    println!("  ✓ PR #{} is still open\n", pr_number);

    // Step 7: Set slot with --yes flag to auto-confirm
    println!("Step 7: Changing slot to 'updated-slot' (with --yes flag)...");
    env.set_slot_yes("1", "updated-slot")?;
    println!("  ✓ Slot changed to 'updated-slot'\n");

    // Step 8: Assert PR was closed
    println!("Step 8: Verifying PR was closed...");
    env.assert_github()
        .pr_with_number(pr_number)
        .fetch()
        .await?
        .is_closed();
    println!("  ✓ PR #{} is now closed\n", pr_number);

    // Step 9: Assert view shows new slot and no PR
    println!("Step 9: Verifying new slot and PR status...");
    let view = env.assert_view()?;
    view.has_commits(1)
        .commit(1)
            .has_title("feat: test slot change")
            .has_slot("updated-slot")
            .no_pr();
    println!("  ✓ Slot is 'updated-slot', no PR\n");

    println!("=== ✅ PASSED ===\n");
    Ok(())
}
