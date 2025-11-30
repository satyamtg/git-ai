use std::{ops::Add, time::Duration};

use serde_json::json;

use crate::{
    authorship::working_log::CheckpointKind,
    observability::log_performance,
    utils::{debug_performance_log, debug_performance_log_structured},
};

pub const PERFORMANCE_FLOOR_MS: Duration = Duration::from_millis(270);

/// Performance benchmark result containing timing breakdowns
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct BenchmarkResult {
    pub total_duration: Duration,
    pub git_duration: Duration,
    pub post_command_duration: Duration,
    pub pre_command_duration: Duration,
}

pub fn log_performance_target_if_violated(
    command: &str,
    pre_command: Duration,
    git_duration: Duration,
    post_command: Duration,
) {
    let total_duration = pre_command + git_duration + post_command;
    let git_ai_overhead = pre_command + post_command;
    let within_target: bool = match command {
        "commit" => {
            git_duration.mul_f32(1.1) >= total_duration || git_ai_overhead < PERFORMANCE_FLOOR_MS
        }
        "rebase" => {
            git_duration.mul_f32(1.1) >= total_duration || git_ai_overhead < PERFORMANCE_FLOOR_MS
        }
        "cherry-pick" => {
            git_duration.mul_f32(1.1) >= total_duration || git_ai_overhead < PERFORMANCE_FLOOR_MS
        }
        "reset" => {
            git_duration.mul_f32(1.1) >= total_duration || git_ai_overhead < PERFORMANCE_FLOOR_MS
        }
        "fetch" => {
            git_duration.mul_f32(1.5) >= total_duration || git_ai_overhead < PERFORMANCE_FLOOR_MS
        }
        "pull" => {
            git_duration.mul_f32(1.5) >= total_duration || git_ai_overhead < PERFORMANCE_FLOOR_MS
        }
        "push" => {
            git_duration.mul_f32(1.5) >= total_duration || git_ai_overhead < PERFORMANCE_FLOOR_MS
        }
        _ => git_duration.add(PERFORMANCE_FLOOR_MS) >= total_duration,
    };

    let perf_json = json!({
        "command": command,
        "total_duration_ms": total_duration.as_millis(),
        "git_duration_ms": git_duration.as_millis(),
        "pre_command_duration_ms": pre_command.as_millis(),
        "post_command_duration_ms": post_command.as_millis(),
        "within_target": within_target,
    });

    debug_performance_log_structured(perf_json);

    if !within_target {
        debug_performance_log(&format!(
            "ᕽ Performance target violated for command: {}. Total duration: {}ms, Git duration: {}ms. Pre-command: {}ms, Post-command: {}ms.",
            command,
            total_duration.as_millis(),
            git_duration.as_millis(),
            pre_command.as_millis(),
            post_command.as_millis(),
        ));
        log_performance(
            "performance_target_violated",
            total_duration,
            Some(json!({
                "command": command,
                "total_duration": total_duration.as_millis(),
                "git_duration": git_duration.as_millis(),
                "pre_command": pre_command.as_millis(),
                "post_command": post_command.as_millis(),
            })),
        );
    } else {
        debug_performance_log(&format!(
            "✓ Performance target met for command: {}. Total duration: {}ms, Git duration: {}ms",
            command,
            total_duration.as_millis(),
            git_duration.as_millis(),
        ));
    }
}

pub fn log_performance_for_checkpoint(
    files_edited: usize,
    duration: Duration,
    checkpoint_kind: CheckpointKind,
) {
    if Duration::from_millis(50 * files_edited as u64) >= duration {
        log_performance(
            "checkpoint",
            duration,
            Some(json!({
                "files_edited": files_edited,
                "checkpoint_kind": checkpoint_kind.to_string(),
                "duration": duration.as_millis(),
            })),
        );

        debug_performance_log(&format!(
            "ᕽ Performance target violated for checkpoint: {}. Total duration. Files edited: {}",
            duration.as_millis(),
            files_edited,
        ));
    } else {
        debug_performance_log(&format!(
            "✓ Performance target met for checkpoint: {}. Total duration. Files edited: {}",
            duration.as_millis(),
            files_edited,
        ));
    }
}
