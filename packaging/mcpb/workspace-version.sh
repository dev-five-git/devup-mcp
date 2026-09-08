#!/usr/bin/env bash
# Prints the version `changepacks update` maintains in [workspace.package].
#
# Every crate sets `version.workspace = true`, so this one number is the
# released version and the version the MCP bundle must carry. Reading it here
# rather than in each workflow step keeps the manifest, the archive filename
# and the release tag from drifting apart.
#
# The section range matters: `version` also appears in [workspace.dependencies]
# entries, and reading the whole file would eventually pick one of those up.
# sed looks for the range's closing address on the line after the opening one,
# so `[workspace.package]` does not close its own range.
set -euo pipefail

manifest="${1:-Cargo.toml}"

version="$(sed -n '/^\[workspace\.package\]/,/^\[/ s/^version[[:space:]]*=[[:space:]]*"\(.*\)"/\1/p' "$manifest" | head -n1)"

if [ -z "$version" ]; then
  echo "no [workspace.package] version found in $manifest" >&2
  exit 1
fi

printf '%s\n' "$version"
