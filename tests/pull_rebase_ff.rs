mod repos;

use repos::test_file::ExpectedLineExt;
use repos::test_repo::TestRepo;

/// Helper struct that provides a local repo with an upstream containing seeded commits.
/// The local repo is initially behind the upstream.
struct PullTestSetup {
    /// The local clone - initially behind upstream after setup
    local: TestRepo,
    /// The bare upstream repository (kept alive for the duration of the test)
    #[allow(dead_code)]
    upstream: TestRepo,
    /// SHA of the second commit (upstream is ahead by this)
    upstream_sha: String,
}

/// Creates a test setup for pull scenarios:
/// 1. Creates upstream (bare) and local (clone) repos
/// 2. Makes an initial commit in local, pushes to upstream  
/// 3. Makes a second commit in local, pushes to upstream
/// 4. Resets local back to initial commit (so local is behind upstream)
///
/// After this setup:
/// - upstream has 2 commits
/// - local has 1 commit (behind by 1)
/// - local can `git pull` to get the second commit
fn setup_pull_test() -> PullTestSetup {
    let (local, upstream) = TestRepo::new_with_remote();

    // Make initial commit in local and push
    let mut readme = local.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    let commit = local
        .stage_all_and_commit("initial commit")
        .expect("initial commit should succeed");

    let initial_sha = commit.commit_sha;

    // Push initial commit to upstream
    local
        .git(&["push", "-u", "origin", "HEAD"])
        .expect("push initial commit should succeed");

    // Make second commit (simulating remote changes)
    let mut file = local.filename("upstream_file.txt");
    file.set_contents(vec!["content from upstream".to_string()]);
    let commit = local
        .stage_all_and_commit("upstream commit")
        .expect("upstream commit should succeed");

    let upstream_sha = commit.commit_sha;

    // Push second commit to upstream
    local
        .git(&["push", "origin", "HEAD"])
        .expect("push upstream commit should succeed");

    // Reset local back to initial commit (so it's behind upstream)
    local
        .git(&["reset", "--hard", &initial_sha])
        .expect("reset to initial commit should succeed");

    // Verify local is behind
    assert!(
        local.read_file("upstream_file.txt").is_none(),
        "Local should not have upstream_file.txt after reset"
    );

    PullTestSetup {
        local,
        upstream,
        upstream_sha,
    }
}

#[test]
fn test_fast_forward_pull_preserves_ai_attribution() {
    let setup = setup_pull_test();
    let local = setup.local;

    // Create local AI changes (uncommitted)
    let mut ai_file = local.filename("ai_work.txt");
    ai_file.set_contents(vec!["AI generated line 1".ai(), "AI generated line 2".ai()]);

    local
        .git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Perform fast-forward pull
    local.git(&["pull"]).expect("pull should succeed");

    // Commit and verify AI attribution is preserved through the ff pull
    local
        .stage_all_and_commit("commit after pull")
        .expect("commit should succeed");

    ai_file.assert_lines_and_blame(vec!["AI generated line 1".ai(), "AI generated line 2".ai()]);
}

#[test]
fn test_pull_rebase_autostash_preserves_ai_attribution() {
    let setup = setup_pull_test();
    let local = setup.local;

    // Create local AI changes (uncommitted)
    let mut ai_file = local.filename("ai_work.txt");
    ai_file.set_contents(vec![
        "AI generated line 1".ai(),
        "AI generated line 2".ai(),
        "AI generated line 3".ai(),
    ]);

    local
        .git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Perform pull with --rebase --autostash flags
    local
        .git(&["pull", "--rebase", "--autostash"])
        .expect("pull --rebase --autostash should succeed");

    // Commit and verify AI attribution is preserved through stash/unstash cycle
    local
        .stage_all_and_commit("commit after rebase pull")
        .expect("commit should succeed");

    ai_file.assert_lines_and_blame(vec![
        "AI generated line 1".ai(),
        "AI generated line 2".ai(),
        "AI generated line 3".ai(),
    ]);
}

