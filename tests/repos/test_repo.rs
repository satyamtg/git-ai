use git_ai::authorship::authorship_log_serialization::AuthorshipLog;
use git_ai::authorship::stats::CommitStats;
use git_ai::git::repo_storage::PersistedWorkingLog;
use git_ai::git::repository as GitAiRepository;
use git_ai::observability::wrapper_performance_targets::BenchmarkResult;
use git2::Repository;
use insta::assert_debug_snapshot;
use rand::Rng;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use super::test_file::TestFile;

#[derive(Clone, Debug)]
pub struct TestRepo {
    path: PathBuf,
}

impl TestRepo {
    pub fn new() -> Self {
        let mut rng = rand::thread_rng();
        let n: u64 = rng.gen_range(0..10000000000);
        let base = std::env::temp_dir();
        let path = base.join(n.to_string());
        let repo = Repository::init(&path).expect("failed to initialize git2 repository");
        let mut config = Repository::config(&repo).expect("failed to initialize git2 repository");
        config
            .set_str("user.name", "Test User")
            .expect("failed to initialize git2 repository");
        config
            .set_str("user.email", "test@example.com")
            .expect("failed to initialize git2 repository");

        Self { path }
    }

    pub fn new_at_path(path: &PathBuf) -> Self {
        let repo = Repository::init(path).expect("failed to initialize git2 repository");
        let mut config = Repository::config(&repo).expect("failed to initialize git2 repository");
        config
            .set_str("user.name", "Test User")
            .expect("failed to initialize git2 repository");
        config
            .set_str("user.email", "test@example.com")
            .expect("failed to initialize git2 repository");
        Self { path: path.clone() }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn canonical_path(&self) -> PathBuf {
        self.path
            .canonicalize()
            .expect("failed to canonicalize test repo path")
    }

    pub fn stats(&self) -> Result<CommitStats, String> {
        let mut stats = self.git_ai(&["stats", "--json"]).unwrap();
        stats = stats.split("}}}").next().unwrap().to_string() + "}}}";
        let stats: CommitStats = serde_json::from_str(&stats).unwrap();
        Ok(stats)
    }

    pub fn current_branch(&self) -> String {
        self.git(&["branch", "--show-current"])
            .unwrap()
            .trim()
            .to_string()
    }

    pub fn git_ai(&self, args: &[&str]) -> Result<String, String> {
        return self.git_ai_with_env(args, &[]);
    }

    pub fn git(&self, args: &[&str]) -> Result<String, String> {
        return self.git_with_env(args, &[]);
    }

    pub fn benchmark_git(&self, args: &[&str]) -> Result<BenchmarkResult, String> {
        let output = self.git_with_env(args, &[("GIT_AI_DEBUG_PERFORMANCE", "2")])?;

        println!("output: {}", output);
        // Find the JSON performance line
        for line in output.lines() {
            if line.contains("[git-ai (perf-json)]") {
                // Extract the JSON part after the colored prefix
                if let Some(json_start) = line.find('{') {
                    let json_str = &line[json_start..];
                    let parsed: serde_json::Value = serde_json::from_str(json_str)
                        .map_err(|e| format!("Failed to parse performance JSON: {}", e))?;

                    return Ok(BenchmarkResult {
                        total_duration: Duration::from_millis(
                            parsed["total_duration_ms"].as_u64().unwrap_or(0),
                        ),
                        git_duration: Duration::from_millis(
                            parsed["git_duration_ms"].as_u64().unwrap_or(0),
                        ),
                        pre_command_duration: Duration::from_millis(
                            parsed["pre_command_duration_ms"].as_u64().unwrap_or(0),
                        ),
                        post_command_duration: Duration::from_millis(
                            parsed["post_command_duration_ms"].as_u64().unwrap_or(0),
                        ),
                    });
                }
            }
        }

        Err("No performance data found in output".to_string())
    }

    pub fn git_with_env(&self, args: &[&str], envs: &[(&str, &str)]) -> Result<String, String> {
        let binary_path = get_binary_path();

        let mut full_args = vec!["-C", self.path.to_str().unwrap()];
        full_args.extend(args);

        let mut command = Command::new(binary_path);
        command.args(&full_args).env("GIT_AI", "git");

        // Add custom environment variables
        for (key, value) in envs {
            command.env(key, value);
        }

        let output = command.output().expect(&format!(
            "Failed to execute git command with env: {:?}",
            args
        ));

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            // Combine stdout and stderr since git often writes to stderr
            let combined = if stdout.is_empty() {
                stderr
            } else if stderr.is_empty() {
                stdout
            } else {
                format!("{}{}", stdout, stderr)
            };
            Ok(combined)
        } else {
            Err(stderr)
        }
    }

