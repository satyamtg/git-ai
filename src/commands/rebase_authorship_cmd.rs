use crate::authorship::rebase_authorship::rewrite_authorship_after_rebase_v2;
use crate::git::find_repository_in_path;

pub fn handle_rebase_authorship(args: &[String]) {
    // Parse arguments
    let mut original_head = None;
    let mut original_commits = Vec::new();
    let mut new_commits = Vec::new();
    let mut dry_run = false;

    let mut i = 0;
    let mut parsing_original = false;
    let mut parsing_new = false;

    while i < args.len() {
        match args[i].as_str() {
            "--original-commits" => {
                parsing_original = true;
                parsing_new = false;
                i += 1;
            }
            "--new-commits" => {
                parsing_new = true;
                parsing_original = false;
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
                } else if parsing_original {
                    original_commits.push(arg.to_string());
                } else if parsing_new {
                    new_commits.push(arg.to_string());
                } else if original_head.is_none() {
                    original_head = Some(arg.to_string());
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
    let original_head = match original_head {
        Some(s) => s,
        None => {
            eprintln!("Error: original_head argument is required");
            print_usage();
            std::process::exit(1);
        }
    };

    if original_commits.is_empty() {
        eprintln!("Error: --original-commits requires at least one commit SHA");
        print_usage();
        std::process::exit(1);
    }

    if new_commits.is_empty() {
        eprintln!("Error: --new-commits requires at least one commit SHA");
        print_usage();
        std::process::exit(1);
    }

    if dry_run {
        println!("DRY RUN: Would rewrite authorship for rebase:");
        println!("  Original HEAD: {}", original_head);
        println!("  Original commits ({}):", original_commits.len());
        for (i, sha) in original_commits.iter().enumerate() {
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
    if let Err(e) = rewrite_authorship_after_rebase_v2(
        &repo,
        &original_head,
        &original_commits,
        &new_commits,
        &default_user_name,
    ) {
        eprintln!("Rebase authorship failed: {}", e);
        std::process::exit(1);
    }

    println!("✓ Successfully rewrote authorship for {} new commits", new_commits.len());
}

fn print_usage() {
    eprintln!("Usage: git-ai rebase-authorship <original_head> --original-commits <sha1> [<sha2> ...] --new-commits <sha1> [<sha2> ...] [--dry-run]");
    eprintln!();
    eprintln!("Arguments:");
    eprintln!("  <original_head>           SHA of HEAD before rebase");
    eprintln!("  --original-commits        List of original commit SHAs (oldest first)");
    eprintln!("  --new-commits             List of new commit SHAs after rebase (oldest first)");
    eprintln!("  --dry-run                 Show what would be done without making changes");
    eprintln!();
    eprintln!("Example:");
    eprintln!("  git-ai rebase-authorship abc123 \\");
    eprintln!("    --original-commits def456 ghi789 jkl012 \\");
    eprintln!("    --new-commits mno345 pqr678 stu901");
}