#[test]
fn test_pull_rebase_autostash_with_mixed_attribution() {
    let setup = setup_pull_test();
    let local = setup.local;

    // Create local changes with mixed human and AI attribution
    let mut mixed_file = local.filename("mixed_work.txt");
    mixed_file.set_contents(vec![
        "Human written line 1".human(),
        "AI generated line 1".ai(),
        "Human written line 2".human(),
        "AI generated line 2".ai(),
    ]);

    local
        .git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Perform pull with --rebase --autostash
    local
        .git(&["pull", "--rebase", "--autostash"])
        .expect("pull --rebase --autostash should succeed");

    // Commit and verify mixed attribution is preserved
    local
        .stage_all_and_commit("commit with mixed attribution")
        .expect("commit should succeed");

    mixed_file.assert_lines_and_blame(vec![
        "Human written line 1".human(),
        "AI generated line 1".ai(),
        "Human written line 2".human(),
        "AI generated line 2".ai(),
    ]);
}

#[test]
fn test_pull_rebase_autostash_via_git_config() {
    let setup = setup_pull_test();
    let local = setup.local;

    // Set git config to always use rebase and autostash for pull
    local
        .git(&["config", "pull.rebase", "true"])
        .expect("set pull.rebase should succeed");
    local
        .git(&["config", "rebase.autoStash", "true"])
        .expect("set rebase.autoStash should succeed");

    // Create local AI changes (uncommitted)
    let mut ai_file = local.filename("ai_config_test.txt");
    ai_file.set_contents(vec!["AI line via config".ai()]);

    local
        .git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Perform regular pull (should use rebase+autostash from config)
    local.git(&["pull"]).expect("pull should succeed");

    // Commit and verify AI attribution is preserved
    local
        .stage_all_and_commit("commit after config-based rebase pull")
        .expect("commit should succeed");

    ai_file.assert_lines_and_blame(vec!["AI line via config".ai()]);
}

#[test]
fn test_pull_rebase_preserves_committed_ai_authorship() {
    let (local, upstream) = TestRepo::new_with_remote();

    // Make initial commit in local and push
    let mut readme = local.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    let initial = local
        .stage_all_and_commit("initial commit")
        .expect("initial commit should succeed");

    local
        .git(&["push", "-u", "origin", "HEAD"])
        .expect("push initial commit should succeed");

    // Create a committed AI-authored change locally
    let mut ai_file = local.filename("ai_feature.txt");
    ai_file.set_contents(vec![
        "AI generated feature line 1".ai(),
        "AI generated feature line 2".ai(),
    ]);
    local
        .stage_all_and_commit("add AI feature")
        .expect("AI feature commit should succeed");

    // Simulate upstream advancing: create a second clone, make a commit, push
    // We do this by going back to initial, making a divergent commit, pushing from
    // a detached state via the bare upstream directly.
    // Simpler approach: use the local repo to push a commit to a temp branch,
    // then reset local back.

    // First, record the AI commit SHA
    let ai_commit_sha = local
        .git(&["rev-parse", "HEAD"])
        .expect("rev-parse should succeed")
        .trim()
        .to_string();

    // Create an upstream-only commit by:
    // 1. Stash our AI commit
    // 2. Create upstream commit and push
    // 3. Restore our AI commit
    // Actually, easier: reset to initial, create upstream commit, push, then cherry-pick AI back.

    // Save current branch name
    let branch = local.current_branch();

    // Reset to initial to create a divergent upstream commit
    local
        .git(&["reset", "--hard", &initial.commit_sha])
        .expect("reset should succeed");

    let mut upstream_file = local.filename("upstream_change.txt");
    upstream_file.set_contents(vec!["upstream content".to_string()]);
    local
        .stage_all_and_commit("upstream divergent commit")
        .expect("upstream commit should succeed");

    // Force push this as the upstream state
    local
        .git(&["push", "--force", "origin", &format!("HEAD:{}", branch)])
        .expect("force push upstream commit should succeed");

    // Now reset back to the AI commit (local is ahead with AI work, behind on upstream)
    local
        .git(&["reset", "--hard", &ai_commit_sha])
        .expect("reset to AI commit should succeed");

    // Perform pull --rebase (committed local changes will be rebased onto upstream)
    local
        .git(&["pull", "--rebase"])
        .expect("pull --rebase should succeed");

    // Verify we got upstream changes
    assert!(
        local.read_file("upstream_change.txt").is_some(),
        "Should have upstream_change.txt after pull --rebase"
    );

    // The AI commit got new SHA after rebase - verify authorship survived
    let new_head = local
        .git(&["rev-parse", "HEAD"])
        .expect("rev-parse should succeed")
        .trim()
        .to_string();

    assert_ne!(
        new_head, ai_commit_sha,
        "HEAD should have a new SHA after rebase"
    );

    // Verify AI authorship is preserved on the rebased commit
    ai_file.assert_lines_and_blame(vec![
        "AI generated feature line 1".ai(),
        "AI generated feature line 2".ai(),
    ]);
}

