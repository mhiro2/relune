#!/usr/bin/env bash
# review.sh — Run `relune review` for the GitHub Action.
#
# Single-pass implementation backed by `relune review --emit-summary`. The
# user-visible report is written at OUTPUT_PATH while the same run also writes
# a structured JSON summary to a runner-temp path. The summary file is produced
# even when `--deny` short-circuits the user-visible run with rc=10, so
# `has-findings` / `summary-*` outputs stay independent of the deny gate.
#
# Environment variables:
#   RELUNE_BIN        — Path to a pre-built relune binary (optional).
#   BEFORE            — Baseline schema file path (required).
#   AFTER             — Updated schema file path (required).
#   FORMAT            — text|markdown|json (required).
#   OUTPUT_PATH       — Output file path (optional; derived from FORMAT).
#   DIALECT           — auto|postgres|mysql|sqlite (optional). Drives both
#                       the SQL parser and the lock-risk rule evaluation.
#   DENY              — info|warning|caution|breaking (optional).
#   RULES             — Newline-separated rule ids (optional).
#   EXCEPT_RULES      — Newline-separated rule ids to exclude (optional).
#   EXCEPT_TABLES     — Newline-separated table patterns to suppress (optional).
#   FAIL_ON_BLOCKING  — "true" to exit non-zero when blocking findings are
#                       detected; anything else keeps the action successful.
#   RUNNER_TEMP       — Set by the GitHub Actions runtime.
#   GITHUB_OUTPUT     — Set by the GitHub Actions runtime.

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
    markdown) OUTPUT_PATH="relune-review.md" ;;
    json)     OUTPUT_PATH="relune-review.json" ;;
    text)     OUTPUT_PATH="relune-review.txt" ;;
    *)        OUTPUT_PATH="relune-review.txt" ;;
  esac
fi

# Use a unique temp file per invocation so concurrent jobs on a self-hosted
# runner cannot stomp on each other's summary, and a pre-existing symlink at a
# predictable path cannot redirect the write. BSD `mktemp` (macOS) only
# substitutes the trailing `XXXXXX`, so the placeholder must be the final
# segment of the template; a fixed suffix would leave the filename literal
# and break uniqueness. GNU `mktemp` (Ubuntu) accepts the same shape.
summary_dir="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
summary_path=$(mktemp "${summary_dir}/relune-review.summary.XXXXXX")
trap 'rm -f "${summary_path}"' EXIT

# Build CLI args (filters + dialect + emit-summary).
args=(
  review
  --before "${BEFORE}"
  --after  "${AFTER}"
  --format "${FORMAT}"
  -o "${OUTPUT_PATH}"
  --emit-summary "${summary_path}"
)

if [[ -n "${DIALECT:-}" && "${DIALECT}" != "auto" ]]; then
  args+=(--dialect "${DIALECT}")
fi

# Strip a trailing CR so workflow YAMLs authored with CRLF line endings do
# not produce rule ids / table patterns that the CLI rejects as unknown.
while IFS= read -r line; do
  line="${line%$'\r'}"
  [[ -z "${line}" ]] && continue
  args+=(--rules "${line}")
done <<< "${RULES:-}"

while IFS= read -r line; do
  line="${line%$'\r'}"
  [[ -z "${line}" ]] && continue
  args+=(--except-rule "${line}")
done <<< "${EXCEPT_RULES:-}"

while IFS= read -r line; do
  line="${line%$'\r'}"
  [[ -z "${line}" ]] && continue
  args+=(--except-table "${line}")
done <<< "${EXCEPT_TABLES:-}"

if [[ -n "${DENY:-}" ]]; then
  args+=(--deny "${DENY}")
fi

# Run the review. rc=0 → no blocking findings; rc=10 → `--deny` threshold met
# (the summary file is still written). Any other rc is an internal failure.
set +e
"${relune}" "${args[@]}"
user_rc=$?
set -e

case "${user_rc}" in
  0|10) ;;
  *)
    echo "::error::relune review failed with exit code ${user_rc}"
    exit "${user_rc}"
    ;;
esac

if [[ ! -s "${summary_path}" ]]; then
  echo "::error::relune review did not write the --emit-summary file at ${summary_path}"
  exit 1
fi

# Parse summary counts. jq is preinstalled on GitHub-hosted runners.
breaking=$(jq -r '.summary.breaking' "${summary_path}")
caution=$(jq -r '.summary.caution'  "${summary_path}")
warning=$(jq -r '.summary.warning'  "${summary_path}")
info=$(jq    -r '.summary.info'     "${summary_path}")
total=$(( breaking + caution + warning + info ))

if [[ ${total} -gt 0 ]]; then
  has_findings="true"
else
  has_findings="false"
fi

# `has-blocking-findings` follows the rc of the single pass: rc=10 means a
# finding hit the `--deny` threshold. The summary's `.denied` field carries
# the same signal, but rc is the canonical source.
if [[ ${user_rc} -eq 10 ]]; then
  has_blocking="true"
else
  has_blocking="false"
fi

{
  echo "has-findings=${has_findings}"
  echo "has-blocking-findings=${has_blocking}"
  echo "summary-breaking=${breaking}"
  echo "summary-caution=${caution}"
  echo "summary-warning=${warning}"
  echo "summary-info=${info}"
  echo "output-path=${OUTPUT_PATH}"
} >> "${GITHUB_OUTPUT}"

if [[ "${has_blocking}" == "true" && "${FAIL_ON_BLOCKING:-false}" == "true" ]]; then
  echo "::error::relune review found blocking findings (--deny=${DENY})"
  exit 10
fi

exit 0
