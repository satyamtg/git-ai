use crate::authorship::rebase_authorship::rewrite_authorship_after_cherry_pick;
use crate::git::find_repository_in_path;

pub fn handle_cherry_pick_authorship(args: &[String]) {
    // Parse arguments
    let mut source_commits = Vec::new();
    let mut new_commits = Vec::new();
    let mut dry_run = false;

    let mut i = 0;
    let mut parsing_source = false;
    let mut parsing_new = false;

    while i < args.len() {
        match args[i].as_str() {
            "--source-commits" => {
                parsing_source = true;
                parsing_new = false;
                i += 1;
            }
            "--new-commits" => {
                parsing_new = true;
                parsing_source = false;
                i += 1;
            }
            "--dry-run" => {
                dry_run = true;
                i += 1;
            }
            arg => {
                if arg.starts_with("--") {
                    eprintln!("Unknown flag: {}", arg);
                    print_usage();
                    std::process::exit(1);
                } else if parsing_source {
                    source_commits.push(arg.to_string());
                } else if parsing_new {
                    new_commits.push(arg.to_string());
                } else {
                    eprintln!("Unexpected argument: {}", arg);
                    print_usage();
                    std::process::exit(1);
                }
                i += 1;
            }
        }
    }

    // Validate required arguments
    if source_commits.is_empty() {
        eprintln!("Error: --source-commits requires at least one commit SHA");
        print_usage();
        std::process::exit(1);
    }

    if new_commits.is_empty() {
        eprintln!("Error: --new-commits requires at least one commit SHA");
        print_usage();
        std::process::exit(1);
    }

    if dry_run {
        println!("DRY RUN: Would rewrite authorship for cherry-pick:");
        println!("  Source commits ({}):", source_commits.len());
        for (i, sha) in source_commits.iter().enumerate() {
            println!("    {}: {}", i + 1, sha);
        }
        println!("  New commits ({}):", new_commits.len());
        for (i, sha) in new_commits.iter().enumerate() {
            println!("    {}: {}", i + 1, sha);
        }
        return;
    }

    // Find the git repository
    let repo = match find_repository_in_path(".") {
        Ok(repo) => repo,
        Err(e) => {
            eprintln!("Failed to find repository: {}", e);
            std::process::exit(1);
        }
    };

    // Get default author
    let default_user_name = match repo.config_get_str("user.name") {
        Ok(Some(name)) if !name.trim().is_empty() => name,
        _ => {
            eprintln!("Warning: git user.name not configured. Using 'unknown' as author.");
            "unknown".to_string()
        }
    };

    // Call the rewrite function
    if let Err(e) = rewrite_authorship_after_cherry_pick(
        &repo,
        &source_commits,
        &new_commits,
        &default_user_name,
    ) {
        eprintln!("Cherry-pick authorship failed: {}", e);
        std::process::exit(1);
    }

    println!("✓ Successfully rewrote authorship for {} cherry-picked commit(s)", new_commits.len());
}

fn print_usage() {
    eprintln!("Usage: git-ai cherry-pick-authorship --source-commits <sha1> [<sha2> ...] --new-commits <sha1> [<sha2> ...] [--dry-run]");
    eprintln!();
    eprintln!("Arguments:");
    eprintln!("  --source-commits      List of original commit SHAs that were cherry-picked (oldest first)");
    eprintln!("  --new-commits         List of new commit SHAs created by cherry-pick (oldest first)");
    eprintln!("  --dry-run             Show what would be done without making changes");
    eprintln!();
    eprintln!("Example:");
    eprintln!("  git-ai cherry-pick-authorship \\");
    eprintln!("    --source-commits abc123 def456 \\");
    eprintln!("    --new-commits ghi789 jkl012");
}