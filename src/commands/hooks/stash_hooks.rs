use crate::authorship::virtual_attribution::VirtualAttributions;
use crate::commands::git_handlers::CommandHooksContext;
use crate::commands::hooks::commit_hooks::get_commit_default_author;
use crate::error::GitAiError;
use crate::git::cli_parser::ParsedGitInvocation;
use crate::git::repository::{Repository, exec_git};
use crate::utils::debug_log;

pub fn pre_stash_hook(
    parsed_args: &ParsedGitInvocation,
    repository: &mut Repository,
    command_hooks_context: &mut CommandHooksContext,
) {
    // Check if this is a pop or apply command - we need to capture the stash SHA before Git deletes it
    let subcommand = match parsed_args.pos_command(0) {
        Some(cmd) => cmd,
        None => return, // Implicit push, nothing to capture
    };

    if subcommand == "pop" || subcommand == "apply" {
        // Capture the stash SHA BEFORE git runs (pop will delete it)
        let stash_ref = parsed_args
            .pos_command(1)
            .unwrap_or_else(|| "stash@{0}".to_string());

        if let Ok(stash_sha) = resolve_stash_to_sha(repository, &stash_ref) {
            command_hooks_context.stash_sha = Some(stash_sha);
            debug_log(&format!("Pre-stash: captured stash SHA for {}", subcommand));
        }
    }
}

pub fn post_stash_hook(
    command_hooks_context: &CommandHooksContext,
    parsed_args: &ParsedGitInvocation,
    repository: &mut Repository,
    exit_status: std::process::ExitStatus,
) {
    if !exit_status.success() {
        debug_log("Stash failed, skipping post-stash hook");
        return;
    }

    // Check what subcommand was used
    let subcommand = match parsed_args.pos_command(0) {
        Some(cmd) => cmd,
        None => {
            // No subcommand means implicit "push"
            "push".to_string()
        }
    };

    debug_log(&format!("Post-stash: processing stash {}", subcommand));

    // Handle different subcommands
    if subcommand == "push" || subcommand == "save" {
        // Stash was created - save authorship log as git note
        if let Err(e) = save_stash_authorship_log(repository) {
            debug_log(&format!("Failed to save stash authorship log: {}", e));
        }
    } else if subcommand == "pop" || subcommand == "apply" {
        // Stash was applied - restore attributions from git note
        // Use the stash SHA we captured in pre-hook (before Git deleted it)
        let stash_sha = match &command_hooks_context.stash_sha {
            Some(sha) => sha.clone(),
            None => {
                debug_log("No stash SHA captured in pre-hook, cannot restore attributions");
                return;
            }
        };

        debug_log(&format!(
            "Restoring attributions from stash SHA: {}",
            stash_sha
        ));

        let human_author = get_commit_default_author(&repository, &parsed_args.command_args);

        if let Err(e) = restore_stash_attributions(repository, &stash_sha, &human_author) {
            debug_log(&format!("Failed to restore stash attributions: {}", e));
        }
    }
}

/// Save the current working log as an authorship log in git notes (refs/notes/ai-stash)
fn save_stash_authorship_log(repo: &Repository) -> Result<(), GitAiError> {
    let head_sha = repo.head()?.target()?.to_string();

    // Get the stash SHA that was just created (stash@{0})
    let stash_sha = resolve_stash_to_sha(repo, "stash@{0}")?;
    debug_log(&format!("Stash created with SHA: {}", stash_sha));

    // Build VirtualAttributions from the working log before it was cleared
    let working_log_va =
        VirtualAttributions::from_just_working_log(repo.clone(), head_sha.clone(), None)?;

    // If there are no attributions, just clean up working log
    if working_log_va.files().is_empty() {
        debug_log("No attributions to save for stash");
        repo.storage.delete_working_log_for_base_commit(&head_sha)?;
        return Ok(());
    }

    // Convert to authorship log
    let authorship_log = working_log_va.to_authorship_log()?;

    // Save as git note at refs/notes/ai-stash
    let json = authorship_log
        .serialize_to_string()
        .map_err(|e| GitAiError::Generic(format!("Failed to serialize authorship log: {}", e)))?;
    save_stash_note(repo, &stash_sha, &json)?;

    debug_log(&format!(
        "Saved authorship log to refs/notes/ai-stash for stash {}",
        stash_sha
    ));

    // Delete the working log for HEAD (changes are now in the stash)
    repo.storage.delete_working_log_for_base_commit(&head_sha)?;
    debug_log(&format!("Deleted working log for HEAD {}", head_sha));

    Ok(())
}

