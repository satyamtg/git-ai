#!/usr/bin/env bash
set -euo pipefail

# Replicates the steps in test_squash_merge_with_duplicate_lines using git and git-ai
# Initializes in the current directory and mimics set_contents behavior exactly.

if ! command -v git-ai >/dev/null 2>&1; then
  echo "git-ai not found in PATH" >&2
  exit 1
fi

# Init repo and basic identity
git init .
git config user.name "Test User"
git config user.email "test@example.com"

# helpers.rs: initial human-authored function via set_contents (with exact staging + checkpoint behavior)
# sleep 2
cat > helpers.rs <<'EOF'
pub fn format_string(s: &str) -> String {
    s.to_uppercase()
}
EOF
perl -i -0777 -pe 's/\n\z//' helpers.rs
git add -A
git-ai checkpoint

# sleep 2
# No AI lines here; set_contents still writes the same content and does an AI checkpoint
cat > helpers.rs <<'EOF'
pub fn format_string(s: &str) -> String {
    s.to_uppercase()
}
EOF
perl -i -0777 -pe 's/\n\z//' helpers.rs
git add -A
git-ai checkpoint mock_ai

# helpers.rs: add AI-authored second function via set_contents
# First pass: human checkpoint with AI lines replaced by the pending placeholder
# sleep 2
cat > helpers.rs <<'EOF'
pub fn format_string(s: &str) -> String {
    s.to_uppercase()
}
||__AI LINE__ PENDING__||
||__AI LINE__ PENDING__||
||__AI LINE__ PENDING__||
EOF
perl -i -0777 -pe 's/\n\z//' helpers.rs
git add -A
git-ai checkpoint

# Second pass: AI checkpoint with actual AI lines
# sleep 2
cat > helpers.rs <<'EOF'
pub fn format_string(s: &str) -> String {
    s.to_uppercase()
}
pub fn reverse_string(s: &str) -> String {
    s.chars().rev().collect()
}
EOF
perl -i -0777 -pe 's/\n\z//' helpers.rs
git add -A
git-ai checkpoint mock_ai

git add -A
git commit -m "initial commit"
