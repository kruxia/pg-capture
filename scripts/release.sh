#!/bin/bash
# Release script for pg-capture

set -e

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo "=== pg-capture Release Script ==="
echo ""

# Check if we're on main branch
CURRENT_BRANCH=$(git branch --show-current)
if [ "$CURRENT_BRANCH" != "main" ]; then
    echo -e "${RED}Error: Must be on main branch to release${NC}"
    echo "Current branch: $CURRENT_BRANCH"
    exit 1
fi

# Check for uncommitted changes
if ! git diff-index --quiet HEAD --; then
    echo -e "${RED}Error: Uncommitted changes detected${NC}"
    echo "Please commit or stash your changes first"
    exit 1
fi

# Get current version
CURRENT_VERSION=$(grep "^version" Cargo.toml | sed 's/version = "\(.*\)"/\1/')
echo "Current version: $CURRENT_VERSION"

# Ask for new version
echo -n "Enter new version (e.g., 0.2.0): "
read NEW_VERSION

if [ -z "$NEW_VERSION" ]; then
    echo -e "${RED}Error: Version cannot be empty${NC}"
    exit 1
fi

# Validate version format
if ! [[ "$NEW_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo -e "${RED}Error: Invalid version format. Use x.y.z${NC}"
    exit 1
fi

echo ""
echo "Preparing release v$NEW_VERSION..."
echo ""

# Update version in Cargo.toml
sed -i.bak "s/^version = \".*\"/version = \"$NEW_VERSION\"/" Cargo.toml
rm Cargo.toml.bak

# Update version in Cargo.lock
cargo update -p pg-capture

# Update VERSION file
echo "$NEW_VERSION" > VERSION

# Run tests
echo -e "${YELLOW}Running tests...${NC}"
cargo test
echo -e "${GREEN}✓ Tests passed${NC}"

# Run formatting and linting
echo -e "${YELLOW}Running formatting and linting...${NC}"
cargo fmt
cargo clippy -- -D warnings
echo -e "${GREEN}✓ Code quality checks passed${NC}"

# Build release binary to ensure it compiles
echo -e "${YELLOW}Building release binary...${NC}"
cargo build --release
echo -e "${GREEN}✓ Build successful${NC}"

# Update CHANGELOG.md
echo ""
echo -e "${YELLOW}Please update CHANGELOG.md with the changes for v$NEW_VERSION${NC}"
echo "Press Enter when done..."
read

# Commit changes
git add Cargo.toml Cargo.lock VERSION CHANGELOG.md
git commit -m "chore: prepare release v$NEW_VERSION"

# Create tag
git tag -a "v$NEW_VERSION" -m "Release v$NEW_VERSION"

echo ""
echo -e "${GREEN}Release v$NEW_VERSION prepared successfully!${NC}"
echo ""
echo "Next steps:"
echo "1. Review the commit: git show HEAD"
echo "2. Push to GitHub: git push origin main --tags"
echo "3. GitHub Actions will automatically:"
echo "   - Build binaries for multiple platforms"
echo "   - Create Docker images"
echo "   - Create GitHub release"
echo "   - Publish to crates.io (if configured)"
echo ""
echo -e "${YELLOW}Ready to push? (y/N)${NC}"
read -r response

if [[ "$response" =~ ^([yY][eE][sS]|[yY])$ ]]; then
    git push origin main --tags
    echo -e "${GREEN}✓ Release pushed to GitHub${NC}"
    echo "Check https://github.com/yourusername/pg-capture/actions for build status"
else
    echo "Release prepared but not pushed. To push later:"
    echo "  git push origin main --tags"
fi