#[test]
fn test_pull_rebase_committed_and_autostash_preserves_all_authorship() {
    let (local, _upstream) = TestRepo::new_with_remote();

    // Make initial commit in local and push
    let mut readme = local.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    let initial = local
        .stage_all_and_commit("initial commit")
        .expect("initial commit should succeed");

    local
        .git(&["push", "-u", "origin", "HEAD"])
        .expect("push initial commit should succeed");

    // Create a committed AI-authored change locally
    let mut committed_ai = local.filename("committed_ai.txt");
    committed_ai.set_contents(vec!["Committed AI line".ai()]);
    local
        .stage_all_and_commit("committed AI work")
        .expect("AI commit should succeed");

    let ai_commit_sha = local
        .git(&["rev-parse", "HEAD"])
        .expect("rev-parse should succeed")
        .trim()
        .to_string();

    let branch = local.current_branch();

    // Create divergent upstream commit
    local
        .git(&["reset", "--hard", &initial.commit_sha])
        .expect("reset should succeed");

    let mut upstream_file = local.filename("upstream.txt");
    upstream_file.set_contents(vec!["upstream".to_string()]);
    local
        .stage_all_and_commit("upstream commit")
        .expect("upstream commit should succeed");

    local
        .git(&["push", "--force", "origin", &format!("HEAD:{}", branch)])
        .expect("force push should succeed");

    // Reset back to AI commit
    local
        .git(&["reset", "--hard", &ai_commit_sha])
        .expect("reset should succeed");

    // Also create uncommitted AI changes (will trigger autostash)
    let mut uncommitted_ai = local.filename("uncommitted_ai.txt");
    uncommitted_ai.set_contents(vec!["Uncommitted AI line".ai()]);
    local
        .git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Pull --rebase --autostash: both committed and uncommitted AI changes need preservation
    local
        .git(&["pull", "--rebase", "--autostash"])
        .expect("pull --rebase --autostash should succeed");

    // Commit the previously-uncommitted changes
    local
        .stage_all_and_commit("commit uncommitted AI work")
        .expect("commit should succeed");

    // Verify committed AI authorship survived the rebase
    committed_ai.assert_lines_and_blame(vec!["Committed AI line".ai()]);

    // Verify uncommitted AI authorship survived the autostash cycle
    uncommitted_ai.assert_lines_and_blame(vec!["Uncommitted AI line".ai()]);
}

#[test]
fn test_fast_forward_pull_without_local_changes() {
    let setup = setup_pull_test();
    let local = setup.local;

    // No local changes - just a clean fast-forward pull

    // Perform fast-forward pull
    local.git(&["pull"]).expect("pull should succeed");

    // Verify we got the upstream changes
    assert!(
        local.read_file("upstream_file.txt").is_some(),
        "Should have upstream_file.txt after pull"
    );

    // Verify HEAD is at the expected upstream commit
    let head = local.git(&["rev-parse", "HEAD"]).unwrap();
    assert_eq!(
        head.trim(),
        setup.upstream_sha,
        "HEAD should be at upstream commit"
    );
}
