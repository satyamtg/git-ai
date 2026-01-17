# Contributing to Git AI

Thank you for your interest in contributing to `git-ai`. This is a cool moment for the industry and we're all here to build ~~a~~ the standard for tracking AI code. 

## Getting Started

### Prerequisites

- Rust https://rustup.rs/ (compiler and tooling)
- Taskfile https://taskfile.dev/ (modern make)

### Development Setup

1. **Fork the repository** on GitHub

2. **Clone your fork**:
   ```bash
   git clone https://github.com/YOUR_USERNAME/git-ai.git
   cd git-ai
   ```

3. **Build the project**:
   ```bash
   cargo build
   ```

4. **Run the tests**:
   ```bash
   cargo test
   ```

### (Option 1) Using debug commands with symlinks (Recommended)

The recommended way to test your changes is to use the debug commands (`debug-git-ai` and `debug-git`). This approach doesn't interfere with any installed version of git-ai.

Build and create the debug symlinks:

```bash
cargo build                  # Build debug version
sh scripts/dev-symlinks.sh   # Create symlinks (no additional build needed)
```

Then add the debug commands to your PATH by adding this to your `~/.zshrc` or `~/.bashrc`:

```bash
export PATH="/path/to/git-ai/target/gitwrap/bin:$PATH"
```

After restarting your terminal, you can use:

```bash
debug-git-ai --version              # Test git-ai commands
debug-git-ai rebase-authorship --help
debug-git status                    # Test as git wrapper
```

**Why this is better:** The symlinks automatically point to your latest build, so just run `cargo build` after making changes - no reinstall needed!

### (Option 1b) Install debug build to ~/.local/bin

Alternatively, you can install the debug build to `~/.local/bin/git-ai`:

```bash
task debug:local   # Builds AND installs in one command
```

This installs as `git-ai` (not `debug-git-ai`) and replaces any existing installation in `~/.local/bin`. You'll need to run `task debug:local` again after each code change.

### (Option 2) Running with Cargo directly

You can run specific commands directly with cargo without creating symlinks:

```bash
GIT_AI=git-ai cargo run -- --version
GIT_AI=git-ai cargo run -- rebase-authorship --help
GIT_AI=git cargo run -- status     # Test as git wrapper
```

## Contributing Changes

### Before You Start

- **Check existing issues**: Look for related issues or feature requests
- **For new features or architectural changes**: We encourage you to chat with the core maintainers first to discuss your approach. This helps ensure your contribution aligns with the project's direction and saves you time.

### Submitting a Pull Request

1. Create a new branch for your changes:
   ```bash
   git checkout -b my-feature-branch
   ```

2. Make your changes and commit them with clear, descriptive messages

3. Push to your fork:
   ```bash
   git push origin my-feature-branch
   ```

4. Open a Pull Request against the main repository

5. **Reference any related issues** in your PR description (e.g., "Fixes #123" or "Related to #456")

6. Wait for review from the maintainers

## Code Style

The project uses standard Rust formatting. Please run `cargo fmt` before committing your changes.


## Getting Help

If you have questions about contributing, feel free to open an issue or reach out to the maintainers.

- **Discord**: [Link TBD]
- **Office Hours**: [Schedule TBD]
