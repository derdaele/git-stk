use anyhow::Result;
use crate::common::TestEnv;

/// Basic Land Workflow Test
/// Tests landing commits and verifying they are marked as merged
#[tokio::test]
#[ignore] // Run with: cargo test --test land_tests -- --ignored
async fn test_basic_land_workflow() -> Result<()> {
    println!("\n=== Basic Land Workflow Test ===\n");

    let env = TestEnv::setup()?;

    // Step 1: Create two commits
    println!("Step 1: Creating 2 commits...");
    let sha1 = env.repo.create_commit("feat: implement feature X")?;
    let sha2 = env.repo.create_commit("feat: implement feature Y")?;
    println!("  ✓ Created commits: {}, {}\n", sha1, sha2);

    // Step 2: Export
    println!("Step 2: Exporting stack...");
    let export_output = env.export_default()?;
    println!("{}", export_output);

    // Step 3: Assert view shows both commits synced
    println!("\nStep 3: Verifying both commits synced...");
    let view = env.assert_view()?;
    view.has_commits(2)
        .commit(1)
            .has_title("feat: implement feature X")
            .has_slot("01")
            .is_synced();
    view.commit(2)
        .has_title("feat: implement feature Y")
        .has_slot("02")
        .is_synced();
    println!("  ✓ Both commits synced with slots 01 and 02\n");

    // Step 4: Land first commit
    println!("Step 4: Landing first commit...");
    let land_output = env.land()?;
    println!("{}", land_output);

    // Step 5: Assert view shows only second commit (first was merged and removed from stack)
    println!("\nStep 5: Verifying first commit landed...");
    let view = env.assert_view()?;
    view.has_commits(1)
        .commit(1)
            .has_title("feat: implement feature Y")
            .has_slot("02")
            .is_synced();
    println!("  ✓ First commit landed, second commit remains in stack\n");

    // Step 6: Land second commit
    println!("Step 6: Landing second commit...");
    let land_output = env.land()?;
    println!("{}", land_output);

    // Step 7: Assert view is empty (all commits landed)
    println!("\nStep 7: Verifying all commits landed...");
    let view = env.assert_view()?;
    view.has_commits(0);
    println!("  ✓ All commits landed, stack is empty\n");

    println!("=== ✅ PASSED ===\n");
    Ok(())
}
