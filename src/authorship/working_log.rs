use crate::authorship::transcript::AiTranscript;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/* Types  */
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Line {
    Single(u32),
    Range(u32, u32),
}

#[allow(dead_code)]
impl Line {
    pub fn start(&self) -> u32 {
        match self {
            Line::Single(line) => *line,
            Line::Range(start, _) => *start,
        }
    }

    pub fn end(&self) -> u32 {
        match self {
            Line::Single(line) => *line,
            Line::Range(_, end) => *end,
        }
    }

    /// Check if this line/range contains a given line number
    pub fn contains(&self, line_number: u32) -> bool {
        match self {
            Line::Single(line) => *line == line_number,
            Line::Range(start, end) => line_number >= *start && line_number <= *end,
        }
    }
}

impl fmt::Display for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Line::Single(line) => write!(f, "{}", line),
            Line::Range(start, end) => write!(f, "[{}, {}]", start, end),
        }
    }
}

/// Represents a working log entry for a specific file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkingLogEntry {
    /// The file path relative to the repository root
    pub file: String,
    /// SHA256 hash of the file content at this checkpoint
    #[serde(default)]
    pub blob_sha: String,
    /// List of lines or line ranges that were added
    pub added_lines: Vec<Line>,
    /// List of lines or line ranges that were deleted
    pub deleted_lines: Vec<Line>,
}

impl WorkingLogEntry {
    /// Create a new working log entry
    pub fn new(
        file: String,
        blob_sha: String,
        added_lines: Vec<Line>,
        deleted_lines: Vec<Line>,
    ) -> Self {
        Self {
            file,
            blob_sha,
            added_lines,
            deleted_lines,
        }
    }

    /// Add a single line to the added lines
    pub fn add_added_line(&mut self, line: u32) {
        self.added_lines.push(Line::Single(line));
    }

    /// Add a line range to the added lines
    pub fn add_added_range(&mut self, start: u32, end: u32) {
        self.added_lines.push(Line::Range(start, end));
    }

    /// Add a single line to the deleted lines
    pub fn add_deleted_line(&mut self, line: u32) {
        self.deleted_lines.push(Line::Single(line));
    }

    /// Add a line range to the deleted lines
    pub fn add_deleted_range(&mut self, start: u32, end: u32) {
        self.deleted_lines.push(Line::Range(start, end));
    }

    /// Check if a specific line number is covered by this working log entry
    pub fn covers_line(&self, line_number: u32) -> bool {
        self.added_lines
            .iter()
            .any(|line| line.contains(line_number))
            || self
                .deleted_lines
                .iter()
                .any(|line| line.contains(line_number))
    }

    /// Get all lines (both added and deleted) for backward compatibility
    pub fn all_lines(&self) -> Vec<Line> {
        let mut all_lines = Vec::new();
        all_lines.extend(self.added_lines.clone());
        all_lines.extend(self.deleted_lines.clone());
        all_lines
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentId {
    pub tool: String, // e.g., "cursor", "windsurf"
    pub id: String,   // id in their domain
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub diff: String,
    pub author: String,
    pub entries: Vec<WorkingLogEntry>,
    pub timestamp: u64,
    pub transcript: Option<AiTranscript>,
    pub agent_id: Option<AgentId>,
    #[serde(default)]
    pub allow_reset_to_checkpoint: bool,
    #[serde(default = "default_pass_through_attribution_checkpoint")]
    pub pass_through_attribution_checkpoint: bool,
}

impl Checkpoint {
    pub fn new(diff: String, author: String, entries: Vec<WorkingLogEntry>) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            diff,
            author,
            entries,
            timestamp,
            transcript: None,
            agent_id: None,
            allow_reset_to_checkpoint: false,
            pass_through_attribution_checkpoint: false,
        }
    }

    /// Create a passthrough checkpoint that tracks line changes but doesn't attribute authorship
    pub fn new_passthrough(diff: String, author: String, entries: Vec<WorkingLogEntry>) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            diff,
            author,
            entries,
            timestamp,
            transcript: None, // Passthrough checkpoints never have transcripts
            agent_id: None,   // Passthrough checkpoints never have agent_id
            allow_reset_to_checkpoint: false,
            pass_through_attribution_checkpoint: true, // This is the key difference
        }
    }
}

