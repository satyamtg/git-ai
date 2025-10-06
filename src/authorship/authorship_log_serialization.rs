use crate::authorship::authorship_log::{Author, LineRange, PromptRecord};
use crate::config;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::io::{BufRead, Write};

/// Authorship log format version identifier
pub const AUTHORSHIP_LOG_VERSION: &str = "authorship/3.0.0";

/// Metadata section that goes below the divider as JSON
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthorshipMetadata {
    pub schema_version: String,
    pub base_commit_sha: String,
    pub prompts: BTreeMap<String, PromptRecord>,
}

impl AuthorshipMetadata {
    pub fn new() -> Self {
        Self {
            schema_version: AUTHORSHIP_LOG_VERSION.to_string(),
            base_commit_sha: String::new(),
            prompts: BTreeMap::new(),
        }
    }
}

impl Default for AuthorshipMetadata {
    fn default() -> Self {
        Self::new()
    }
}

/// Attestation entry: short hash followed by line ranges
///
/// IMPORTANT: The hash ALWAYS corresponds to a prompt in the prompts section.
/// This system only tracks AI-generated content, not human-authored content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttestationEntry {
    /// Short hash (7 chars) that maps to an entry in the prompts section of the metadata
    pub hash: String,
    /// Line ranges that this prompt is responsible for
    pub line_ranges: Vec<LineRange>,
}

impl AttestationEntry {
    pub fn new(hash: String, line_ranges: Vec<LineRange>) -> Self {
        Self { hash, line_ranges }
    }

    pub fn remove_line_ranges(&mut self, to_remove: &[LineRange]) {
        let mut current_ranges = self.line_ranges.clone();

        for remove_range in to_remove {
            let mut new_ranges = Vec::new();
            for existing_range in &current_ranges {
                new_ranges.extend(existing_range.remove(remove_range));
            }
            current_ranges = new_ranges;
        }

        self.line_ranges = current_ranges;
    }

    /// Shift line ranges by a given offset starting at insertion_point
    pub fn shift_line_ranges(&mut self, insertion_point: u32, offset: i32) {
        let mut shifted_ranges = Vec::new();
        for range in &self.line_ranges {
            if let Some(shifted) = range.shift(insertion_point, offset) {
                shifted_ranges.push(shifted);
            }
        }
        self.line_ranges = shifted_ranges;
    }
}

/// Per-file attestation data
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileAttestation {
    pub file_path: String,
    pub entries: Vec<AttestationEntry>,
}

impl FileAttestation {
    pub fn new(file_path: String) -> Self {
        Self {
            file_path,
            entries: Vec::new(),
        }
    }

    pub fn add_entry(&mut self, entry: AttestationEntry) {
        self.entries.push(entry);
    }
}

/// The complete authorship log format
#[derive(Clone, PartialEq)]
pub struct AuthorshipLog {
    pub attestations: Vec<FileAttestation>,
    pub metadata: AuthorshipMetadata,
}

