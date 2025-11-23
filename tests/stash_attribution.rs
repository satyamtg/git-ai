mod repos;

use repos::test_file::ExpectedLineExt;
use repos::test_repo::TestRepo;

#[test]
fn test_stash_pop_with_ai_attribution() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create a file with AI attribution
    let mut example = repo.filename("example.txt");
    example.set_contents(vec!["line 1".ai(), "line 2".ai(), "line 3".ai()]);

    // Run checkpoint to track AI attribution
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Stash the changes
    repo.git(&["stash", "push", "-m", "test stash"])
        .expect("stash should succeed");

    // Verify file is gone
    assert!(repo.read_file("example.txt").is_none());

    // Pop the stash
    repo.git(&["stash", "pop"])
        .expect("stash pop should succeed");

    // Verify file is back
    assert!(repo.read_file("example.txt").is_some());

    // Commit the changes
    let commit = repo
        .stage_all_and_commit("apply stashed changes")
        .expect("commit should succeed");

    // Verify AI attribution is preserved
    let blame_output = repo
        .git_ai(&["blame", "example.txt"])
        .expect("blame should succeed");

    println!("Blame output:\n{}", blame_output);

    // Should show AI attribution (mock_ai)
    assert!(
        blame_output.contains("mock_ai"),
        "Expected AI attribution from stash, but got: {}",
        blame_output
    );

    // Check authorship log has AI prompts
    assert!(
        !commit.authorship_log.metadata.prompts.is_empty(),
        "Expected AI prompts in authorship log"
    );
}

#[test]
fn test_stash_apply_with_ai_attribution() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create a file with AI attribution
    let mut example = repo.filename("example.txt");
    example.set_contents(vec!["line 1".ai(), "line 2".ai()]);

    // Run checkpoint to track AI attribution
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Stash the changes
    repo.git(&["stash"]).expect("stash should succeed");

    // Apply (not pop) the stash
    repo.git(&["stash", "apply"])
        .expect("stash apply should succeed");

    // Commit the changes
    let commit = repo
        .stage_all_and_commit("apply stashed changes")
        .expect("commit should succeed");

    // Verify AI attribution is preserved
    let blame_output = repo
        .git_ai(&["blame", "example.txt"])
        .expect("blame should succeed");

    assert!(
        blame_output.contains("mock_ai"),
        "Expected AI attribution from stash, but got: {}",
        blame_output
    );

    // Check authorship log has AI prompts
    assert!(
        !commit.authorship_log.metadata.prompts.is_empty(),
        "Expected AI prompts in authorship log"
    );
}

#[test]
fn test_stash_apply_named_reference() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create first stash
    let mut file1 = repo.filename("file1.txt");
    file1.set_contents(vec!["first stash".ai()]);
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");
    repo.git(&["stash"]).expect("first stash should succeed");

    // Create second stash
    let mut file2 = repo.filename("file2.txt");
    file2.set_contents(vec!["second stash".ai()]);
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");
    repo.git(&["stash"]).expect("second stash should succeed");

    // Apply the first stash (stash@{1})
    repo.git(&["stash", "apply", "stash@{1}"])
        .expect("stash apply stash@{1} should succeed");

    // Verify file1 is back
    assert!(repo.read_file("file1.txt").is_some());
    assert!(repo.read_file("file2.txt").is_none());

    // Commit and verify attribution
    let commit = repo
        .stage_all_and_commit("apply first stash")
        .expect("commit should succeed");

    let blame_output = repo
        .git_ai(&["blame", "file1.txt"])
        .expect("blame should succeed");

    assert!(
        blame_output.contains("mock_ai"),
        "Expected AI attribution from stash@{{1}}, but got: {}",
        blame_output
    );

    assert!(
        !commit.authorship_log.metadata.prompts.is_empty(),
        "Expected AI prompts in authorship log"
    );
}

#[test]
fn test_stash_multiple_files() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create multiple files with AI attribution
    let mut file1 = repo.filename("file1.txt");
    file1.set_contents(vec!["file 1 line 1".ai(), "file 1 line 2".ai()]);

    let mut file2 = repo.filename("file2.txt");
    file2.set_contents(vec!["file 2 line 1".ai(), "file 2 line 2".ai()]);

    let mut file3 = repo.filename("file3.txt");
    file3.set_contents(vec!["file 3 line 1".ai()]);

    // Run checkpoint to track AI attribution
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Stash all changes
    repo.git(&["stash"]).expect("stash should succeed");

    // Verify files are gone
    assert!(repo.read_file("file1.txt").is_none());
    assert!(repo.read_file("file2.txt").is_none());
    assert!(repo.read_file("file3.txt").is_none());

    // Pop the stash
    repo.git(&["stash", "pop"])
        .expect("stash pop should succeed");

    // Commit all files
    let commit = repo
        .stage_all_and_commit("apply multi-file stash")
        .expect("commit should succeed");

    // Verify all files have AI attribution
    for file in &["file1.txt", "file2.txt", "file3.txt"] {
        let blame_output = repo
            .git_ai(&["blame", file])
            .expect(&format!("blame {} should succeed", file));

        assert!(
            blame_output.contains("mock_ai"),
            "Expected AI attribution in {}, but got: {}",
            file,
            blame_output
        );
    }

    // Check authorship log has the files
    assert!(
        !commit.authorship_log.metadata.prompts.is_empty(),
        "Expected AI prompts in authorship log"
    );
    assert_eq!(
        commit.authorship_log.attestations.len(),
        3,
        "Expected 3 files in authorship log"
    );
}