/// Restore attributions from a stash by reading the git note and converting to INITIAL attributions
fn restore_stash_attributions(
    repo: &Repository,
    stash_sha: &str,
    _human_author: &str,
) -> Result<(), GitAiError> {
    debug_log(&format!(
        "Restoring stash attributions from SHA: {}",
        stash_sha
    ));

    let head_sha = repo.head()?.target()?.to_string();

    // Try to read authorship log from git note (refs/notes/ai-stash)
    let note_content = match read_stash_note(repo, &stash_sha) {
        Ok(content) => content,
        Err(_) => {
            debug_log("No authorship log found in refs/notes/ai-stash for this stash");
            return Ok(());
        }
    };

    // Parse the authorship log
    let authorship_log = match crate::authorship::authorship_log_serialization::AuthorshipLog::deserialize_from_string(&note_content) {
        Ok(log) => log,
        Err(e) => {
            debug_log(&format!("Failed to parse stash authorship log: {}", e));
            return Ok(());
        }
    };

    debug_log(&format!(
        "Loaded authorship log from stash: {} files, {} prompts",
        authorship_log.attestations.len(),
        authorship_log.metadata.prompts.len()
    ));

    // Convert authorship log to INITIAL attributions
    let mut initial_files = std::collections::HashMap::new();
    for attestation in &authorship_log.attestations {
        let mut line_attrs = Vec::new();
        for entry in &attestation.entries {
            for range in &entry.line_ranges {
                let (start, end) = match range {
                    crate::authorship::authorship_log::LineRange::Single(line) => (*line, *line),
                    crate::authorship::authorship_log::LineRange::Range(start, end) => {
                        (*start, *end)
                    }
                };
                line_attrs.push(crate::authorship::attribution_tracker::LineAttribution {
                    start_line: start,
                    end_line: end,
                    author_id: entry.hash.clone(),
                    overrode: None,
                });
            }
        }
        if !line_attrs.is_empty() {
            initial_files.insert(attestation.file_path.clone(), line_attrs);
        }
    }

    let initial_prompts: std::collections::HashMap<_, _> = authorship_log
        .metadata
        .prompts
        .clone()
        .into_iter()
        .collect();

    // Write INITIAL attributions to working log
    if !initial_files.is_empty() || !initial_prompts.is_empty() {
        let working_log = repo.storage.working_log_for_base_commit(&head_sha);
        working_log.write_initial_attributions(initial_files.clone(), initial_prompts.clone())?;

        let _ = std::fs::write(
            "/tmp/stash_write_success.txt",
            format!(
                "Wrote initial attributions successfully\nFiles: {:?}\nPrompts: {:?}\n",
                initial_files.keys().collect::<Vec<_>>(),
                initial_prompts.keys().collect::<Vec<_>>()
            ),
        );

        debug_log(&format!(
            "✓ Wrote INITIAL attributions to working log for {}",
            head_sha
        ));
    }

    Ok(())
}

/// Save a note to refs/notes/ai-stash
fn save_stash_note(repo: &Repository, stash_sha: &str, content: &str) -> Result<(), GitAiError> {
    let mut args = repo.global_args_for_exec();
    args.push("notes".to_string());
    args.push("--ref=ai-stash".to_string());
    args.push("add".to_string());
    args.push("-f".to_string()); // Force overwrite if exists
    args.push("-m".to_string());
    args.push(content.to_string());
    args.push(stash_sha.to_string());

    let output = exec_git(&args)?;

    if !output.status.success() {
        return Err(GitAiError::Generic(format!(
            "Failed to save stash note: git notes exited with status {}",
            output.status
        )));
    }

    Ok(())
}

/// Read a note from refs/notes/ai-stash
fn read_stash_note(repo: &Repository, stash_sha: &str) -> Result<String, GitAiError> {
    let mut args = repo.global_args_for_exec();
    args.push("notes".to_string());
    args.push("--ref=ai-stash".to_string());
    args.push("show".to_string());
    args.push(stash_sha.to_string());

    let output = exec_git(&args)?;

    if !output.status.success() {
        return Err(GitAiError::Generic(format!(
            "Failed to read stash note: git notes exited with status {}",
            output.status
        )));
    }

    let content = std::str::from_utf8(&output.stdout)?;
    Ok(content.to_string())
}

/// Resolve a stash reference to its commit SHA
fn resolve_stash_to_sha(repo: &Repository, stash_ref: &str) -> Result<String, GitAiError> {
    let mut args = repo.global_args_for_exec();
    args.push("rev-parse".to_string());
    args.push(stash_ref.to_string());

    let output = exec_git(&args)?;

    if !output.status.success() {
        return Err(GitAiError::Generic(format!(
            "Failed to resolve stash reference '{}': git rev-parse exited with status {}",
            stash_ref, output.status
        )));
    }

    let stdout = std::str::from_utf8(&output.stdout)?;
    let sha = stdout.trim().to_string();

    Ok(sha)
}