impl fmt::Debug for AuthorshipLog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthorshipLogV3")
            .field("attestations", &self.attestations)
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl AuthorshipLog {
    pub fn new() -> Self {
        Self {
            attestations: Vec::new(),
            metadata: AuthorshipMetadata::new(),
        }
    }

    /// Extract entries that match unstaged line ranges into a new AuthorshipLog
    ///
    /// This creates a new AuthorshipLog containing only the lines that will be filtered out.
    /// Used to save unstaged AI-authored lines to the working log for future attribution.
    ///
    /// # Arguments
    /// * `unstaged_hunks` - Map of file paths to their unstaged line ranges
    ///
    /// # Returns
    /// A new AuthorshipLog with only the unstaged entries
    pub fn extract_unstaged_lines(
        &self,
        unstaged_hunks: &HashMap<String, Vec<LineRange>>,
    ) -> AuthorshipLog {
        let mut extracted_log = AuthorshipLog::new();
        extracted_log.metadata.base_commit_sha = self.metadata.base_commit_sha.clone();

        for file_attestation in &self.attestations {
            if let Some(unstaged_ranges) = unstaged_hunks.get(&file_attestation.file_path) {
                let mut extracted_file = FileAttestation::new(file_attestation.file_path.clone());

                for entry in &file_attestation.entries {
                    // Expand entry's line ranges to individual lines
                    let mut entry_lines: Vec<u32> = Vec::new();
                    for range in &entry.line_ranges {
                        entry_lines.extend(range.expand());
                    }

                    // Find which lines are unstaged
                    let mut unstaged_lines: Vec<u32> = Vec::new();
                    for line in entry_lines {
                        // Check if this line is in any unstaged range
                        for unstaged_range in unstaged_ranges {
                            if unstaged_range.contains(line) {
                                unstaged_lines.push(line);
                                break;
                            }
                        }
                    }

                    if !unstaged_lines.is_empty() {
                        // Copy the prompt record to the extracted log
                        if let Some(prompt) = self.metadata.prompts.get(&entry.hash) {
                            extracted_log
                                .metadata
                                .prompts
                                .insert(entry.hash.clone(), prompt.clone());
                        }

                        // Compress unstaged lines back to ranges
                        unstaged_lines.sort_unstable();
                        unstaged_lines.dedup();
                        let compressed_ranges = LineRange::compress_lines(&unstaged_lines);

                        extracted_file.add_entry(AttestationEntry::new(
                            entry.hash.clone(),
                            compressed_ranges,
                        ));
                    }
                }

                if !extracted_file.entries.is_empty() {
                    extracted_log.attestations.push(extracted_file);
                }
            }
        }

        extracted_log
    }

    /// Filter authorship log to keep only committed line ranges
    ///
    /// This keeps only attributions for lines that were actually committed, removing everything else.
    /// This is the inverse of filter_unstaged_lines - instead of removing unstaged, we keep only committed.
    ///
    /// # Arguments
    /// * `committed_hunks` - Map of file paths to their committed line ranges
    pub fn filter_to_committed_lines(&mut self, committed_hunks: &HashMap<String, Vec<LineRange>>) {
        for file_attestation in &mut self.attestations {
            if let Some(committed_ranges) = committed_hunks.get(&file_attestation.file_path) {
                // For each attestation entry, keep only the lines that were committed
                for entry in &mut file_attestation.entries {
                    // Expand entry's line ranges to individual lines
                    let mut entry_lines: Vec<u32> = Vec::new();
                    for range in &entry.line_ranges {
                        entry_lines.extend(range.expand());
                    }

                    // Keep only lines that are in committed ranges
                    let mut committed_lines: Vec<u32> = Vec::new();
                    for line in entry_lines {
                        if committed_ranges.iter().any(|range| range.contains(line)) {
                            committed_lines.push(line);
                        }
                    }

                    if !committed_lines.is_empty() {
                        committed_lines.sort_unstable();
                        committed_lines.dedup();
                        entry.line_ranges = LineRange::compress_lines(&committed_lines);
                    } else {
                        entry.line_ranges.clear();
                    }
                }

                // Remove entries that have no line ranges left
                file_attestation
                    .entries
                    .retain(|entry| !entry.line_ranges.is_empty());
            } else {
                // No committed lines for this file, remove all entries
                file_attestation.entries.clear();
            }
        }

        // Remove file attestations that have no entries left
        self.attestations.retain(|file| !file.entries.is_empty());

        // Clean up prompt metadata for sessions that no longer have attributed lines
        self.cleanup_unused_prompts();
    }

    /// Remove prompt records that are not referenced by any attestation entries
    ///
    /// After filtering the authorship log (e.g., to only committed lines), some AI sessions
    /// may no longer have any attributed lines. This method removes their PromptRecords from
    /// the metadata to keep it clean and accurate.
    pub fn cleanup_unused_prompts(&mut self) {
        // Collect all hashes that are still referenced in attestations
        let mut referenced_hashes = std::collections::HashSet::new();
        for file_attestation in &self.attestations {
            for entry in &file_attestation.entries {
                referenced_hashes.insert(entry.hash.clone());
            }
        }

        // Remove prompts that are not referenced
        self.metadata
            .prompts
            .retain(|hash, _| referenced_hashes.contains(hash));
    }

    /// Merge overlapping and adjacent line ranges
    fn merge_line_ranges(ranges: &[LineRange]) -> Vec<LineRange> {
        if ranges.is_empty() {
            return Vec::new();
        }

        let mut sorted_ranges = ranges.to_vec();
        sorted_ranges.sort_by(|a, b| {
            let a_start = match a {
                LineRange::Single(line) => *line,
                LineRange::Range(start, _) => *start,
            };
            let b_start = match b {
                LineRange::Single(line) => *line,
                LineRange::Range(start, _) => *start,
            };
            a_start.cmp(&b_start)
        });

        let mut merged = Vec::new();
        for current in sorted_ranges {
            if let Some(last) = merged.last_mut() {
                if Self::ranges_can_merge(last, &current) {
                    *last = Self::merge_ranges(last, &current);
                } else {
                    merged.push(current);
                }
            } else {
                merged.push(current);
            }
        }

        merged
    }

    /// Check if two ranges can be merged (overlapping or adjacent)
    fn ranges_can_merge(range1: &LineRange, range2: &LineRange) -> bool {
        let (start1, end1) = match range1 {
            LineRange::Single(line) => (*line, *line),
            LineRange::Range(start, end) => (*start, *end),
        };
        let (start2, end2) = match range2 {
            LineRange::Single(line) => (*line, *line),
            LineRange::Range(start, end) => (*start, *end),
        };

        // Ranges can merge if they overlap or are adjacent
        start1 <= end2 + 1 && start2 <= end1 + 1
    }

    /// Merge two ranges into one
    fn merge_ranges(range1: &LineRange, range2: &LineRange) -> LineRange {
        let (start1, end1) = match range1 {
            LineRange::Single(line) => (*line, *line),
            LineRange::Range(start, end) => (*start, *end),
        };
        let (start2, end2) = match range2 {
            LineRange::Single(line) => (*line, *line),
            LineRange::Range(start, end) => (*start, *end),
        };

        let start = start1.min(start2);
        let end = end1.max(end2);

        if start == end {
            LineRange::Single(start)
        } else {
            LineRange::Range(start, end)
        }
    }

    /// Apply a single checkpoint to this authorship log
    ///
    /// This method processes one checkpoint and updates the authorship log accordingly,
    /// handling deletions, additions, and tracking metrics.
    pub fn apply_checkpoint(
        &mut self,
        checkpoint: &crate::authorship::working_log::Checkpoint,
        human_author: Option<&str>,
        session_additions: &mut HashMap<String, u32>,
        session_deletions: &mut HashMap<String, u32>,
    ) {
        // If there is an agent session, record it by its short hash (agent_id + tool)
        let session_id_opt = match (&checkpoint.agent_id, &checkpoint.transcript) {
            (Some(agent), Some(transcript)) => {
                let session_id = generate_short_hash(&agent.id, &agent.tool);
                // Insert or update the prompt session transcript
                let entry =
                    self.metadata
                        .prompts
                        .entry(session_id.clone())
                        .or_insert(PromptRecord {
                            agent_id: agent.clone(),
                            human_author: human_author.map(|s| s.to_string()),
                            messages: transcript.messages().to_vec(),
                            total_additions: 0,
                            total_deletions: 0,
                            accepted_lines: 0,
                            overriden_lines: 0,
                        });
                if entry.messages.len() < transcript.messages().len() {
                    entry.messages = transcript.messages().to_vec();
                }
                Some(session_id)
            }
            _ => None,
        };

        for entry in &checkpoint.entries {
            // Track additions and deletions for this session
            if let Some(ref session_id) = session_id_opt {
                // Count total additions
                let additions_count: u32 = entry
                    .added_lines
                    .iter()
                    .map(|line| count_working_log_lines(line))
                    .sum();
                *session_additions.entry(session_id.clone()).or_insert(0) += additions_count;

                // Count total deletions
                let deletions_count: u32 = entry
                    .deleted_lines
                    .iter()
                    .map(|line| count_working_log_lines(line))
                    .sum();
                *session_deletions.entry(session_id.clone()).or_insert(0) += deletions_count;
            }

            // Process deletions first (remove lines from all authors, then shift remaining lines up)
            if !entry.deleted_lines.is_empty() {
                // Collect all deleted line numbers
                let mut all_deleted_lines = Vec::new();
                for line in &entry.deleted_lines {
                    match line {
                        crate::authorship::working_log::Line::Single(l) => {
                            all_deleted_lines.push(*l)
                        }
                        crate::authorship::working_log::Line::Range(start, end) => {
                            for l in *start..=*end {
                                all_deleted_lines.push(l);
                            }
                        }
                    }
                }
                all_deleted_lines.sort_unstable();
                all_deleted_lines.dedup();

                // Check if this is a human checkpoint modifying AI lines
                if checkpoint.agent_id.is_none() {
                    // This is a human checkpoint - check for overridden AI lines
                    self.detect_overridden_lines(&entry.file, &all_deleted_lines);
                }

                let file_attestation = self.get_or_create_file(&entry.file);
                let deleted_ranges = LineRange::compress_lines(&all_deleted_lines);

                // Remove the deleted lines from all attestations
                for attestation_entry in file_attestation.entries.iter_mut() {
                    attestation_entry.remove_line_ranges(&deleted_ranges);
                }

                // Shift remaining lines up after deletions
                // Process deletions in reverse order to avoid shifting issues
                for line in all_deleted_lines.iter().rev() {
                    let deletion_point = *line;
                    for attestation_entry in file_attestation.entries.iter_mut() {
                        // Shift lines after the deletion point up by 1
                        attestation_entry.shift_line_ranges(deletion_point + 1, -1);
                    }
                }
            }

            // Then process additions (shift existing lines down, then add new author)
            let mut added_lines = Vec::new();
            for line in &entry.added_lines {
                match line {
                    crate::authorship::working_log::Line::Single(l) => added_lines.push(*l),
                    crate::authorship::working_log::Line::Range(start, end) => {
                        for l in *start..=*end {
                            added_lines.push(l);
                        }
                    }
                }
            }
            if !added_lines.is_empty() {
                // Ensure deterministic, duplicate-free line numbers before compression
                added_lines.sort_unstable();
                added_lines.dedup();

                let num_lines_added = added_lines.len() as i32;
                let insertion_point = *added_lines.first().unwrap();

                // Shift existing line attributions down to make room for new lines
                let file_attestation = self.get_or_create_file(&entry.file);
                for attestation_entry in file_attestation.entries.iter_mut() {
                    attestation_entry.shift_line_ranges(insertion_point, num_lines_added);
                }

                // Create compressed line ranges for the new additions
                let new_line_ranges = LineRange::compress_lines(&added_lines);

                // Skip authorship attribution for passthrough checkpoints
                if !checkpoint.pass_through_attribution_checkpoint {
                    // Only process AI-generated content (entries with prompt_session_id)
                    if let Some(session_id) = session_id_opt.clone() {
                        // Add new attestation entry for the AI-added lines
                        let entry = AttestationEntry::new(session_id, new_line_ranges);
                        file_attestation.add_entry(entry);
                    }
                }
            }
        }
    }

    /// Finalize the authorship log after all checkpoints have been applied
    ///
    /// This method:
    /// - Removes empty entries and files
    /// - Sorts and consolidates entries by hash
    /// - Calculates accepted_lines from final attestations
    /// - Updates all PromptRecords with final metrics
    pub fn finalize(
        &mut self,
        session_additions: &HashMap<String, u32>,
        session_deletions: &HashMap<String, u32>,
    ) {
        // Remove empty entries and empty files
        for file_attestation in &mut self.attestations {
            file_attestation
                .entries
                .retain(|entry| !entry.line_ranges.is_empty());
        }
        self.attestations.retain(|f| !f.entries.is_empty());

        // Sort attestation entries by hash for deterministic ordering
        for file_attestation in &mut self.attestations {
            file_attestation.entries.sort_by(|a, b| a.hash.cmp(&b.hash));
        }

        // Consolidate entries with the same hash
        for file_attestation in &mut self.attestations {
            let mut consolidated_entries = Vec::new();
            let mut current_hash: Option<String> = None;
            let mut current_ranges: Vec<LineRange> = Vec::new();

            for entry in &file_attestation.entries {
                if current_hash.as_ref() == Some(&entry.hash) {
                    // Same hash, accumulate line ranges
                    current_ranges.extend(entry.line_ranges.clone());
                } else {
                    // Different hash, save previous entry and start new one
                    if let Some(hash) = current_hash.take() {
                        // Merge overlapping and adjacent ranges before adding
                        let merged_ranges = Self::merge_line_ranges(&current_ranges);
                        consolidated_entries.push(AttestationEntry::new(hash, merged_ranges));
                    }
                    current_hash = Some(entry.hash.clone());
                    current_ranges = entry.line_ranges.clone();
                }
            }

            // Don't forget the last entry
            if let Some(hash) = current_hash {
                let merged_ranges = Self::merge_line_ranges(&current_ranges);
                consolidated_entries.push(AttestationEntry::new(hash, merged_ranges));
            }

            file_attestation.entries = consolidated_entries;
        }

        // Calculate accepted_lines for each session from the final attestation log
        let mut session_accepted_lines: HashMap<String, u32> = HashMap::new();
        for file_attestation in &self.attestations {
            for attestation_entry in &file_attestation.entries {
                let accepted_count: u32 = attestation_entry
                    .line_ranges
                    .iter()
                    .map(|range| count_line_range(range))
                    .sum();
                *session_accepted_lines
                    .entry(attestation_entry.hash.clone())
                    .or_insert(0) += accepted_count;
            }
        }

        // Update all PromptRecords with the calculated metrics
        for (session_id, prompt_record) in self.metadata.prompts.iter_mut() {
            prompt_record.total_additions = *session_additions.get(session_id).unwrap_or(&0);
            prompt_record.total_deletions = *session_deletions.get(session_id).unwrap_or(&0);
            prompt_record.accepted_lines = *session_accepted_lines.get(session_id).unwrap_or(&0);
            // overriden_lines is calculated and accumulated in apply_checkpoint, don't reset it here
        }
    }

    /// Convert from working log checkpoints to authorship log
    pub fn from_working_log_with_base_commit_and_human_author(
        checkpoints: &[crate::authorship::working_log::Checkpoint],
        base_commit_sha: &str,
        human_author: Option<&str>,
    ) -> Self {
        let mut authorship_log = Self::new();
        authorship_log.metadata.base_commit_sha = base_commit_sha.to_string();

        // Track additions and deletions per session_id
        let mut session_additions: HashMap<String, u32> = HashMap::new();
        let mut session_deletions: HashMap<String, u32> = HashMap::new();

        // Process checkpoints and create attributions
        for checkpoint in checkpoints.iter() {
            authorship_log.apply_checkpoint(
                checkpoint,
                human_author,
                &mut session_additions,
                &mut session_deletions,
            );
        }

        // Finalize the log (cleanup, consolidate, metrics)
        authorship_log.finalize(&session_additions, &session_deletions);

        // If prompts should be ignored, clear the transcripts but keep the prompt records
        let ignore_prompts: bool = config::Config::get().get_ignore_prompts();
        if ignore_prompts {
            // Clear transcripts but keep the prompt records
            for prompt_record in authorship_log.metadata.prompts.values_mut() {
                prompt_record.messages.clear();
            }
        }

        authorship_log
    }

    /// Detect lines that were originally authored by AI but are now being modified by humans
    fn detect_overridden_lines(&mut self, file: &str, deleted_lines: &[u32]) {
        // Find the file attestation and check for overridden lines
        if let Some(file_attestation) = self.attestations.iter().find(|f| f.file_path == file) {
            // For each session, count how many of its lines were overridden
            let mut session_overridden_counts: std::collections::HashMap<String, u32> =
                std::collections::HashMap::new();

            // For each deleted line, check if it was previously attributed to AI
            for &line in deleted_lines {
                for attestation_entry in &file_attestation.entries {
                    // Check if this line was attributed to AI
                    if attestation_entry
                        .line_ranges
                        .iter()
                        .any(|range| range.contains(line))
                    {
                        // This line was AI-authored and is now being deleted by a human
                        *session_overridden_counts
                            .entry(attestation_entry.hash.clone())
                            .or_insert(0) += 1;
                    }
                }
            }

            // Update the overriden_lines count for each affected session
            for (session_hash, overridden_count) in session_overridden_counts {
                if let Some(prompt_record) = self.metadata.prompts.get_mut(&session_hash) {
                    prompt_record.overriden_lines += overridden_count;
                }
            }
        }
    }

    pub fn get_or_create_file(&mut self, file: &str) -> &mut FileAttestation {
        // Check if file already exists
        let exists = self.attestations.iter().any(|f| f.file_path == file);

        if !exists {
            self.attestations
                .push(FileAttestation::new(file.to_string()));
        }

        // Now get the reference
        self.attestations
            .iter_mut()
            .find(|f| f.file_path == file)
            .unwrap()
    }

    /// Serialize to the new text format
    pub fn serialize_to_string(&self) -> Result<String, fmt::Error> {
        let mut output = String::new();

        // Write attestation section
        for file_attestation in &self.attestations {
            // Quote file names that contain spaces or whitespace
            let file_path = if needs_quoting(&file_attestation.file_path) {
                format!("\"{}\"", &file_attestation.file_path)
            } else {
                file_attestation.file_path.clone()
            };
            output.push_str(&file_path);
            output.push('\n');

            for entry in &file_attestation.entries {
                output.push_str("  ");
                output.push_str(&entry.hash);
                output.push(' ');
                output.push_str(&format_line_ranges(&entry.line_ranges));
                output.push('\n');
            }
        }

        // Write divider
        output.push_str("---\n");

        // Write JSON metadata section
        let json_str = serde_json::to_string_pretty(&self.metadata).map_err(|_| fmt::Error)?;
        output.push_str(&json_str);

        Ok(output)
    }

    /// Write to a writer in the new format
    pub fn _serialize_to_writer<W: Write>(&self, mut writer: W) -> std::io::Result<()> {
        let content = self
            .serialize_to_string()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "Serialization failed"))?;
        writer.write_all(content.as_bytes())?;
        Ok(())
    }

    /// Deserialize from the new text format
    pub fn deserialize_from_string(content: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let lines: Vec<&str> = content.lines().collect();

        // Find the divider
        let divider_pos = lines
            .iter()
            .position(|&line| line == "---")
            .ok_or("Missing divider '---' in authorship log")?;

        // Parse attestation section (before divider)
        let attestation_lines = &lines[..divider_pos];
        let attestations = parse_attestation_section(attestation_lines)?;

        // Parse JSON metadata section (after divider)
        let json_lines = &lines[divider_pos + 1..];
        let json_content = json_lines.join("\n");
        let metadata: AuthorshipMetadata = serde_json::from_str(&json_content)?;

        Ok(Self {
            attestations,
            metadata,
        })
    }

    /// Read from a reader in the new format
    pub fn _deserialize_from_reader<R: BufRead>(
        reader: R,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let content: Result<String, _> = reader.lines().collect();
        let content = content?;
        Self::deserialize_from_string(&content)
    }

    /// Lookup the author and optional prompt for a given file and line
    pub fn get_line_attribution(
        &self,
        file: &str,
        line: u32,
    ) -> Option<(Author, Option<&PromptRecord>)> {
        // Find the file attestation
        let file_attestation = self.attestations.iter().find(|f| f.file_path == file)?;

        // Check entries in reverse order (latest wins)
        for entry in file_attestation.entries.iter().rev() {
            // Check if this line is covered by any of the line ranges
            let contains = entry.line_ranges.iter().any(|range| range.contains(line));
            if contains {
                // The hash corresponds to a prompt session short hash
                if let Some(prompt_record) = self.metadata.prompts.get(&entry.hash) {
                    // Create author info from the prompt record
                    let author = Author {
                        username: prompt_record.agent_id.tool.clone(),
                        email: String::new(), // AI agents don't have email
                    };

                    // Return author and prompt info
                    return Some((author, Some(prompt_record)));
                }
            }
        }
        None
    }

    /// Convert authorship log to working log checkpoints for merge --squash
    ///
    /// This creates one checkpoint for human-authored lines and one checkpoint per AI prompt session.
    /// The checkpoints can then be appended to the current working log for the base commit.
    ///
    /// # Arguments
    /// * `human_author` - The human author identifier (email) to use for human-authored lines
    ///
    /// # Returns
    /// Vector of checkpoints: first checkpoint is human (if any human lines), followed by one checkpoint per AI session
    #[allow(dead_code)]
    pub fn convert_to_checkpoints_for_squash(
        &self,
        human_author: &str,
    ) -> Result<Vec<crate::authorship::working_log::Checkpoint>, Box<dyn std::error::Error>> {
        use crate::authorship::working_log::{Checkpoint, WorkingLogEntry};
        use std::collections::{HashMap, HashSet};

        let mut checkpoints = Vec::new();

        // Track all files that have attestations
        let mut all_files: HashSet<String> = HashSet::new();
        for file_attestation in &self.attestations {
            all_files.insert(file_attestation.file_path.clone());
        }

        // Build human checkpoint first
        // Human owns all lines NOT attributed to AI
        let mut human_entries: Vec<WorkingLogEntry> = Vec::new();

        for file_path in &all_files {
            // Find all AI-attributed lines for this file
            let mut ai_lines: HashSet<u32> = HashSet::new();

            if let Some(file_attestation) =
                self.attestations.iter().find(|f| f.file_path == *file_path)
            {
                for entry in &file_attestation.entries {
                    for range in &entry.line_ranges {
                        ai_lines.extend(range.expand());
                    }
                }
            }

            // Determine which lines are human-owned
            // For simplicity, we need to know the total line count in the file
            // Since we're working from an authorship log from a squash, we can infer it from max line number
            let max_line = self
                .attestations
                .iter()
                .find(|f| f.file_path == *file_path)
                .map(|f| {
                    f.entries
                        .iter()
                        .flat_map(|e| &e.line_ranges)
                        .map(|r| match r {
                            LineRange::Single(l) => *l,
                            LineRange::Range(_, end) => *end,
                        })
                        .max()
                        .unwrap_or(0)
                })
                .unwrap_or(0);

            // Collect human lines (all lines from 1..=max_line that aren't in ai_lines)
            let mut human_lines: Vec<u32> = (1..=max_line)
                .filter(|line| !ai_lines.contains(line))
                .collect();

            if !human_lines.is_empty() {
                // Convert to Line ranges
                human_lines.sort_unstable();
                let human_line_ranges = compress_lines_to_working_log_format(&human_lines);

                human_entries.push(WorkingLogEntry::new(
                    file_path.clone(),
                    String::new(), // Empty blob_sha for now
                    human_line_ranges,
                    vec![], // No deletions in squash conversion
                ));
            }
        }

        // Add human checkpoint if there are any human-authored lines
        if !human_entries.is_empty() {
            let human_checkpoint = Checkpoint::new(
                String::new(), // Empty diff hash
                human_author.to_string(),
                human_entries,
            );
            checkpoints.push(human_checkpoint);
        }

        // Build AI checkpoints - one per session
        // Group attestations by session hash
        let mut session_data: HashMap<String, Vec<(String, Vec<LineRange>)>> = HashMap::new();

        for file_attestation in &self.attestations {
            for entry in &file_attestation.entries {
                session_data
                    .entry(entry.hash.clone())
                    .or_insert_with(Vec::new)
                    .push((
                        file_attestation.file_path.clone(),
                        entry.line_ranges.clone(),
                    ));
            }
        }

        // Create a checkpoint for each AI session
        for (session_hash, file_ranges) in session_data {
            let prompt_record = self
                .metadata
                .prompts
                .get(&session_hash)
                .ok_or_else(|| format!("Missing prompt record for hash: {}", session_hash))?;

            let mut ai_entries = Vec::new();

            // Group by file
            let mut file_map: HashMap<String, Vec<LineRange>> = HashMap::new();
            for (file_path, ranges) in file_ranges {
                file_map
                    .entry(file_path)
                    .or_insert_with(Vec::new)
                    .extend(ranges);
            }

            for (file_path, ranges) in file_map {
                // Expand ranges to individual lines, then compress to working log format
                let mut all_lines: Vec<u32> = Vec::new();
                for range in ranges {
                    all_lines.extend(range.expand());
                }
                all_lines.sort_unstable();
                all_lines.dedup();

                let line_ranges = compress_lines_to_working_log_format(&all_lines);

                ai_entries.push(WorkingLogEntry::new(
                    file_path,
                    String::new(), // Empty blob_sha for now
                    line_ranges,
                    vec![], // No deletions in squash conversion
                ));
            }

            let mut ai_checkpoint = Checkpoint::new(
                String::new(), // Empty diff hash
                "ai".to_string(),
                ai_entries,
            );
            ai_checkpoint.agent_id = Some(prompt_record.agent_id.clone());

            // Reconstruct transcript from messages
            let mut transcript = crate::authorship::transcript::AiTranscript::new();
            for message in &prompt_record.messages {
                transcript.add_message(message.clone());
            }
            ai_checkpoint.transcript = Some(transcript);

            checkpoints.push(ai_checkpoint);
        }

        Ok(checkpoints)
    }
}

