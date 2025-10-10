use crate::authorship::working_log::Checkpoint;
use crate::error::GitAiError;
use crate::git::rewrite_log::{RewriteLogEvent, append_event_to_file};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

pub struct RepoStorage {
    pub repo_path: PathBuf,
    pub working_logs: PathBuf,
    pub rewrite_log: PathBuf,
}

impl RepoStorage {
    pub fn for_repo_path(repo_path: &Path) -> RepoStorage {
        let ai_dir = repo_path.join("ai");
        let working_logs_dir = ai_dir.join("working_logs");
        let rewrite_log_file = ai_dir.join("rewrite_log");

        let config = RepoStorage {
            repo_path: repo_path.to_path_buf(),
            working_logs: working_logs_dir,
            rewrite_log: rewrite_log_file,
        };

        // @todo - @acunniffe, make this lazy on a read or write.
        // it's probably fine to run this when Repository is loaded but there
        // are many git commands for which it is not needed
        config.ensure_config_directory().unwrap();
        return config;
    }

    fn ensure_config_directory(&self) -> Result<(), GitAiError> {
        let ai_dir = self.repo_path.join("ai");

        fs::create_dir_all(ai_dir)?;

        // Create working_logs directory
        fs::create_dir_all(&self.working_logs)?;

        if !&self.rewrite_log.exists() && !&self.rewrite_log.is_file() {
            fs::write(&self.rewrite_log, "")?;
        }

        Ok(())
    }

    /* Working Log Persistance */

    pub fn working_log_for_base_commit(&self, sha: &str) -> PersistedWorkingLog {
        let working_log_dir = self.working_logs.join(sha);
        fs::create_dir_all(&working_log_dir).unwrap();
        // The repo_path is the .git directory, so we need to go up one level to get the actual repo root
        let repo_root = self.repo_path.parent().unwrap().to_path_buf();
        PersistedWorkingLog::new(working_log_dir, sha, repo_root)
    }

    #[allow(dead_code)]
    pub fn delete_working_log_for_base_commit(&self, sha: &str) -> Result<(), GitAiError> {
        let working_log_dir = self.working_logs.join(sha);
        if working_log_dir.exists() {
            fs::remove_dir_all(&working_log_dir)?;
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn delete_all_working_logs(&self) -> Result<(), GitAiError> {
        if self.working_logs.exists() {
            fs::remove_dir_all(&self.working_logs)?;
            // Recreate the empty directory structure
            fs::create_dir_all(&self.working_logs)?;
        }
        Ok(())
    }

    /* Rewrite Log Persistance */

    /// Append a rewrite event to the rewrite log file and return the full log
    pub fn append_rewrite_event(
        &self,
        event: RewriteLogEvent,
    ) -> Result<Vec<RewriteLogEvent>, GitAiError> {
        append_event_to_file(&self.rewrite_log, event)?;
        self.read_rewrite_events()
    }

    /// Read all rewrite events from the rewrite log file
    pub fn read_rewrite_events(&self) -> Result<Vec<RewriteLogEvent>, GitAiError> {
        if !self.rewrite_log.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&self.rewrite_log)?;
        crate::git::rewrite_log::deserialize_events_from_jsonl(&content)
    }
}

pub struct PersistedWorkingLog {
    pub dir: PathBuf,
    #[allow(dead_code)]
    pub base_commit: String,
    pub repo_root: PathBuf,
}

impl PersistedWorkingLog {
    pub fn new(dir: PathBuf, base_commit: &str, repo_root: PathBuf) -> Self {
        Self {
            dir,
            base_commit: base_commit.to_string(),
            repo_root,
        }
    }

    pub fn reset_working_log(&self) -> Result<(), GitAiError> {
        // Clear all blobs by removing the blobs directory
        let blobs_dir = self.dir.join("blobs");
        if blobs_dir.exists() {
            fs::remove_dir_all(&blobs_dir)?;
        }

        // Clear checkpoints by truncating the JSONL file
        let checkpoints_file = self.dir.join("checkpoints.jsonl");
        fs::write(&checkpoints_file, "")?;

        Ok(())
    }

    /* blob storage */
    pub fn get_file_version(&self, sha: &str) -> Result<String, GitAiError> {
        let blob_path = self.dir.join("blobs").join(sha);
        Ok(fs::read_to_string(blob_path)?)
    }

    pub fn persist_file_version(&self, content: &str) -> Result<String, GitAiError> {
        // Create SHA256 hash of the content
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let sha = format!("{:x}", hasher.finalize());

        // Ensure blobs directory exists
        let blobs_dir = self.dir.join("blobs");
        fs::create_dir_all(&blobs_dir)?;

        // Write content to blob file
        let blob_path = blobs_dir.join(&sha);
        fs::write(blob_path, content)?;

        Ok(sha)
    }

    /* append checkpoint */
    pub fn append_checkpoint(&self, checkpoint: &Checkpoint) -> Result<(), GitAiError> {
        let checkpoints_file = self.dir.join("checkpoints.jsonl");

        // Serialize checkpoint to JSON and append to JSONL file
        let json_line = serde_json::to_string(checkpoint)?;

        // Open file in append mode and write the JSON line
        use std::fs::OpenOptions;
        use std::io::Write;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&checkpoints_file)?;

