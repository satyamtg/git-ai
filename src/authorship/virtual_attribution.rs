use crate::authorship::attribution_tracker::{
    Attribution, LineAttribution, line_attributions_to_attributions,
};
use crate::authorship::working_log::CheckpointKind;
use crate::commands::blame::GitAiBlameOptions;
use crate::error::GitAiError;
use crate::git::repository::Repository;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct VirtualAttributions {
    repo: Repository,
    base_commit: String,
    // Maps file path -> (char attributions, line attributions)
    attributions: HashMap<String, (Vec<Attribution>, Vec<LineAttribution>)>,
    // Timestamp to use for attributions
    ts: u128,
}

impl VirtualAttributions {
    /// Create a new VirtualAttributions for the given base commit with initial pathspecs
    pub async fn new_for_base_commit(
        repo: Repository,
        base_commit: String,
        pathspecs: &[String],
    ) -> Result<Self, GitAiError> {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();

        let mut virtual_attrs = VirtualAttributions {
            repo,
            base_commit,
            attributions: HashMap::new(),
            ts,
        };

        // Process all pathspecs concurrently
        if !pathspecs.is_empty() {
            virtual_attrs.add_pathspecs_concurrent(pathspecs).await?;
        }

        Ok(virtual_attrs)
    }

    /// Add a single pathspec to the virtual attributions
    pub async fn add_pathspec(&mut self, pathspec: &str) -> Result<(), GitAiError> {
        self.add_pathspecs_concurrent(&[pathspec.to_string()]).await
    }

    /// Add multiple pathspecs concurrently
    async fn add_pathspecs_concurrent(&mut self, pathspecs: &[String]) -> Result<(), GitAiError> {
        const MAX_CONCURRENT: usize = 30;

        let semaphore = Arc::new(smol::lock::Semaphore::new(MAX_CONCURRENT));
        let mut tasks = Vec::new();

        for pathspec in pathspecs {
            let pathspec = pathspec.clone();
            let repo = self.repo.clone();
            let base_commit = self.base_commit.clone();
            let ts = self.ts;
            let semaphore = Arc::clone(&semaphore);

            let task = smol::spawn(async move {
                // Acquire semaphore permit to limit concurrency
                let _permit = semaphore.acquire().await;

                // Wrap blocking git operations in smol::unblock
                smol::unblock(move || {
                    compute_attributions_for_file(&repo, &base_commit, &pathspec, ts)
                })
                .await
            });

            tasks.push(task);
        }

        // Await all tasks
        let results = futures::future::join_all(tasks).await;

        // Process results and store in HashMap
        for result in results {
            match result {
                Ok(Some((file_path, char_attrs, line_attrs))) => {
                    self.attributions
                        .insert(file_path, (char_attrs, line_attrs));
                }
                Ok(None) => {
                    // File had no changes or couldn't be processed, skip
                }
                Err(e) => return Err(e),
            }
        }

        Ok(())
    }

    /// Get both character and line attributions for a file
    pub fn get_attributions(
        &self,
        file_path: &str,
    ) -> Option<&(Vec<Attribution>, Vec<LineAttribution>)> {
        self.attributions.get(file_path)
    }

    /// Get just character-level attributions for a file
    pub fn get_char_attributions(&self, file_path: &str) -> Option<&Vec<Attribution>> {
        self.attributions
            .get(file_path)
            .map(|(char_attrs, _)| char_attrs)
    }

    /// Get just line-level attributions for a file
    pub fn get_line_attributions(&self, file_path: &str) -> Option<&Vec<LineAttribution>> {
        self.attributions
            .get(file_path)
            .map(|(_, line_attrs)| line_attrs)
    }

    /// List all tracked files
    pub fn files(&self) -> Vec<String> {
        self.attributions.keys().cloned().collect()
    }

    /// Get the base commit SHA
    pub fn base_commit(&self) -> &str {
        &self.base_commit
    }

    /// Get the timestamp used for attributions
    pub fn timestamp(&self) -> u128 {
        self.ts
    }
}

