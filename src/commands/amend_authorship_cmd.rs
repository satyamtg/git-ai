use crate::authorship::rebase_authorship::rewrite_authorship_after_commit_amend;
use crate::git::find_repository_in_path;

pub fn handle_amend_authorship(args: &[String]) {
    // Parse arguments
    let mut old_sha = None;
    let mut new_sha = None;
    let mut dry_run = false;

    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--dry-run" => {
                dry_run = true;
                i += 1;
            }
            arg => {
                if arg.starts_with("--") {
                    eprintln!("Unknown flag: {}", arg);
                    print_usage();
                    std::process::exit(1);
                } else if old_sha.is_none() {
                    old_sha = Some(arg.to_string());
                } else if new_sha.is_none() {
                    new_sha = Some(arg.to_string());
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
    let old_sha = match old_sha {
        Some(s) => s,
        None => {
            eprintln!("Error: old_sha argument is required");
            print_usage();
            std::process::exit(1);
        }
    };

    let new_sha = match new_sha {
        Some(s) => s,
        None => {
            eprintln!("Error: new_sha argument is required");
            print_usage();
            std::process::exit(1);
        }
    };

    if dry_run {
        println!("DRY RUN: Would rewrite authorship for commit amend:");
        println!("  Old commit: {}", old_sha);
        println!("  New commit: {}", new_sha);
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
    match rewrite_authorship_after_commit_amend(
        &repo,
        &old_sha,
        &new_sha,
        default_user_name,
    ) {
        Ok(_authorship_log) => {
            println!("✓ Successfully rewrote authorship for amended commit {}", new_sha);
        }
        Err(e) => {
            eprintln!("Amend authorship failed: {}", e);
            std::process::exit(1);
        }
    }
}

fn print_usage() {
    eprintln!("Usage: git-ai amend-authorship <old_sha> <new_sha> [--dry-run]");
    eprintln!();
    eprintln!("Arguments:");
    eprintln!("  <old_sha>     SHA of the original commit before amendment");
    eprintln!("  <new_sha>     SHA of the new commit created by amend");
    eprintln!("  --dry-run     Show what would be done without making changes");
    eprintln!();
    eprintln!("Example:");
    eprintln!("  git-ai amend-authorship abc123 def456");
}