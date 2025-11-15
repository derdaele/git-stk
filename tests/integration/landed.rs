use anyhow::Result;
use crate::common::TestEnv;

/// Landed Command Test
/// Tests detecting and cleaning up commits that were merged externally on GitHub
#[tokio::test]
#[ignore] // Run with: cargo test --test landed_tests -- --ignored
async fn test_externally_merged_pr() -> Result<()> {
    println!("\n=== Externally Merged PR Test ===\n");

    let env = TestEnv::setup()?;

    // Step 1: Create two commits
    println!("Step 1: Creating 2 commits...");
    let sha1 = env.repo.create_commit("feat: external merge test A")?;
    let sha2 = env.repo.create_commit("feat: external merge test B")?;
    println!("  ✓ Created commits: {}, {}\n", sha1, sha2);

    // Step 2: Export
    println!("Step 2: Exporting stack...");
    let export_output = env.export_default()?;
    println!("{}", export_output);

    // Step 3: Verify both commits synced
    println!("\nStep 3: Verifying both commits synced...");
    let view = env.assert_view()?;
    view.has_commits(2)
        .commit(1)
            .has_title("feat: external merge test A")
            .has_slot("01")
            .is_synced();
    view.commit(2)
        .has_title("feat: external merge test B")
        .has_slot("02")
            .is_synced();
    println!("  ✓ Both commits synced\n");

    // Step 4: Merge first PR on GitHub (externally, not using git-stk land)
    println!("Step 4: Merging first PR on GitHub...");
    env.merge_pr_on_github("01").await?;
    println!("  ✓ PR #01 merged on GitHub\n");

    // Step 5: Assert view shows first commit merged, second still synced
    println!("Step 5: Verifying view after external merge...");
    let view = env.assert_view()?;
    view.has_commits(2)
        .commit(1)
            .has_title("feat: external merge test A")
            .has_slot("01")
            .is_merged();
    view.commit(2)
        .has_title("feat: external merge test B")
        .has_slot("02")
            .is_synced();
    println!("  ✓ First commit marked as merged, second still synced\n");

    // Step 6: Run landed command to clean up
    println!("Step 6: Running landed command...");
    let landed_output = env.landed()?;
    println!("{}", landed_output);

    // Step 7: Assert view shows only second commit
    println!("\nStep 7: Verifying first commit removed from stack...");
    let view = env.assert_view()?;
    view.has_commits(1)
        .commit(1)
            .has_title("feat: external merge test B")
            .has_slot("02")
            .is_synced();
    println!("  ✓ First commit removed, second commit remains synced\n");

    println!("=== ✅ PASSED ===\n");
    Ok(())
}
