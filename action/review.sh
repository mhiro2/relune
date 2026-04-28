#!/usr/bin/env bash
# review.sh — Run `relune review` for the GitHub Action.
#
# Implements the 2-pass strategy described in ROADMAP_REVIEW_PHASE2.md §3.2.1:
#   1. User pass: honors --deny / --rules / --except-* / --dialect, writes the
#      user-visible report at OUTPUT_PATH. rc determines has-blocking-findings.
#   2. Summary pass: same filters but WITHOUT --deny, writes a JSON to a
#      runner-temp path, drives has-findings / summary-* outputs. rc must be 0;
#      anything else is treated as an internal action error.
#
# Environment variables:
#   RELUNE_BIN        — Path to a pre-built relune binary (optional).
#   BEFORE            — Baseline schema file path (required).
#   AFTER             — Updated schema file path (required).
#   FORMAT            — text|markdown|json (required).
#   OUTPUT_PATH       — Output file path (optional; derived from FORMAT).
#   DIALECT           — auto|postgres|mysql|sqlite (optional).
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

# Build the shared args (filters + dialect) used by both passes.
common_args=(
  --before "${BEFORE}"
  --after  "${AFTER}"
)

if [[ -n "${DIALECT:-}" && "${DIALECT}" != "auto" ]]; then
  common_args+=(--dialect "${DIALECT}")
fi

while IFS= read -r line; do
  [[ -z "${line}" ]] && continue
  common_args+=(--rules "${line}")
done <<< "${RULES:-}"

while IFS= read -r line; do
  [[ -z "${line}" ]] && continue
  common_args+=(--except-rule "${line}")
done <<< "${EXCEPT_RULES:-}"

while IFS= read -r line; do
  [[ -z "${line}" ]] && continue
  common_args+=(--except-table "${line}")
done <<< "${EXCEPT_TABLES:-}"

# ---------------------------------------------------------------------------
# Pass 1: user-visible run (honors --deny). rc drives has-blocking-findings.
# ---------------------------------------------------------------------------
user_args=(review "${common_args[@]}" --format "${FORMAT}" -o "${OUTPUT_PATH}")
if [[ -n "${DENY:-}" ]]; then
  user_args+=(--deny "${DENY}")
fi

set +e
"${relune}" "${user_args[@]}"
user_rc=$?
set -e

case "${user_rc}" in
  0|10) ;;
  *)
    echo "::error::relune review failed with exit code ${user_rc}"
    exit "${user_rc}"
    ;;
esac

# ---------------------------------------------------------------------------
# Pass 2: summary pass (no --deny, JSON output). rc must be 0.
# ---------------------------------------------------------------------------
summary_path="${RUNNER_TEMP:-/tmp}/relune-review.summary.json"

set +e
"${relune}" review "${common_args[@]}" --format json -o "${summary_path}"
summary_rc=$?
set -e

if [[ ${summary_rc} -ne 0 ]]; then
  echo "::error::relune review summary pass failed with exit code ${summary_rc}"
  exit "${summary_rc}"
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
