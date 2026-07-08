#!/bin/bash
# Tag a release from the version in Cargo.toml.
# Usage:
#   ./tag-release.sh        create local tag only
#   ./tag-release.sh --push  create tag and push to origin

set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"

VERSION=$(grep '^version' "$ROOT/src-tauri/Cargo.toml" | head -1 | sed 's/.*"\(.*\)"/\1/')
TAG="v$VERSION"

if git rev-parse "$TAG" >/dev/null 2>&1; then
  echo "Error: tag $TAG already exists"
  exit 1
fi

git tag -a "$TAG" -m "Release $TAG"

echo "Created tag: $TAG"

if [[ "${1:-}" == "--push" ]]; then
  git push origin "$TAG"
  echo "Pushed $TAG to origin"
fi