    pub fn git_ai_with_env(&self, args: &[&str], envs: &[(&str, &str)]) -> Result<String, String> {
        let binary_path = get_binary_path();

        let mut command = Command::new(binary_path);
        command.args(args).current_dir(&self.path);

        // Add custom environment variables
        for (key, value) in envs {
            command.env(key, value);
        }

        let output = command
            .output()
            .expect(&format!("Failed to execute git-ai command: {:?}", args));

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            // Combine stdout and stderr since git-ai often writes to stderr
            let combined = if stdout.is_empty() {
                stderr
            } else if stderr.is_empty() {
                stdout
            } else {
                format!("{}{}", stdout, stderr)
            };
            Ok(combined)
        } else {
            Err(stderr)
        }
    }

    pub fn filename(&self, filename: &str) -> TestFile {
        let file_path = self.path.join(filename);

        // If file exists, populate from existing file with blame
        if file_path.exists() {
            TestFile::from_existing_file(file_path, self)
        } else {
            // New file, start with empty lines
            TestFile::new_with_filename(file_path, vec![], self)
        }
    }

    pub fn current_working_logs(&self) -> PersistedWorkingLog {
        let repo = GitAiRepository::find_repository_in_path(self.path.to_str().unwrap())
            .expect("Failed to find repository");

        // Get the current HEAD commit SHA, or use "initial" for empty repos
        let commit_sha = repo
            .head()
            .ok()
            .and_then(|head| head.target().ok())
            .unwrap_or_else(|| "initial".to_string());

        // Get the working log for the current HEAD commit
        repo.storage.working_log_for_base_commit(&commit_sha)
    }

    pub fn commit(&self, message: &str) -> Result<NewCommit, String> {
        return self.commit_with_env(message, &[]);
    }

    pub fn stage_all_and_commit(&self, message: &str) -> Result<NewCommit, String> {
        self.git(&["add", "-A"]).expect("add --all should succeed");
        self.commit(message)
    }

    pub fn commit_with_env(
        &self,
        message: &str,
        envs: &[(&str, &str)],
    ) -> Result<NewCommit, String> {
        let output = self.git_with_env(&["commit", "-m", message], envs);

        // println!("commit output: {:?}", output);
        if output.is_ok() {
            let combined = output.unwrap();

            // Get the repository and HEAD commit SHA
            let repo = GitAiRepository::find_repository_in_path(self.path.to_str().unwrap())
                .map_err(|e| format!("Failed to find repository: {}", e))?;

            let head_commit = repo
                .head()
                .map_err(|e| format!("Failed to get HEAD: {}", e))?
                .target()
                .map_err(|e| format!("Failed to get HEAD target: {}", e))?;

            // Get the authorship log for the new commit
            let authorship_log = match git_ai::git::refs::show_authorship_note(&repo, &head_commit)
            {
                Some(content) => AuthorshipLog::deserialize_from_string(&content)
                    .map_err(|e| format!("Failed to parse authorship log: {}", e))?,
                None => {
                    return Err("No authorship log found for the new commit".to_string());
                }
            };

            Ok(NewCommit {
                commit_sha: head_commit,
                authorship_log,
                stdout: combined,
            })
        } else {
            Err(output.unwrap_err())
        }
    }

    pub fn read_file(&self, filename: &str) -> Option<String> {
        let file_path = self.path.join(filename);
        fs::read_to_string(&file_path).ok()
    }
}

impl Drop for TestRepo {
    fn drop(&mut self) {
        fs::remove_dir_all(self.path.clone()).expect("failed to remove test repo");
    }
}

#[derive(Debug)]
pub struct NewCommit {
    pub authorship_log: AuthorshipLog,
    pub stdout: String,
    pub commit_sha: String,
}

impl NewCommit {
    pub fn assert_authorship_snapshot(&self) {
        assert_debug_snapshot!(self.authorship_log);
    }
    pub fn print_authorship(&self) {
        // Debug method to print authorship log
        println!("{}", self.authorship_log.serialize_to_string().unwrap());
    }
}

static COMPILED_BINARY: OnceLock<PathBuf> = OnceLock::new();

fn compile_binary() -> PathBuf {
    println!("Compiling git-ai binary for tests...");

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let output = Command::new("cargo")
        .args(&["build", "--bin", "git-ai"])
        .current_dir(manifest_dir)
        .output()
        .expect("Failed to compile git-ai binary");

    if !output.status.success() {
        panic!(
            "Failed to compile git-ai:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let binary_path = PathBuf::from(manifest_dir).join("target/debug/git-ai");
    binary_path
}

fn get_binary_path() -> &'static PathBuf {
    COMPILED_BINARY.get_or_init(compile_binary)
}

#[cfg(test)]
mod tests {
    use super::super::test_file::ExpectedLineExt;
    use super::TestRepo;
    use crate::lines;

    #[test]
    fn test_invoke_git() {
        let repo = TestRepo::new();
        let output = repo.git(&["status"]).expect("git status should succeed");
        println!("output: {}", output);
        assert!(output.contains("On branch"));
    }

    #[test]
    fn test_invoke_git_ai() {
        let repo = TestRepo::new();
        let output = repo
            .git_ai(&["version"])
            .expect("git-ai version should succeed");
        assert!(!output.is_empty());
    }

    // #[test]
    // fn test_exp() {
    //     let repo = TestRepo::new();

    //     let mut example_txt = repo.filename("example.txt");
    //     example_txt.set_contents(vec!["og".human(), "og2".ai()]);

    //     example_txt.insert_at(
    //         0,
    //         lines![
    //             "HUMAN",
    //             "HUMAN".ai(),
    //             "HUMAN",
    //             "HUMAN",
    //             "Hello, world!".ai(),
    //         ],
    //     );

    //     example_txt.delete_at(3);

    //     let _commit = repo.stage_all_and_commit("mix ai human").unwrap();

    //     // Assert that blame output matches expected authorship
    //     example_txt.assert_blame_contents_expected();

    //     example_txt.assert_blame_snapshot();

    //     example_txt.assert_contents_expected();
    // }

    #[test]
    fn test_assert_lines_and_blame() {
        let repo = TestRepo::new();

        let mut example_txt = repo.filename("example.txt");

        // Set up the file with some AI and human lines
        example_txt.set_contents(lines!["line 1", "line 2".ai(), "line 3", "line 4".ai()]);

        repo.stage_all_and_commit("test commit").unwrap();

        // Now assert the exact output using the new syntax
        example_txt.assert_lines_and_blame(lines![
            "line 1".human(),
            "line 2".ai(),
            "line 3".human(),
            "line 4".ai(),
        ]);
    }
}