#[test]
fn test_stash_with_existing_initial_attributions() {
    // Test that stash attributions merge correctly with existing INITIAL attributions
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create a file and commit it (this will have some attribution)
    let mut example = repo.filename("example.txt");
    example.set_contents(vec!["existing line".human()]);
    let _first_commit = repo
        .stage_all_and_commit("add example")
        .expect("commit should succeed");

    // Modify the file with AI
    example.set_contents(vec!["existing line".human(), "new AI line".ai()]);

    // Run checkpoint
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Stash the changes
    repo.git(&["stash"]).expect("stash should succeed");

    // Verify file reverted to original
    let content = repo.read_file("example.txt").expect("file should exist");
    assert_eq!(content.lines().count(), 1, "Should have reverted to 1 line");

    // Pop the stash
    repo.git(&["stash", "pop"])
        .expect("stash pop should succeed");

    // Commit
    let commit = repo
        .stage_all_and_commit("apply stash")
        .expect("commit should succeed");

    // Verify mixed attribution
    let blame_output = repo
        .git_ai(&["blame", "example.txt"])
        .expect("blame should succeed");

    println!("Mixed blame output:\n{}", blame_output);

    // Should show AI attribution for line 2
    assert!(
        blame_output.contains("mock_ai"),
        "Expected AI attribution in mixed file, but got: {}",
        blame_output
    );

    // Should have both human and AI in authorship
    assert!(
        !commit.authorship_log.metadata.prompts.is_empty(),
        "Expected AI prompts in authorship log"
    );
}

#[test]
fn test_stash_pop_default_reference() {
    // Test that stash pop defaults to stash@{0}
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create AI content
    let mut example = repo.filename("example.txt");
    example.set_contents(vec!["AI content".ai()]);
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Stash without explicit reference
    repo.git(&["stash"]).expect("stash should succeed");

    // Pop without explicit reference (should use stash@{0})
    repo.git(&["stash", "pop"])
        .expect("stash pop should succeed");

    // Commit and verify
    let commit = repo
        .stage_all_and_commit("apply default stash")
        .expect("commit should succeed");

    let blame_output = repo
        .git_ai(&["blame", "example.txt"])
        .expect("blame should succeed");

    assert!(
        blame_output.contains("mock_ai"),
        "Expected AI attribution from default stash, but got: {}",
        blame_output
    );

    assert!(
        !commit.authorship_log.metadata.prompts.is_empty(),
        "Expected AI prompts in authorship log"
    );
}

#[test]
fn test_stash_pop_empty_repo() {
    // Test that stash operations don't crash on edge cases
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Try to pop when there's no stash - should fail gracefully
    let result = repo.git(&["stash", "pop"]);
    assert!(result.is_err(), "Should fail when no stash exists");
}

#[test]
fn test_stash_mixed_human_and_ai() {
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create file with mixed attribution
    let mut example = repo.filename("example.txt");
    example.set_contents(vec![
        "line 1".human(),
        "line 2".ai(),
        "line 3".human(),
        "line 4".ai(),
    ]);

    // Run checkpoint
    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Stash and pop
    repo.git(&["stash"]).expect("stash should succeed");
    repo.git(&["stash", "pop"])
        .expect("stash pop should succeed");

    // Commit
    let commit = repo
        .stage_all_and_commit("mixed content")
        .expect("commit should succeed");

    // Verify blame shows mixed attribution
    let blame_output = repo
        .git_ai(&["blame", "example.txt"])
        .expect("blame should succeed");

    println!("Mixed attribution blame:\n{}", blame_output);

    // Should have AI attribution
    assert!(
        blame_output.contains("mock_ai"),
        "Expected AI attribution in mixed file"
    );

    // Should have human attribution (Test User)
    assert!(
        blame_output.contains("Test User"),
        "Expected human attribution in mixed file"
    );

    // Authorship log should have AI prompts
    assert!(
        !commit.authorship_log.metadata.prompts.is_empty(),
        "Expected AI prompts in authorship log"
    );
}