/// Compute attributions for a single file at a specific commit
fn compute_attributions_for_file(
    repo: &Repository,
    base_commit: &str,
    file_path: &str,
    ts: u128,
) -> Result<Option<(String, Vec<Attribution>, Vec<LineAttribution>)>, GitAiError> {
    // Set up blame options
    let mut ai_blame_opts = GitAiBlameOptions::default();
    ai_blame_opts.no_output = true;
    ai_blame_opts.return_human_authors_as_human = true;
    ai_blame_opts.use_prompt_hashes_as_names = true;
    ai_blame_opts.newest_commit = Some(base_commit.to_string());

    // Run blame at the base commit
    let ai_blame = repo.blame(file_path, &ai_blame_opts);

    match ai_blame {
        Ok((blames, _)) => {
            // Convert blame results to line attributions
            let mut line_attributions = Vec::new();
            for (line, author) in blames {
                // Skip human-only lines as they don't need tracking
                if author == CheckpointKind::Human.to_str() {
                    continue;
                }
                line_attributions.push(LineAttribution {
                    start_line: line,
                    end_line: line,
                    author_id: author.clone(),
                    overridden: false,
                });
            }

            // Get the file content at this commit to convert to character attributions
            // We need to read the file content that blame operated on
            let file_content = get_file_content_at_commit(repo, base_commit, file_path)?;

            // Convert line attributions to character attributions
            let char_attributions =
                line_attributions_to_attributions(&line_attributions, &file_content, ts);

            Ok(Some((
                file_path.to_string(),
                char_attributions,
                line_attributions,
            )))
        }
        Err(_) => {
            // File doesn't exist at this commit or can't be blamed, skip it
            Ok(None)
        }
    }
}

/// Get file content at a specific commit
fn get_file_content_at_commit(
    repo: &Repository,
    commit_sha: &str,
    file_path: &str,
) -> Result<String, GitAiError> {
    let commit = repo.find_commit(commit_sha.to_string())?;
    let tree = commit.tree()?;

    match tree.get_path(std::path::Path::new(file_path)) {
        Ok(entry) => {
            if let Ok(blob) = repo.find_blob(entry.id()) {
                let blob_content = blob.content().unwrap_or_default();
                Ok(String::from_utf8_lossy(&blob_content).to_string())
            } else {
                Ok(String::new())
            }
        }
        Err(_) => Ok(String::new()),
    }
}

mod tests {

    use crate::git::find_repository_in_path;

    use super::*;

    #[test]
    fn test_virtual_attributions() {
        let repo = find_repository_in_path(".").unwrap();

        let virtual_attributions = smol::block_on(async {
            VirtualAttributions::new_for_base_commit(
                repo,
                "5753483e6a8d0024dacfc6eaab8b8f5b2f2301c5".to_string(),
                &["src/utils.rs".to_string()],
            )
            .await
        })
        .unwrap();

        println!(
            "virtual_attributions files: {:?}",
            virtual_attributions.files()
        );
        println!("base_commit: {}", virtual_attributions.base_commit());
        println!("timestamp: {}", virtual_attributions.timestamp());

        if let Some((char_attrs, line_attrs)) =
            virtual_attributions.get_attributions("src/utils.rs")
        {
            println!("\n=== src/utils.rs Attribution Info ===");
            println!("Character-level attributions: {} ranges", char_attrs.len());
            for (i, attr) in char_attrs.iter().enumerate() {
                println!(
                    "  [{}] chars {}..{} (len={}) -> author: '{}', ts: {}",
                    i,
                    attr.start,
                    attr.end,
                    attr.end - attr.start,
                    attr.author_id,
                    attr.ts
                );
            }

            println!("\nLine-level attributions: {} ranges", line_attrs.len());
            for (i, attr) in line_attrs.iter().enumerate() {
                println!(
                    "  [{}] lines {}..{} (count={}) -> author: '{}', overridden: {}",
                    i,
                    attr.start_line,
                    attr.end_line,
                    attr.line_count(),
                    attr.author_id,
                    attr.overridden
                );
            }
        }

        assert!(!virtual_attributions.files().is_empty());
    }
}