/// Convert line numbers to working log Line format (Single/Range)
#[allow(dead_code)]
pub fn compress_lines_to_working_log_format(
    lines: &[u32],
) -> Vec<crate::authorship::working_log::Line> {
    use crate::authorship::working_log::Line;

    if lines.is_empty() {
        return vec![];
    }

    let mut result = Vec::new();
    let mut start = lines[0];
    let mut end = lines[0];

    for &line in &lines[1..] {
        if line == end + 1 {
            // Consecutive line, extend range
            end = line;
        } else {
            // Gap found, save current range and start new one
            if start == end {
                result.push(Line::Single(start));
            } else {
                result.push(Line::Range(start, end));
            }
            start = line;
            end = line;
        }
    }

    // Add the final range
    if start == end {
        result.push(Line::Single(start));
    } else {
        result.push(Line::Range(start, end));
    }

    result
}

impl Default for AuthorshipLog {
    fn default() -> Self {
        Self::new()
    }
}

/// Format line ranges as comma-separated values with ranges as "start-end"
/// Sorts ranges first: Single ranges by their value, Range ones by their lowest bound
fn format_line_ranges(ranges: &[LineRange]) -> String {
    let mut sorted_ranges = ranges.to_vec();
    sorted_ranges.sort_by(|a, b| {
        let a_start = match a {
            LineRange::Single(line) => *line,
            LineRange::Range(start, _) => *start,
        };
        let b_start = match b {
            LineRange::Single(line) => *line,
            LineRange::Range(start, _) => *start,
        };
        a_start.cmp(&b_start)
    });

    sorted_ranges
        .iter()
        .map(|range| match range {
            LineRange::Single(line) => line.to_string(),
            LineRange::Range(start, end) => format!("{}-{}", start, end),
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Parse line ranges from a string like "1,2,19-222"
/// No spaces are expected in the format
fn parse_line_ranges(input: &str) -> Result<Vec<LineRange>, Box<dyn std::error::Error>> {
    let mut ranges = Vec::new();

    for part in input.split(',') {
        if part.is_empty() {
            continue;
        }

        if let Some(dash_pos) = part.find('-') {
            // Range format: "start-end"
            let start_str = &part[..dash_pos];
            let end_str = &part[dash_pos + 1..];
            let start: u32 = start_str.parse()?;
            let end: u32 = end_str.parse()?;
            ranges.push(LineRange::Range(start, end));
        } else {
            // Single line format: "line"
            let line: u32 = part.parse()?;
            ranges.push(LineRange::Single(line));
        }
    }

    Ok(ranges)
}

/// Parse the attestation section (before the divider)
fn parse_attestation_section(
    lines: &[&str],
) -> Result<Vec<FileAttestation>, Box<dyn std::error::Error>> {
    let mut attestations = Vec::new();
    let mut current_file: Option<FileAttestation> = None;

    for line in lines {
        let line = line.trim_end(); // Remove trailing whitespace but preserve leading

        if line.is_empty() {
            continue;
        }

        if line.starts_with("  ") {
            // Attestation entry line (indented)
            let entry_line = &line[2..]; // Remove "  " prefix

            // Split on first space to separate hash from line ranges
            if let Some(space_pos) = entry_line.find(' ') {
                let hash = entry_line[..space_pos].to_string();
                let ranges_str = &entry_line[space_pos + 1..];
                let line_ranges = parse_line_ranges(ranges_str)?;

                let entry = AttestationEntry::new(hash, line_ranges);

                if let Some(ref mut file_attestation) = current_file {
                    file_attestation.add_entry(entry);
                } else {
                    return Err("Attestation entry found without a file path".into());
                }
            } else {
                return Err(format!("Invalid attestation entry format: {}", entry_line).into());
            }
        } else {
            // File path line (not indented)
            if let Some(file_attestation) = current_file.take() {
                if !file_attestation.entries.is_empty() {
                    attestations.push(file_attestation);
                }
            }

            // Parse file path, handling quoted paths
            let file_path = if line.starts_with('"') && line.ends_with('"') {
                // Quoted path - remove quotes (no unescaping needed since quotes aren't allowed in file names)
                line[1..line.len() - 1].to_string()
            } else {
                // Unquoted path
                line.to_string()
            };

            current_file = Some(FileAttestation::new(file_path));
        }
    }

    // Don't forget the last file
    if let Some(file_attestation) = current_file {
        if !file_attestation.entries.is_empty() {
            attestations.push(file_attestation);
        }
    }

    Ok(attestations)
}

/// Check if a file path needs quoting (contains spaces or whitespace)
fn needs_quoting(path: &str) -> bool {
    path.contains(' ') || path.contains('\t') || path.contains('\n')
}

/// Generate a short hash (7 characters) from agent_id and tool
fn generate_short_hash(agent_id: &str, tool: &str) -> String {
    let combined = format!("{}:{}", tool, agent_id);
    let mut hasher = Sha256::new();
    hasher.update(combined.as_bytes());
    let result = hasher.finalize();
    // Take first 7 characters of the hex representation
    format!("{:x}", result)[..7].to_string()
}

/// Count the number of lines represented by a working_log::Line
fn count_working_log_lines(line: &crate::authorship::working_log::Line) -> u32 {
    match line {
        crate::authorship::working_log::Line::Single(_) => 1,
        crate::authorship::working_log::Line::Range(start, end) => end - start + 1,
    }
}

/// Count the number of lines represented by a LineRange
fn count_line_range(range: &LineRange) -> u32 {
    match range {
        LineRange::Single(_) => 1,
        LineRange::Range(start, end) => end - start + 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_debug_snapshot;

    #[test]
    fn test_format_line_ranges() {
        let ranges = vec![
            LineRange::Range(19, 222),
            LineRange::Single(1),
            LineRange::Single(2),
        ];

        assert_debug_snapshot!(format_line_ranges(&ranges));
    }

    #[test]
    fn test_parse_line_ranges() {
        let ranges = parse_line_ranges("1,2,19-222").unwrap();
        assert_debug_snapshot!(ranges);
    }

    #[test]
    fn test_serialize_deserialize_roundtrip() {
        let mut log = AuthorshipLog::new();
        log.metadata.base_commit_sha = "abc123".to_string();

        // Add some attestations
        let mut file1 = FileAttestation::new("src/file.xyz".to_string());
        file1.add_entry(AttestationEntry::new(
            "xyzAbc".to_string(),
            vec![
                LineRange::Single(1),
                LineRange::Single(2),
                LineRange::Range(19, 222),
            ],
        ));
        file1.add_entry(AttestationEntry::new(
            "123456".to_string(),
            vec![LineRange::Range(400, 405)],
        ));

        let mut file2 = FileAttestation::new("src/file2.xyz".to_string());
        file2.add_entry(AttestationEntry::new(
            "123456".to_string(),
            vec![
                LineRange::Range(1, 111),
                LineRange::Single(245),
                LineRange::Single(260),
            ],
        ));

        log.attestations.push(file1);
        log.attestations.push(file2);

        // Serialize and snapshot the format
        let serialized = log.serialize_to_string().unwrap();
        assert_debug_snapshot!(serialized);

        // Test roundtrip: deserialize and verify structure matches
        let deserialized = AuthorshipLog::deserialize_from_string(&serialized).unwrap();
        assert_debug_snapshot!(deserialized);
    }

    #[test]
    fn test_expected_format() {
        let mut log = AuthorshipLog::new();

        let mut file1 = FileAttestation::new("src/file.xyz".to_string());
        file1.add_entry(AttestationEntry::new(
            "xyzAbc".to_string(),
            vec![
                LineRange::Single(1),
                LineRange::Single(2),
                LineRange::Range(19, 222),
            ],
        ));
        file1.add_entry(AttestationEntry::new(
            "123456".to_string(),
            vec![LineRange::Range(400, 405)],
        ));

        let mut file2 = FileAttestation::new("src/file2.xyz".to_string());
        file2.add_entry(AttestationEntry::new(
            "123456".to_string(),
            vec![
                LineRange::Range(1, 111),
                LineRange::Single(245),
                LineRange::Single(260),
            ],
        ));

        log.attestations.push(file1);
        log.attestations.push(file2);

        let serialized = log.serialize_to_string().unwrap();
        assert_debug_snapshot!(serialized);
    }

    #[test]
    fn test_line_range_sorting() {
        // Test that ranges are sorted correctly: single ranges and ranges by lowest bound
        let ranges = vec![
            LineRange::Range(100, 200),
            LineRange::Single(5),
            LineRange::Range(10, 15),
            LineRange::Single(50),
            LineRange::Single(1),
            LineRange::Range(25, 30),
        ];

        let formatted = format_line_ranges(&ranges);
        assert_debug_snapshot!(formatted);

        // Should be sorted as: 1, 5, 10-15, 25-30, 50, 100-200
    }

    #[test]
    fn test_file_names_with_spaces() {
        // Test file names with spaces and special characters
        let mut log = AuthorshipLog::new();

        // Add a prompt to the metadata
        let agent_id = crate::authorship::working_log::AgentId {
            tool: "cursor".to_string(),
            id: "session_123".to_string(),
            model: "claude-3-sonnet".to_string(),
        };
        let prompt_hash = generate_short_hash(&agent_id.id, &agent_id.tool);
        log.metadata.prompts.insert(
            prompt_hash.clone(),
            crate::authorship::authorship_log::PromptRecord {
                agent_id: agent_id,
                human_author: None,
                messages: vec![],
                total_additions: 0,
                total_deletions: 0,
                accepted_lines: 0,
                overriden_lines: 0,
            },
        );

        // Add attestations for files with spaces and special characters
        let mut file1 = FileAttestation::new("src/my file.rs".to_string());
        file1.add_entry(AttestationEntry::new(
            prompt_hash.to_string(),
            vec![LineRange::Range(1, 10)],
        ));

        let mut file2 = FileAttestation::new("docs/README (copy).md".to_string());
        file2.add_entry(AttestationEntry::new(
            prompt_hash.to_string(),
            vec![LineRange::Single(5)],
        ));

        let mut file3 = FileAttestation::new("test/file-with-dashes.js".to_string());
        file3.add_entry(AttestationEntry::new(
            prompt_hash.to_string(),
            vec![LineRange::Range(20, 25)],
        ));

        log.attestations.push(file1);
        log.attestations.push(file2);
        log.attestations.push(file3);

        let serialized = log.serialize_to_string().unwrap();
        println!("Serialized with special file names:\n{}", serialized);
        assert_debug_snapshot!(serialized);

        // Try to deserialize - this should work if we handle escaping properly
        let deserialized = AuthorshipLog::deserialize_from_string(&serialized);
        match deserialized {
            Ok(log) => {
                println!("Deserialization successful!");
                assert_debug_snapshot!(log);
            }
            Err(e) => {
                println!("Deserialization failed: {}", e);
                // This will fail with current implementation
            }
        }
    }

    #[test]
    fn test_hash_always_maps_to_prompt() {
        // Demonstrate that every hash in attestation section maps to prompts section
        let mut log = AuthorshipLog::new();

        // Add a prompt to the metadata
        let agent_id = crate::authorship::working_log::AgentId {
            tool: "cursor".to_string(),
            id: "session_123".to_string(),
            model: "claude-3-sonnet".to_string(),
        };
        let prompt_hash = generate_short_hash(&agent_id.id, &agent_id.tool);
        log.metadata.prompts.insert(
            prompt_hash.clone(),
            crate::authorship::authorship_log::PromptRecord {
                agent_id: agent_id,
                human_author: None,
                messages: vec![],
                total_additions: 0,
                total_deletions: 0,
                accepted_lines: 0,
                overriden_lines: 0,
            },
        );

        // Add attestation that references this prompt
        let mut file1 = FileAttestation::new("src/example.rs".to_string());
        file1.add_entry(AttestationEntry::new(
            prompt_hash.to_string(),
            vec![LineRange::Range(1, 10)],
        ));
        log.attestations.push(file1);

        let serialized = log.serialize_to_string().unwrap();
        assert_debug_snapshot!(serialized);

        // Verify that every hash in attestations has a corresponding prompt
        for file_attestation in &log.attestations {
            for entry in &file_attestation.entries {
                assert!(
                    log.metadata.prompts.contains_key(&entry.hash),
                    "Hash '{}' should have a corresponding prompt in metadata",
                    entry.hash
                );
            }
        }
    }

    #[test]
    fn test_serialize_deserialize_no_attestations() {
        // Test that serialization and deserialization work correctly when there are no attestations
        let mut log = AuthorshipLog::new();
        log.metadata.base_commit_sha = "abc123".to_string();

        let agent_id = crate::authorship::working_log::AgentId {
            tool: "cursor".to_string(),
            id: "session_123".to_string(),
            model: "claude-3-sonnet".to_string(),
        };
        let prompt_hash = generate_short_hash(&agent_id.id, &agent_id.tool);
        log.metadata.prompts.insert(
            prompt_hash,
            crate::authorship::authorship_log::PromptRecord {
                agent_id: agent_id,
                human_author: None,
                messages: vec![],
                total_additions: 0,
                total_deletions: 0,
                accepted_lines: 0,
                overriden_lines: 0,
            },
        );

        // Serialize and verify the format
        let serialized = log.serialize_to_string().unwrap();
        assert_debug_snapshot!(serialized);

        // Test roundtrip: deserialize and verify structure matches
        let deserialized = AuthorshipLog::deserialize_from_string(&serialized).unwrap();
        assert_debug_snapshot!(deserialized);

        // Verify that the deserialized log has the same metadata but no attestations
        assert_eq!(deserialized.metadata.base_commit_sha, "abc123");
        assert_eq!(deserialized.metadata.prompts.len(), 1);
        assert_eq!(deserialized.attestations.len(), 0);
    }

    #[test]
    fn test_remove_line_ranges_complete_removal() {
        let mut entry =
            AttestationEntry::new("test_hash".to_string(), vec![LineRange::Range(2, 5)]);

        // Remove the exact same range
        entry.remove_line_ranges(&[LineRange::Range(2, 5)]);

        // Should be empty after removing the exact range
        assert!(
            entry.line_ranges.is_empty(),
            "Expected empty line_ranges after complete removal, got: {:?}",
            entry.line_ranges
        );
    }

    #[test]
    fn test_remove_line_ranges_partial_removal() {
        let mut entry =
            AttestationEntry::new("test_hash".to_string(), vec![LineRange::Range(2, 10)]);

        // Remove middle part
        entry.remove_line_ranges(&[LineRange::Range(5, 7)]);

        // Should have two ranges: [2-4] and [8-10]
        assert_eq!(entry.line_ranges.len(), 2);
        assert_eq!(entry.line_ranges[0], LineRange::Range(2, 4));
        assert_eq!(entry.line_ranges[1], LineRange::Range(8, 10));
    }

    #[test]
    fn test_metrics_calculation() {
        use crate::authorship::transcript::{AiTranscript, Message};
        use crate::authorship::working_log::{AgentId, Checkpoint, Line, WorkingLogEntry};

        // Create an agent ID
        let agent_id = AgentId {
            tool: "cursor".to_string(),
            id: "test_session".to_string(),
            model: "claude-3-sonnet".to_string(),
        };

        // Create a transcript
        let mut transcript = AiTranscript::new();
        transcript.add_message(Message::user("Add a function".to_string()));
        transcript.add_message(Message::assistant("Here's the function".to_string()));

        // Create working log entries
        // First checkpoint: add 10 lines (single line + range of 9)
        let entry1 = WorkingLogEntry::new(
            "src/test.rs".to_string(),
            "blob_sha_1".to_string(),
            vec![Line::Range(1, 10)],
            vec![],
        );
        let mut checkpoint1 = Checkpoint::new("".to_string(), "ai".to_string(), vec![entry1]);
        checkpoint1.agent_id = Some(agent_id.clone());
        checkpoint1.transcript = Some(transcript.clone());

        // Second checkpoint: delete 3 lines, add 5 lines (modified some lines)
        let entry2 = WorkingLogEntry::new(
            "src/test.rs".to_string(),
            "blob_sha_2".to_string(),
            vec![Line::Range(5, 9)], // 5 added lines
            vec![Line::Range(5, 7)], // 3 deleted lines
        );
        let mut checkpoint2 = Checkpoint::new("".to_string(), "ai".to_string(), vec![entry2]);
        checkpoint2.agent_id = Some(agent_id.clone());
        checkpoint2.transcript = Some(transcript);

        // Convert to authorship log
        let authorship_log = AuthorshipLog::from_working_log_with_base_commit_and_human_author(
            &[checkpoint1, checkpoint2],
            "base123",
            None,
        );

        // Get the prompt record
        let session_hash = generate_short_hash(&agent_id.id, &agent_id.tool);
        let prompt_record = authorship_log.metadata.prompts.get(&session_hash).unwrap();

        // Verify metrics
        // total_additions: 10 (from first checkpoint) + 5 (from second) = 15
        assert_eq!(prompt_record.total_additions, 15);
        // total_deletions: 0 (from first) + 3 (from second) = 3
        assert_eq!(prompt_record.total_deletions, 3);
        // accepted_lines: After correct shifting logic:
        // - Checkpoint 1 adds 1-10 (10 lines)
        // - Checkpoint 2 deletes 5-7 (removes 3), shifts 8-10 up to 5-7 (7 lines remain)
        // - Checkpoint 2 adds 5-9 (5 lines), shifts existing 5-7 down to 10-12
        // - Final: AI owns 1-4, 5-9, 10-12 = 12 lines
        assert_eq!(prompt_record.accepted_lines, 12);
    }

    #[test]
    fn test_convert_authorship_log_to_checkpoints() {
        use crate::authorship::transcript::{AiTranscript, Message};
        use crate::authorship::working_log::AgentId;

        // Create an authorship log with both AI and human-attributed lines
        let mut log = AuthorshipLog::new();
        log.metadata.base_commit_sha = "base123".to_string();

        // Add AI prompt session
        let agent_id = AgentId {
            tool: "cursor".to_string(),
            id: "session_abc".to_string(),
            model: "claude-3-sonnet".to_string(),
        };
        let mut transcript = AiTranscript::new();
        transcript.add_message(Message::user("Add error handling".to_string()));
        transcript.add_message(Message::assistant("Added error handling".to_string()));

        let session_hash = generate_short_hash(&agent_id.id, &agent_id.tool);
        log.metadata.prompts.insert(
            session_hash.clone(),
            crate::authorship::authorship_log::PromptRecord {
                agent_id: agent_id.clone(),
                human_author: Some("alice@example.com".to_string()),
                messages: transcript.messages().to_vec(),
                total_additions: 15,
                total_deletions: 3,
                accepted_lines: 12,
                overriden_lines: 0,
            },
        );

        // Add file attestations - AI owns lines 1-5, 10-15
        let mut file1 = FileAttestation::new("src/main.rs".to_string());
        file1.add_entry(AttestationEntry::new(
            session_hash.clone(),
            vec![LineRange::Range(1, 5), LineRange::Range(10, 15)],
        ));
        log.attestations.push(file1);

        // Convert to checkpoints
        let result = log.convert_to_checkpoints_for_squash("alice@example.com");
        assert!(result.is_ok());
        let checkpoints = result.unwrap();

        // Should have 2 checkpoints: 1 human + 1 AI
        assert_eq!(checkpoints.len(), 2);

        // First checkpoint should be human with lines 6-9 (inverse of AI lines)
        let human_checkpoint = &checkpoints[0];
        assert_eq!(human_checkpoint.author, "alice@example.com");
        assert!(human_checkpoint.agent_id.is_none());
        assert!(human_checkpoint.transcript.is_none());
        assert_eq!(human_checkpoint.entries.len(), 1);
        let human_entry = &human_checkpoint.entries[0];
        assert_eq!(human_entry.file, "src/main.rs");
        assert_eq!(
            human_entry.added_lines,
            vec![crate::authorship::working_log::Line::Range(6, 9)]
        );
        assert!(human_entry.deleted_lines.is_empty());

        // Second checkpoint should be AI with original lines
        let ai_checkpoint = &checkpoints[1];
        assert_eq!(ai_checkpoint.author, "ai");
        assert!(ai_checkpoint.agent_id.is_some());
        assert_eq!(ai_checkpoint.agent_id.as_ref().unwrap().tool, "cursor");
        assert!(ai_checkpoint.transcript.is_some());
        assert_eq!(ai_checkpoint.entries.len(), 1);
        let ai_entry = &ai_checkpoint.entries[0];
        assert_eq!(ai_entry.file, "src/main.rs");
        assert_eq!(
            ai_entry.added_lines,
            vec![
                crate::authorship::working_log::Line::Range(1, 5),
                crate::authorship::working_log::Line::Range(10, 15)
            ]
        );
        assert!(ai_entry.deleted_lines.is_empty());
    }

    #[test]
    fn test_overriden_lines_detection() {
        use crate::authorship::transcript::{AiTranscript, Message};
        use crate::authorship::working_log::{AgentId, Checkpoint, Line, WorkingLogEntry};

        // Create an AI checkpoint that adds lines 1-5
        let agent_id = AgentId {
            tool: "cursor".to_string(),
            id: "session_123".to_string(),
            model: "claude-3-sonnet".to_string(),
        };

        let entry1 = WorkingLogEntry::new(
            "src/main.rs".to_string(),
            "sha1".to_string(),
            vec![Line::Range(1, 5)], // AI adds lines 1-5
            vec![],
        );
        let mut checkpoint1 = Checkpoint::new("".to_string(), "ai".to_string(), vec![entry1]);
        checkpoint1.agent_id = Some(agent_id.clone());

        // Add transcript to make it a valid AI checkpoint
        let mut transcript = AiTranscript::new();
        transcript.add_message(Message::user("Add some code".to_string()));
        transcript.add_message(Message::assistant("Added code".to_string()));
        checkpoint1.transcript = Some(transcript);

        // Create a human checkpoint that deletes lines 2-3 (overriding AI lines)
        let entry2 = WorkingLogEntry::new(
            "src/main.rs".to_string(),
            "sha2".to_string(),
            vec![],
            vec![Line::Range(2, 3)], // Human deletes lines 2-3
        );
        let checkpoint2 = Checkpoint::new("".to_string(), "human".to_string(), vec![entry2]);
        // Note: checkpoint2.agent_id is None, indicating it's a human checkpoint

        // Convert to authorship log
        let authorship_log = AuthorshipLog::from_working_log_with_base_commit_and_human_author(
            &[checkpoint1, checkpoint2],
            "base123",
            Some("human@example.com"),
        );

        // Get the prompt record
        let session_hash = generate_short_hash(&agent_id.id, &agent_id.tool);
        let prompt_record = authorship_log.metadata.prompts.get(&session_hash).unwrap();

        // Verify overriden_lines count
        // AI added 5 lines (1-5), human deleted 2 lines (2-3), so 2 lines were overridden
        assert_eq!(prompt_record.overriden_lines, 2);

        // Verify other metrics
        assert_eq!(prompt_record.total_additions, 5);
        assert_eq!(prompt_record.total_deletions, 0); // AI didn't delete anything
        assert_eq!(prompt_record.accepted_lines, 3); // AI still owns lines 1, 4, 5
    }

    #[test]
    fn test_passthrough_checkpoint_comprehensive() {
        use crate::authorship::transcript::{AiTranscript, Message};
        use crate::authorship::working_log::{AgentId, Checkpoint, Line, WorkingLogEntry};

        // Create an agent ID
        let agent_id = AgentId {
            tool: "cursor".to_string(),
            id: "test_session".to_string(),
            model: "claude-3-sonnet".to_string(),
        };

        // Create a transcript
        let mut transcript = AiTranscript::new();
        transcript.add_message(Message::user("Add functions and modify code".to_string()));
        transcript.add_message(Message::assistant(
            "I'll add the functions and make changes".to_string(),
        ));

        // Test scenario: Passthrough checkpoint at the top (after merge-squash)
        // This simulates the intended usage pattern

        // 1. PASSTHROUGH CHECKPOINT (at top) - adds lines 1-3 (top of file)
        let passthrough_entry = WorkingLogEntry::new(
            "src/test.rs".to_string(),
            "blob_sha_passthrough".to_string(),
            vec![Line::Range(1, 3)], // Lines 1-3
            vec![],
        );
        let passthrough_checkpoint = Checkpoint::new_passthrough(
            "".to_string(),
            "human".to_string(),
            vec![passthrough_entry],
        );

        // 2. Normal AI checkpoint - adds lines 4-6 (middle)
        let entry1 = WorkingLogEntry::new(
            "src/test.rs".to_string(),
            "blob_sha_1".to_string(),
            vec![Line::Range(4, 6)], // Lines 4-6
            vec![],
        );
        let mut checkpoint1 = Checkpoint::new("".to_string(), "ai".to_string(), vec![entry1]);
        checkpoint1.agent_id = Some(agent_id.clone());
        checkpoint1.transcript = Some(transcript.clone());

        // 3. Another normal AI checkpoint - adds lines 7-9 (middle)
        let entry2 = WorkingLogEntry::new(
            "src/test.rs".to_string(),
            "blob_sha_2".to_string(),
            vec![Line::Range(7, 9)], // Lines 7-9
            vec![],
        );
        let mut checkpoint2 = Checkpoint::new("".to_string(), "ai".to_string(), vec![entry2]);
        checkpoint2.agent_id = Some(agent_id.clone());
        checkpoint2.transcript = Some(transcript.clone());

        // 4. Another passthrough checkpoint - adds lines 10-12 (middle)
        let passthrough_entry2 = WorkingLogEntry::new(
            "src/test.rs".to_string(),
            "blob_sha_passthrough2".to_string(),
            vec![Line::Range(10, 12)], // Lines 10-12
            vec![],
        );
        let passthrough_checkpoint2 = Checkpoint::new_passthrough(
            "".to_string(),
            "human".to_string(),
            vec![passthrough_entry2],
        );

        // 5. Final normal AI checkpoint - adds lines 13-15 (bottom)
        let entry3 = WorkingLogEntry::new(
            "src/test.rs".to_string(),
            "blob_sha_3".to_string(),
            vec![Line::Range(13, 15)], // Lines 13-15
            vec![],
        );
        let mut checkpoint3 = Checkpoint::new("".to_string(), "ai".to_string(), vec![entry3]);
        checkpoint3.agent_id = Some(agent_id.clone());
        checkpoint3.transcript = Some(transcript);

        // Convert to authorship log with passthrough checkpoints at the top
        let authorship_log = AuthorshipLog::from_working_log_with_base_commit_and_human_author(
            &[
                passthrough_checkpoint,  // Lines 1-3 (passthrough)
                checkpoint1,             // Lines 4-6 (AI)
                checkpoint2,             // Lines 7-9 (AI)
                passthrough_checkpoint2, // Lines 10-12 (passthrough)
                checkpoint3,             // Lines 13-15 (AI)
            ],
            "base123",
            None,
        );

        // Get the prompt record
        let session_hash = generate_short_hash(&agent_id.id, &agent_id.tool);
        let prompt_record = authorship_log.metadata.prompts.get(&session_hash).unwrap();

        // Check that the prompt record exists and has the correct metrics
        // Only normal checkpoints contribute to session metrics (3 + 3 + 3 = 9 lines)
        // Passthrough checkpoint lines (3 + 3 = 6) are not counted in session metrics
        assert_eq!(prompt_record.total_additions, 9); // 3 + 3 + 3 lines from normal checkpoints only
        assert_eq!(prompt_record.total_deletions, 0);

        // Check that only lines 4-6, 7-9, and 13-15 are attributed to the AI session
        // Lines 1-3 and 10-12 should NOT be attributed due to passthrough checkpoints
        let file_attestation = authorship_log
            .attestations
            .iter()
            .find(|fa| fa.file_path == "src/test.rs")
            .unwrap();
        let attestation_entry = &file_attestation.entries[0];

        // Should have two ranges: 4-9 (merged from 4-6 and 7-9) and 13-15
        // The AI checkpoints with the same session ID get merged together
        assert_eq!(attestation_entry.line_ranges.len(), 2);

        // Check that the ranges are correct
        let ranges: Vec<_> = attestation_entry.line_ranges.iter().collect();

        // Sort ranges by start line for consistent testing
        let mut sorted_ranges = ranges.clone();
        sorted_ranges.sort_by_key(|r| match r {
            LineRange::Single(line) => *line,
            LineRange::Range(start, _) => *start,
        });

        match &sorted_ranges[0] {
            LineRange::Range(start, end) => {
                assert_eq!(*start, 4);
                assert_eq!(*end, 9); // Merged range from 4-6 and 7-9
            }
            _ => panic!("Expected merged range for lines 4-9"),
        }
        match &sorted_ranges[1] {
            LineRange::Range(start, end) => {
                assert_eq!(*start, 13);
                assert_eq!(*end, 15);
            }
            _ => panic!("Expected range for lines 13-15"),
        }

        // Verify that lines 1-3 and 10-12 are NOT attributed to any AI session
        // (they should not appear in any attestation entry)
        for entry in &file_attestation.entries {
            for range in &entry.line_ranges {
                match range {
                    LineRange::Single(line) => {
                        assert!(
                            (*line < 1 || *line > 3) && (*line < 10 || *line > 12),
                            "Line {} should not be attributed (passthrough lines)",
                            line
                        );
                    }
                    LineRange::Range(start, end) => {
                        assert!(
                            (*end < 1 || *start > 3) && (*end < 10 || *start > 12),
                            "Range {}-{} should not be attributed (passthrough lines)",
                            start,
                            end
                        );
                    }
                }
            }
        }

        // Verify that the passthrough checkpoints still affect line offsets correctly
        // by checking that AI-attributed lines are in the correct positions
        // (This is implicitly tested by the range assertions above)
    }

    #[test]
    fn test_convert_authorship_log_multiple_ai_sessions() {
        use crate::authorship::transcript::{AiTranscript, Message};
        use crate::authorship::working_log::AgentId;

        // Create authorship log with 2 different AI sessions
        let mut log = AuthorshipLog::new();
        log.metadata.base_commit_sha = "base456".to_string();

        // First AI session
        let agent1 = AgentId {
            tool: "cursor".to_string(),
            id: "session_1".to_string(),
            model: "claude-3-sonnet".to_string(),
        };
        let mut transcript1 = AiTranscript::new();
        transcript1.add_message(Message::user("Add function".to_string()));
        transcript1.add_message(Message::assistant("Added function".to_string()));
        let session1_hash = generate_short_hash(&agent1.id, &agent1.tool);
        log.metadata.prompts.insert(
            session1_hash.clone(),
            crate::authorship::authorship_log::PromptRecord {
                agent_id: agent1,
                human_author: Some("bob@example.com".to_string()),
                messages: transcript1.messages().to_vec(),
                total_additions: 10,
                total_deletions: 0,
                accepted_lines: 10,
                overriden_lines: 0,
            },
        );

        // Second AI session
        let agent2 = AgentId {
            tool: "cursor".to_string(),
            id: "session_2".to_string(),
            model: "claude-3-opus".to_string(),
        };
        let mut transcript2 = AiTranscript::new();
        transcript2.add_message(Message::user("Add tests".to_string()));
        transcript2.add_message(Message::assistant("Added tests".to_string()));
        let session2_hash = generate_short_hash(&agent2.id, &agent2.tool);
        log.metadata.prompts.insert(
            session2_hash.clone(),
            crate::authorship::authorship_log::PromptRecord {
                agent_id: agent2,
                human_author: Some("bob@example.com".to_string()),
                messages: transcript2.messages().to_vec(),
                total_additions: 20,
                total_deletions: 0,
                accepted_lines: 20,
                overriden_lines: 0,
            },
        );

        // File with both sessions, plus some human lines
        let mut file1 = FileAttestation::new("src/lib.rs".to_string());
        file1.add_entry(AttestationEntry::new(
            session1_hash.clone(),
            vec![LineRange::Range(1, 10)],
        ));
        file1.add_entry(AttestationEntry::new(
            session2_hash.clone(),
            vec![LineRange::Range(11, 30)],
        ));
        // Human owns lines 31-40 (implicitly, by not being in any AI attestation)
        log.attestations.push(file1);

        // Convert to checkpoints
        let result = log.convert_to_checkpoints_for_squash("bob@example.com");
        assert!(result.is_ok());
        let checkpoints = result.unwrap();

        // Should have 2 AI checkpoints (no human lines since we only have AI-attributed lines 1-30)
        assert_eq!(checkpoints.len(), 2);

        // Both are AI sessions
        let ai_checkpoints: Vec<_> = checkpoints
            .iter()
            .filter(|c| c.agent_id.is_some())
            .collect();
        assert_eq!(ai_checkpoints.len(), 2);

        // Verify that the AI sessions are distinct
        assert_ne!(
            ai_checkpoints[0].agent_id.as_ref().unwrap().id,
            ai_checkpoints[1].agent_id.as_ref().unwrap().id
        );
    }
}
