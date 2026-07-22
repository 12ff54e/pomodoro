#!/bin/bash
# Tag a release from the version in Cargo.toml.
# Usage:
#   ./tag-release.sh               create local tag only
#   ./tag-release.sh --push         create tag and push to origin
#   ./tag-release.sh --major        bump major version, commit, tag
#   ./tag-release.sh --minor        bump minor version, commit, tag
#   ./tag-release.sh --patch        bump patch version, commit, tag
#   ./tag-release.sh --patch --push bump patch, commit, tag, push
#
# Options:
#   --major  Bump major version (x.0.0)
#   --minor  Bump minor version (0.x.0)
#   --patch  Bump patch version (0.0.x)
#   --push   Push commit and/or tag to origin

set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"

# --- Argument parsing ---
PUSH=false
BUMP=""

usage() {
  echo "Usage: $0 [--major|--minor|--patch] [--push]"
  echo ""
  echo "  --major  Bump major version (x.0.0)"
  echo "  --minor  Bump minor version (0.x.0)"
  echo "  --patch  Bump patch version (0.0.x)"
  echo "  --push   Push to origin after tagging"
  echo ""
  echo "Without bump options, tags the current version as-is."
  exit 1
}

for arg in "$@"; do
  case "$arg" in
    --major|--minor|--patch)
      if [[ -n "$BUMP" ]]; then
        echo "Error: --major, --minor, and --patch are mutually exclusive" >&2
        exit 1
      fi
      BUMP="${arg#--}"
      ;;
    --push)
      PUSH=true
      ;;
    -h|--help)
      usage
      ;;
    *)
      echo "Error: unknown option '$arg'" >&2
      usage
      ;;
  esac
done

# --- Read current version ---
CURRENT=$(grep '^version' "$ROOT/src-tauri/Cargo.toml" | head -1 | sed 's/.*"\(.*\)"/\1/')

# Validate version format (must be X.Y.Z)
if ! [[ "$CURRENT" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Error: unexpected version format in Cargo.toml: '$CURRENT'" >&2
  echo "Expected semver format: major.minor.patch" >&2
  exit 1
fi

# --- Version bump ---
if [[ -n "$BUMP" ]]; then
  IFS='.' read -r major minor patch <<< "$CURRENT"

  case "$BUMP" in
    major)
      major=$((major + 1))
      minor=0
      patch=0
      ;;
    minor)
      minor=$((minor + 1))
      patch=0
      ;;
    patch)
      patch=$((patch + 1))
      ;;
  esac

  NEW_VERSION="$major.$minor.$patch"
  echo "Bumping version: $CURRENT -> $NEW_VERSION"

  # Update Cargo.toml
  sed -i.bak "s/^version = \".*\"/version = \"$NEW_VERSION\"/" "$ROOT/src-tauri/Cargo.toml"
  rm -f "$ROOT/src-tauri/Cargo.toml.bak"
  echo "Updated: src-tauri/Cargo.toml"

  # Let Cargo regenerate Cargo.lock with the new version
  echo "Running cargo check to update Cargo.lock..."
  export PATH="/c/msys64/mingw64/bin:$PATH"
  (cd "$ROOT/src-tauri" && cargo check 2>&1)
  echo "Updated: src-tauri/Cargo.lock"

  # Stage and commit
  git add "$ROOT/src-tauri/Cargo.toml" "$ROOT/src-tauri/Cargo.lock"
  git commit -m "Bump version to $NEW_VERSION"
  echo "Committed: Bump version to $NEW_VERSION"

  VERSION="$NEW_VERSION"
else
  VERSION="$CURRENT"
fi

# --- Tag ---
TAG="v$VERSION"

if git rev-parse "$TAG" >/dev/null 2>&1; then
  echo "Error: tag $TAG already exists" >&2
  exit 1
fi

git tag -a "$TAG" -m "Release $TAG"
echo "Created tag: $TAG"

# --- Push ---
if $PUSH; then
  if [[ -n "$BUMP" ]]; then
    # Push the bump commit first, then the tag
    git push origin HEAD
    git push origin "$TAG"
    echo "Pushed commit and tag $TAG to origin"
  else
    git push origin "$TAG"
    echo "Pushed $TAG to origin"
  fi
fi