        writeln!(file, "{}", json_line)?;

        Ok(())
    }

    pub fn read_all_checkpoints(&self) -> Result<InMemoryWorkingLog, GitAiError> {
        let checkpoints_file = self.dir.join("checkpoints.jsonl");

        if !checkpoints_file.exists() {
            return Ok(InMemoryWorkingLog::new(Vec::new()));
        }

        let content = fs::read_to_string(&checkpoints_file)?;
        let mut checkpoints = Vec::new();

        // Parse JSONL file - each line is a separate JSON object
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }

            let checkpoint: Checkpoint = serde_json::from_str(line)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

            checkpoints.push(checkpoint);
        }

        Ok(InMemoryWorkingLog::new(checkpoints))
    }
}

pub struct InMemoryWorkingLog {
    pub checkpoints: Vec<Checkpoint>,
    pub edited_files: HashSet<String>,
}

impl InMemoryWorkingLog {
    pub fn new(checkpoints: Vec<Checkpoint>) -> Self {
        let mut edited_files = HashSet::new();
        for checkpoint in &checkpoints {
            for entry in &checkpoint.entries {
                if !edited_files.contains(&entry.file) {
                    edited_files.insert(entry.file.clone());
                }
            }
        }

        Self {
            checkpoints,
            edited_files,
        }
    }
}

#[cfg(test)]
mod tests {

    use crate::git::test_utils::TmpRepo;

    use super::*;
    use std::fs;

    #[test]
    fn test_ensure_config_directory_creates_structure() {
        // Create a temporary repository
        let tmp_repo = TmpRepo::new().expect("Failed to create tmp repo");

        // Create RepoStorage
        let _repo_storage = RepoStorage::for_repo_path(tmp_repo.repo().path());

        // Verify .git/ai directory exists
        let ai_dir = tmp_repo.repo().path().join("ai");
        assert!(ai_dir.exists(), ".git/ai directory should exist");
        assert!(ai_dir.is_dir(), ".git/ai should be a directory");

        // Verify working_logs directory exists
        let working_logs_dir = ai_dir.join("working_logs");
        assert!(
            working_logs_dir.exists(),
            "working_logs directory should exist"
        );
        assert!(
            working_logs_dir.is_dir(),
            "working_logs should be a directory"
        );

        // Verify rewrite_log file exists and is empty
        let rewrite_log_file = ai_dir.join("rewrite_log");
        assert!(rewrite_log_file.exists(), "rewrite_log file should exist");
        assert!(rewrite_log_file.is_file(), "rewrite_log should be a file");

        let content = fs::read_to_string(&rewrite_log_file).expect("Failed to read rewrite_log");
        assert_eq!(content, "", "rewrite_log should be empty by default");
    }

    #[test]
    fn test_ensure_config_directory_handles_existing_files() {
        // Create a temporary repository
        let tmp_repo = TmpRepo::new().expect("Failed to create tmp repo");

        // Create RepoStorage
        let repo_storage = RepoStorage::for_repo_path(&tmp_repo.repo().path());

        // Add some content to rewrite_log
        let rewrite_log_file = tmp_repo.repo().path().join("ai").join("rewrite_log");
        fs::write(&rewrite_log_file, "existing content").expect("Failed to write to rewrite_log");

        // Second call - should not overwrite existing file
        repo_storage
            .ensure_config_directory()
            .expect("Failed to ensure config directory again");

        // Verify the content is preserved
        let content = fs::read_to_string(&rewrite_log_file).expect("Failed to read rewrite_log");
        assert_eq!(
            content, "existing content",
            "Existing rewrite_log content should be preserved"
        );

        // Verify directories still exist
        let ai_dir = tmp_repo.repo().path().join("ai");
        let working_logs_dir = ai_dir.join("working_logs");
        assert!(ai_dir.exists(), ".git/ai directory should still exist");
        assert!(
            working_logs_dir.exists(),
            "working_logs directory should still exist"
        );
    }

    #[test]
    fn test_persisted_working_log_blob_storage() {
        // Create a temporary repository
        let tmp_repo = TmpRepo::new().expect("Failed to create tmp repo");

        // Create RepoStorage and PersistedWorkingLog
        let repo_storage = RepoStorage::for_repo_path(tmp_repo.repo().path());
        let working_log = repo_storage.working_log_for_base_commit("test-commit-sha");

        // Test persisting a file version
        let content = "Hello, World!\nThis is a test file.";
        let sha = working_log
            .persist_file_version(content)
            .expect("Failed to persist file version");

        // Verify the SHA is not empty
        assert!(!sha.is_empty(), "SHA should not be empty");

        // Test retrieving the file version
        let retrieved_content = working_log
            .get_file_version(&sha)
            .expect("Failed to get file version");

        assert_eq!(
            content, retrieved_content,
            "Retrieved content should match original"
        );

        // Verify the blob file exists
        let blob_path = working_log.dir.join("blobs").join(&sha);
        assert!(blob_path.exists(), "Blob file should exist");
        assert!(blob_path.is_file(), "Blob should be a file");

        // Test persisting the same content again should return the same SHA
        let sha2 = working_log
            .persist_file_version(content)
            .expect("Failed to persist file version again");

        assert_eq!(sha, sha2, "Same content should produce same SHA");
    }

