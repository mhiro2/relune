#!/usr/bin/env bash
# diff.sh — Run `relune diff` for the GitHub Action.
#
# Environment variables:
#   RELUNE_BIN    — Path to a pre-built relune binary (optional).
#   BEFORE        — Baseline schema file path (required).
#   AFTER         — Updated schema file path (required).
#   FORMAT        — Output format: text|markdown|json|svg|html (required).
#   OUTPUT_PATH   — Output file path (optional; derived from FORMAT when empty).
#   DIALECT       — SQL dialect: auto|postgres|mysql|sqlite (optional).
#   GITHUB_OUTPUT — Set by GitHub Actions runtime.

set -euo pipefail

# Resolve binary
if [[ -n "${RELUNE_BIN:-}" ]]; then
  relune="${RELUNE_BIN}"
else
  relune="relune"
fi

# Derive default output path from format when not specified
if [[ -z "${OUTPUT_PATH:-}" ]]; then
  case "${FORMAT}" in
    markdown) OUTPUT_PATH="relune-diff.md" ;;
    svg)      OUTPUT_PATH="relune-diff.svg" ;;
    html)     OUTPUT_PATH="relune-diff.html" ;;
    json)     OUTPUT_PATH="relune-diff.json" ;;
    text)     OUTPUT_PATH="relune-diff.txt" ;;
    *)        OUTPUT_PATH="relune-diff.txt" ;;
  esac
fi

# Build args
args=(diff
  --before "${BEFORE}"
  --after  "${AFTER}"
  --format "${FORMAT}"
  --exit-code
  -o "${OUTPUT_PATH}")

if [[ -n "${DIALECT:-}" && "${DIALECT}" != "auto" ]]; then
  args+=(--dialect "${DIALECT}")
fi

# Run diff with --exit-code to detect changes
set +e
"${relune}" "${args[@]}"
rc=$?
set -e

if [[ $rc -eq 10 ]]; then
  echo "has-changes=true" >> "${GITHUB_OUTPUT}"
elif [[ $rc -eq 0 ]]; then
  echo "has-changes=false" >> "${GITHUB_OUTPUT}"
else
  echo "::error::relune diff failed with exit code ${rc}"
  exit "${rc}"
fi

echo "output-path=${OUTPUT_PATH}" >> "${GITHUB_OUTPUT}"
