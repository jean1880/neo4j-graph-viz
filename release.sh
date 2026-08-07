#!/usr/bin/env bash
# Cut a release: bump the version, run the gate, commit, and tag.
#
#   ./release.sh                  # minor bump (a feature release)
#   ./release.sh patch            # 0.2.0 -> 0.2.1
#   ./release.sh major            # 0.2.0 -> 1.0.0
#   ./release.sh 1.4.2            # an explicit version
#   DRY_RUN=1 ./release.sh minor  # print what would happen, change nothing
#
# Deliberately does NOT push. Pushing a tag is what triggers CI to publish an image, so it stays
# a separate, conscious act — the script prints the exact command.
set -euo pipefail
cd "$(dirname "$0")"

DRY_RUN="${DRY_RUN:-0}"
BUMP="${1:-minor}"
BRANCH="${RELEASE_BRANCH:-master}"

die() { printf '\033[31merror:\033[0m %s\n' "$1" >&2; exit 1; }
step() { printf '\033[36m==>\033[0m %s\n' "$1"; }
run() {
  if [[ "$DRY_RUN" == "1" ]]; then
    printf '  \033[90mwould run:\033[0m %s\n' "$*"
  else
    "$@"
  fi
}

# --- preconditions --------------------------------------------------------------------------
# A release must describe a known state of the tree. Releasing with uncommitted changes produces
# a tag that matches nothing you can check out again.
[[ -z "$(git status --porcelain)" ]] || die "working tree is dirty — commit or stash first"

current_branch="$(git branch --show-current)"
[[ "$current_branch" == "$BRANCH" ]] ||
  die "on '$current_branch', expected '$BRANCH' (override with RELEASE_BRANCH=)"

CURRENT="$(grep -m1 '^version = ' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')"
[[ -n "$CURRENT" ]] || die "could not read the current version from Cargo.toml"

# --- work out the next version --------------------------------------------------------------
IFS=. read -r major minor patch <<<"$CURRENT"
case "$BUMP" in
  major) NEXT="$((major + 1)).0.0" ;;
  minor) NEXT="${major}.$((minor + 1)).0" ;;
  patch) NEXT="${major}.${minor}.$((patch + 1))" ;;
  # An explicit version, for anything the three bumps do not express.
  [0-9]*.[0-9]*.[0-9]*) NEXT="$BUMP" ;;
  *) die "unknown bump '$BUMP' — use major, minor, patch, or an explicit x.y.z" ;;
esac

TAG="v${NEXT}"
git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null && die "tag ${TAG} already exists"

step "${CURRENT} -> ${NEXT}  (${TAG})"

# --- gate -----------------------------------------------------------------------------------
# Before the version changes, not after: a failed gate must leave nothing behind to undo.
step "running the gate"
if [[ "$DRY_RUN" == "1" ]]; then
  printf '  \033[90mwould run:\033[0m make gate\n'
else
  make gate || die "gate failed — nothing was changed"
fi

# --- bump, commit, tag ----------------------------------------------------------------------
step "bumping Cargo.toml"
if [[ "$DRY_RUN" != "1" ]]; then
  # Only the [package] version — the first match — never a dependency's.
  sed -i "0,/^version = \"${CURRENT}\"$/s//version = \"${NEXT}\"/" Cargo.toml
  # Refresh Cargo.lock so it agrees with the manifest.
  cargo metadata --format-version 1 --quiet >/dev/null
fi

step "committing"
run git add Cargo.toml Cargo.lock
run git commit -m "chore(release): ${TAG}"

step "tagging"
run git tag -a "${TAG}" -m "${TAG}"

if [[ "$DRY_RUN" == "1" ]]; then
  printf '\n\033[33mdry run — nothing changed.\033[0m\n'
  exit 0
fi

printf '\n\033[32m%s cut.\033[0m Not pushed — pushing the tag is what publishes an image:\n' "$TAG"
printf '    git push origin %s %s\n' "$BRANCH" "$TAG"
