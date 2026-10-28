#!/usr/bin/env bash
# run.sh — Dispatch the GitHub Action to diff.sh or review.sh by MODE.
#
# Environment variables:
#   MODE — "diff" or "review"; anything else exits with code 2.
#   Every other variable is consumed by the dispatched script.

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

case "${MODE:-}" in
  diff)
    exec bash "${script_dir}/diff.sh"
    ;;
  review)
    exec bash "${script_dir}/review.sh"
    ;;
  *)
    echo "::error::Invalid mode '${MODE:-}'. Expected 'diff' or 'review'."
    exit 2
    ;;
esac