fn default_pass_through_attribution_checkpoint() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authorship::transcript::Message;

    #[test]
    fn test_line_serialization() {
        let single_line = Line::Single(5);
        let range_line = Line::Range(10, 15);

        let single_json = serde_json::to_string(&single_line).unwrap();
        let range_json = serde_json::to_string(&range_line).unwrap();

        assert_eq!(single_json, "5");
        assert_eq!(range_json, "[10,15]");

        let deserialized_single: Line = serde_json::from_str(&single_json).unwrap();
        let deserialized_range: Line = serde_json::from_str(&range_json).unwrap();

        assert_eq!(deserialized_single, single_line);
        assert_eq!(deserialized_range, range_line);
    }

    #[test]
    fn test_checkpoint_serialization() {
        let entry = WorkingLogEntry::new(
            "src/xyz.rs".to_string(),
            "abc123def456".to_string(),
            vec![Line::Single(1), Line::Range(2, 5), Line::Single(10)],
            vec![],
        );
        let checkpoint = Checkpoint::new("".to_string(), "claude".to_string(), vec![entry]);

        // Verify timestamp is set (should be recent)
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        assert!(checkpoint.timestamp > 0);
        assert!(checkpoint.timestamp <= current_time);
        assert!(checkpoint.transcript.is_none());
        assert!(checkpoint.agent_id.is_none());

        let json = serde_json::to_string_pretty(&checkpoint).unwrap();
        let deserialized: Checkpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.diff, "");
        assert_eq!(deserialized.entries.len(), 1);
        assert_eq!(deserialized.entries[0].file, "src/xyz.rs");
        assert_eq!(deserialized.entries[0].blob_sha, "abc123def456");
        assert_eq!(deserialized.timestamp, checkpoint.timestamp);
        assert!(deserialized.transcript.is_none());
        assert!(deserialized.agent_id.is_none());
    }

    #[test]
    fn test_log_array_serialization() {
        let entry1 = WorkingLogEntry::new(
            "src/xyz.rs".to_string(),
            "sha1".to_string(),
            vec![Line::Single(1), Line::Range(2, 5), Line::Single(10)],
            vec![],
        );
        let checkpoint1 = Checkpoint::new("".to_string(), "claude".to_string(), vec![entry1]);

        let entry2 = WorkingLogEntry::new(
            "src/xyz.rs".to_string(),
            "sha2".to_string(),
            vec![Line::Single(12), Line::Single(13)],
            vec![],
        );
        let checkpoint2 = Checkpoint::new(
            "/refs/ai/working/xyz.patch".to_string(),
            "user".to_string(),
            vec![entry2],
        );

        // Verify timestamps are set and checkpoint2 is newer than checkpoint1
        assert!(checkpoint1.timestamp > 0);
        assert!(checkpoint2.timestamp > 0);
        assert!(checkpoint2.timestamp >= checkpoint1.timestamp);

        let log = vec![checkpoint1, checkpoint2];
        let json = serde_json::to_string_pretty(&log).unwrap();
        // println!("Working log array JSON:\n{}", json);
        let deserialized: Vec<Checkpoint> = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.len(), 2);
        assert_eq!(deserialized[0].diff, "");
        assert_eq!(deserialized[1].diff, "/refs/ai/working/xyz.patch");
        assert_eq!(deserialized[1].author, "user");
    }

    #[test]
    fn test_line_contains() {
        let single = Line::Single(5);
        let range = Line::Range(10, 15);

        assert!(single.contains(5));
        assert!(!single.contains(6));

        assert!(range.contains(10));
        assert!(range.contains(12));
        assert!(range.contains(15));
        assert!(!range.contains(9));
        assert!(!range.contains(16));
    }

    #[test]
    fn test_working_log_entry_covers_line() {
        let entry = WorkingLogEntry::new(
            "src/xyz.rs".to_string(),
            "test_sha".to_string(),
            vec![Line::Single(1), Line::Range(2, 5), Line::Single(10)],
            vec![],
        );

        assert!(entry.covers_line(1));
        assert!(entry.covers_line(2));
        assert!(entry.covers_line(5));
        assert!(entry.covers_line(10));
        assert!(!entry.covers_line(6));
        assert!(!entry.covers_line(11));
    }

    #[test]
    fn test_checkpoint_with_transcript() {
        let entry = WorkingLogEntry::new(
            "src/xyz.rs".to_string(),
            "test_sha".to_string(),
            vec![Line::Single(1)],
            vec![],
        );

        let user_message = Message::user("Please add error handling to this function".to_string());
        let assistant_message =
            Message::assistant("I'll add error handling to the function.".to_string());

        let mut transcript = AiTranscript::new();
        transcript.add_message(user_message);
        transcript.add_message(assistant_message);

        let agent_id = AgentId {
            tool: "cursor".to_string(),
            model: "gpt-4o".to_string(),
            id: "session-abc123".to_string(),
        };

        let mut checkpoint = Checkpoint::new("".to_string(), "claude".to_string(), vec![entry]);
        checkpoint.transcript = Some(transcript);
        checkpoint.agent_id = Some(agent_id);

        assert!(checkpoint.transcript.is_some());
        assert!(checkpoint.agent_id.is_some());

        let transcript_data = checkpoint.transcript.as_ref().unwrap();
        assert_eq!(transcript_data.messages().len(), 2);

        // Check first message (user)
        match &transcript_data.messages()[0] {
            Message::User { text, .. } => {
                assert_eq!(text, "Please add error handling to this function");
            }
            _ => panic!("Expected user message"),
        }

        // Check second message (assistant)
        match &transcript_data.messages()[1] {
            Message::Assistant { text, .. } => {
                assert_eq!(text, "I'll add error handling to the function.");
            }
            _ => panic!("Expected assistant message"),
        }

        let agent_data = checkpoint.agent_id.as_ref().unwrap();
        assert_eq!(agent_data.tool, "cursor");
        assert_eq!(agent_data.id, "session-abc123");

        let json = serde_json::to_string_pretty(&checkpoint).unwrap();
        let deserialized: Checkpoint = serde_json::from_str(&json).unwrap();
        assert!(deserialized.transcript.is_some());
        assert!(deserialized.agent_id.is_some());

        let deserialized_transcript = deserialized.transcript.as_ref().unwrap();
        assert_eq!(deserialized_transcript.messages().len(), 2);

        let deserialized_agent = deserialized.agent_id.as_ref().unwrap();
        assert_eq!(deserialized_agent.tool, "cursor");
        assert_eq!(deserialized_agent.id, "session-abc123");
    }
}
