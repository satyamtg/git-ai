use serde::Deserialize;

/// Feature flags for the application
#[derive(Debug, Clone)]
pub struct FeatureFlags {
    pub rewrite_stash: bool,
    pub inter_commit_move: bool,
}

impl Default for FeatureFlags {
    fn default() -> Self {
        #[cfg(debug_assertions)]
        {
            return FeatureFlags {
                rewrite_stash: true,
                inter_commit_move: false,
            };
        }
        #[cfg(not(debug_assertions))]
        FeatureFlags {
            rewrite_stash: false,
            inter_commit_move: false,
        }
    }
}

/// Deserializable version of FeatureFlags with all optional fields
/// and unknown fields allowed for graceful degradation
#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct FileFeatureFlags {
    #[serde(default, rename = "rewrite.stash")]
    rewrite_stash: Option<bool>,
    #[serde(default, rename = "checkpoint.inter_commit_move")]
    inter_commit_move: Option<bool>,
}

impl FeatureFlags {
    /// Build FeatureFlags from file configuration
    /// Falls back to defaults for any invalid or missing values
    pub(crate) fn from_file_config(file_flags: Option<FileFeatureFlags>) -> Self {
        let file_flags = match file_flags {
            Some(flags) => flags,
            None => return FeatureFlags::default(),
        };

        let defaults = FeatureFlags::default();

        FeatureFlags {
            rewrite_stash: file_flags.rewrite_stash.unwrap_or(defaults.rewrite_stash),
            inter_commit_move: file_flags
                .inter_commit_move
                .unwrap_or(defaults.inter_commit_move),
        }
    }
}