    #[test]
    fn test_persisted_working_log_checkpoint_storage() {
        // Create a temporary repository
        let tmp_repo = TmpRepo::new().expect("Failed to create tmp repo");

        // Create RepoStorage and PersistedWorkingLog
        let repo_storage = RepoStorage::for_repo_path(tmp_repo.repo().path());
        let working_log = repo_storage.working_log_for_base_commit("test-commit-sha");

        // Create a test checkpoint
        let checkpoint = Checkpoint::new(
            "test-diff".to_string(),
            "test-author".to_string(),
            vec![], // empty entries for simplicity
        );

        // Test appending checkpoint
        working_log
            .append_checkpoint(&checkpoint)
            .expect("Failed to append checkpoint");

        // Test reading all checkpoints
        let working_log_data = working_log
            .read_all_checkpoints()
            .expect("Failed to read checkpoints");

        println!("checkpoints: {:?}", working_log_data.checkpoints);

        assert_eq!(
            working_log_data.checkpoints.len(),
            1,
            "Should have one checkpoint"
        );
        assert_eq!(working_log_data.checkpoints[0].author, "test-author");

        // Verify the JSONL file exists
        let checkpoints_file = working_log.dir.join("checkpoints.jsonl");
        assert!(checkpoints_file.exists(), "Checkpoints file should exist");

        // Test appending another checkpoint
        let checkpoint2 = Checkpoint::new(
            "test-diff-2".to_string(),
            "test-author-2".to_string(),
            vec![],
        );

        working_log
            .append_checkpoint(&checkpoint2)
            .expect("Failed to append second checkpoint");

        let working_log_data = working_log
            .read_all_checkpoints()
            .expect("Failed to read checkpoints after second append");

        assert_eq!(
            working_log_data.checkpoints.len(),
            2,
            "Should have two checkpoints"
        );
        assert_eq!(working_log_data.checkpoints[1].author, "test-author-2");
    }

    #[test]
    fn test_persisted_working_log_reset() {
        // Create a temporary repository
        let tmp_repo = TmpRepo::new().expect("Failed to create tmp repo");

        // Create RepoStorage and PersistedWorkingLog
        let repo_storage = RepoStorage::for_repo_path(tmp_repo.repo().path());
        let working_log = repo_storage.working_log_for_base_commit("test-commit-sha");

        // Add some blobs
        let content = "Test content";
        let sha = working_log
            .persist_file_version(content)
            .expect("Failed to persist file version");

        // Add some checkpoints
        let checkpoint =
            Checkpoint::new("test-diff".to_string(), "test-author".to_string(), vec![]);
        working_log
            .append_checkpoint(&checkpoint)
            .expect("Failed to append checkpoint");

        // Verify they exist
        assert!(working_log.dir.join("blobs").join(&sha).exists());
        let working_log_data = working_log
            .read_all_checkpoints()
            .expect("Failed to read checkpoints");
        assert_eq!(working_log_data.checkpoints.len(), 1);

        // Reset the working log
        working_log
            .reset_working_log()
            .expect("Failed to reset working log");

        // Verify blobs are cleared
        assert!(
            !working_log.dir.join("blobs").exists(),
            "Blobs directory should be removed"
        );

        // Verify checkpoints are cleared
        let working_log_data = working_log
            .read_all_checkpoints()
            .expect("Failed to read checkpoints after reset");
        assert_eq!(
            working_log_data.checkpoints.len(),
            0,
            "Should have no checkpoints after reset"
        );

        // Verify checkpoints.jsonl exists but is empty
        let checkpoints_file = working_log.dir.join("checkpoints.jsonl");
        assert!(
            checkpoints_file.exists(),
            "Checkpoints file should still exist"
        );
        let content =
            fs::read_to_string(&checkpoints_file).expect("Failed to read checkpoints file");
        assert!(
            content.trim().is_empty(),
            "Checkpoints file should be empty"
        );
    }

    #[test]
    fn test_working_log_for_base_commit_creates_directory() {
        // Create a temporary repository
        let tmp_repo = TmpRepo::new().expect("Failed to create tmp repo");

        // Create RepoStorage
        let repo_storage = RepoStorage::for_repo_path(tmp_repo.repo().path());

        // Create working log for a specific commit
        let commit_sha = "abc123def456";
        let working_log = repo_storage.working_log_for_base_commit(commit_sha);

        // Verify the directory was created
        assert!(
            working_log.dir.exists(),
            "Working log directory should exist"
        );
        assert!(
            working_log.dir.is_dir(),
            "Working log should be a directory"
        );

        // Verify it's in the correct location
        let expected_path = tmp_repo
            .repo()
            .path()
            .join("ai")
            .join("working_logs")
            .join(commit_sha);
        assert_eq!(
            working_log.dir, expected_path,
            "Working log directory should be in correct location"
        );
    }
}
