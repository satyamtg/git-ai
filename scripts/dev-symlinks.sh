#!/bin/bash

set -euo pipefail

# Parse arguments
BUILD_TYPE="debug"
if [[ "$#" -gt 0 && "$1" == "--release" ]]; then
    BUILD_TYPE="release"
fi

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

PROJECT_DIR=$(pwd)
GITWRAP_BIN="$PROJECT_DIR/target/gitwrap/bin"
TARGET_BINARY="$PROJECT_DIR/target/$BUILD_TYPE/git-ai"

mkdir -p "$GITWRAP_BIN"

# Check if symlinks already exist and point to correct location
SYMLINK_GIT="$GITWRAP_BIN/debug-git"
SYMLINK_GIT_AI="$GITWRAP_BIN/debug-git-ai"

NEEDS_UPDATE=false

if [ -L "$SYMLINK_GIT" ]; then
    CURRENT_TARGET=$(readlink "$SYMLINK_GIT")
    if [ "$CURRENT_TARGET" != "$TARGET_BINARY" ]; then
        echo -e "${YELLOW}⚠ debug-git symlink exists but points to wrong target${NC}"
        echo "  Current: $CURRENT_TARGET"
        echo "  Expected: $TARGET_BINARY"
        NEEDS_UPDATE=true
    fi
else
    NEEDS_UPDATE=true
fi

if [ -L "$SYMLINK_GIT_AI" ]; then
    CURRENT_TARGET=$(readlink "$SYMLINK_GIT_AI")
    if [ "$CURRENT_TARGET" != "$TARGET_BINARY" ]; then
        echo -e "${YELLOW}⚠ debug-git-ai symlink exists but points to wrong target${NC}"
        echo "  Current: $CURRENT_TARGET"
        echo "  Expected: $TARGET_BINARY"
        NEEDS_UPDATE=true
    fi
else
    NEEDS_UPDATE=true
fi

if [ "$NEEDS_UPDATE" = true ]; then
    echo -e "${BLUE}Creating/updating debug symlinks to target/$BUILD_TYPE${NC}"
    ln -sf "$TARGET_BINARY" "$SYMLINK_GIT"
    ln -sf "$TARGET_BINARY" "$SYMLINK_GIT_AI"
    echo ""
    echo -e "${GREEN}✓ Created/updated symlinks:${NC}"
    echo "  debug-git     → target/$BUILD_TYPE/git-ai (wrapper mode)"
    echo "  debug-git-ai  → target/$BUILD_TYPE/git-ai (direct mode)"
else
    echo -e "${GREEN}✓ Symlinks already exist and are correct:${NC}"
    echo "  debug-git     → target/$BUILD_TYPE/git-ai (wrapper mode)"
    echo "  debug-git-ai  → target/$BUILD_TYPE/git-ai (direct mode)"
fi
echo ""

# Detect shell config file
SHELL_CONFIG=""
if [ -f "$HOME/.zshrc" ]; then
    SHELL_CONFIG="$HOME/.zshrc"
elif [ -f "$HOME/.bashrc" ]; then
    SHELL_CONFIG="$HOME/.bashrc"
elif [ -f "$HOME/.bash_profile" ]; then
    SHELL_CONFIG="$HOME/.bash_profile"
fi

# Check if already in PATH
PATH_EXPORT="export PATH=\"$GITWRAP_BIN:\$PATH\""
ALREADY_IN_PATH=false

if [ -n "$SHELL_CONFIG" ] && [ -f "$SHELL_CONFIG" ]; then
    if grep -q "$GITWRAP_BIN" "$SHELL_CONFIG" 2>/dev/null; then
        ALREADY_IN_PATH=true
    fi
fi

# Offer to add to PATH
if [ "$ALREADY_IN_PATH" = true ]; then
    echo -e "${GREEN}✓ Already in PATH${NC} (found in $SHELL_CONFIG)"
    echo ""
    echo "You can use the commands immediately after reloading your shell:"
    echo -e "  ${YELLOW}source $SHELL_CONFIG${NC}"
elif [ -n "$SHELL_CONFIG" ]; then
    echo -e "${YELLOW}Add to PATH?${NC}"
    echo "Would you like to add debug commands to your PATH in $SHELL_CONFIG? (y/n)"
    read -r response
    if [[ "$response" =~ ^[Yy]$ ]]; then
        echo "" >> "$SHELL_CONFIG"
        echo "# git-ai debug commands (added $(date +%Y-%m-%d))" >> "$SHELL_CONFIG"
        echo "$PATH_EXPORT" >> "$SHELL_CONFIG"
        echo ""
        echo -e "${GREEN}✓ Added to $SHELL_CONFIG${NC}"
        echo ""
        echo "Reload your shell to use the commands:"
        echo -e "  ${YELLOW}source $SHELL_CONFIG${NC}"
    else
        echo ""
        echo -e "${YELLOW}Skipped.${NC} Manually add to your shell profile:"
        echo "  $PATH_EXPORT"
    fi
else
    echo -e "${YELLOW}Could not detect shell config file.${NC}"
    echo "Manually add to your shell profile (~/.zshrc or ~/.bashrc):"
    echo "  $PATH_EXPORT"
fi

echo ""
echo -e "${BLUE}Usage examples:${NC}"
echo "  debug-git-ai --version"
echo "  debug-git-ai rebase-authorship --help"
echo "  debug-git-ai cherry-pick-authorship --help"
echo "  debug-git-ai amend-authorship --help"
echo "  debug-git status  # Acts as git wrapper"
echo ""
echo -e "${GREEN}Note:${NC} These commands use the $BUILD_TYPE build. Rebuild with 'cargo build' to update."
