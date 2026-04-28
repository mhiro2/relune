# Relune Schema Diff & Review action

Composite GitHub Action that runs [`relune diff`](../docs/cli-reference.md#diff) or
[`relune review`](../docs/cli-reference.md#review) against two schema files and writes the
result to a file the workflow can consume.

This document is the **reference** for the action — every input, output, and exit code is
listed here. For higher-level CI guidance and sample workflows see
[`docs/github-actions.md`](../docs/github-actions.md).

> [!NOTE]
> **Supported runners:** Linux (`ubuntu-latest`) and macOS (`macos-latest`). Windows is
> not supported because pre-built binaries are not published for Windows.

> [!IMPORTANT]
> **Version pin:** until the first tagged release is published, `@v0` does not resolve.
> Use `@main` or a full commit SHA in the meantime. The examples below use `@v0` because
> it is the recommended pin once the tag exists.

---

## Quick start

### Diff (default)

Render a Markdown diff for two schema files. `mode` defaults to `diff`, so passing it is
optional but recommended for clarity.

```yaml
- uses: mhiro2/relune/action@v0
  id: diff
  with:
    mode: diff
    before: base-schema.sql
    after: head-schema.sql
```

### Review

Run the migration risk review and gate on `breaking` findings.

```yaml
- uses: mhiro2/relune/action@v0
  id: review
  with:
    mode: review
    before: base-schema.sql
    after: head-schema.sql
    deny: breaking
```

By default the action keeps the workflow successful even when blocking findings are
detected (see [Exit codes](#exit-codes)), so a follow-up step can post the report to the PR
before the job fails. Set `fail-on-blocking: "true"` to make the action fail immediately
instead.

---

## Inputs

### Common (both modes)

| Input | Required | Default | Description |
|-------|----------|---------|-------------|
| `mode` | no | `diff` | `diff` or `review`. Any other value fails the action with a usage error. |
| `version` | no | `latest` | Relune version to install (for example `0.7.0`). Ignored when `binary-path` is set. |
| `before` | **yes** | — | Path to the baseline schema file (SQL DDL or schema JSON). |
| `after` | **yes** | — | Path to the updated schema file (SQL DDL or schema JSON). |
| `format` | no | `markdown` | Output format. Mode-specific accepted values: see below. |
| `output-path` | no | auto | Path for the generated file. Defaults are derived from `mode` and `format` (see [Output paths](#output-paths)). |
| `dialect` | no | `auto` | SQL dialect: `auto`, `postgres`, `mysql`, or `sqlite`. Applied to **both** `diff` and `review`. `auto` lets the CLI infer the dialect from the file contents. |
| `binary-path` | no | `""` | Path to a pre-built `relune` binary. Skips the install step — useful for testing unreleased builds in CI. |

`format` accepted values:

| Mode | Accepted formats |
|------|------------------|
| `diff` | `text`, `json`, `markdown`, `svg`, `html` |
| `review` | `text`, `json`, `markdown` (the CLI rejects `svg` / `html`) |

### Review-only

These inputs are read only when `mode: review`. They are ignored in `diff` mode.

| Input | Default | Description |
|-------|---------|-------------|
| `deny` | `""` | Minimum severity that counts as blocking: `info`, `warning`, `caution`, or `breaking`. Empty means `--deny` is not passed and the CLI never returns `rc=10`. |
| `rules` | `""` | Newline-separated rule ids. Each non-empty line becomes `--rules <id>` (repeatable). Both `risk/<id>` and the bare short form are accepted. |
| `except-rules` | `""` | Newline-separated rule ids to suppress. Each line becomes `--except-rule <id>` (the action input is plural for YAML readability; the CLI flag is singular and repeatable). |
| `except-tables` | `""` | Newline-separated table patterns to suppress (glob `*` supported). Each line becomes `--except-table <pattern>`. |
| `fail-on-blocking` | `"false"` | When `"true"`, the action exits non-zero whenever blocking findings are detected. Default keeps the workflow running so a follow-up step can post a PR comment first. |

### Output paths

When `output-path` is omitted, the action derives a default from `mode` and `format`:

| Mode | Format | Default `output-path` |
|------|--------|-----------------------|
| `diff` | `markdown` | `relune-diff.md` |
| `diff` | `svg` | `relune-diff.svg` |
| `diff` | `html` | `relune-diff.html` |
| `diff` | `json` | `relune-diff.json` |
| `diff` | `text` | `relune-diff.txt` |
| `review` | `markdown` | `relune-review.md` |
| `review` | `json` | `relune-review.json` |
| `review` | `text` | `relune-review.txt` |

---

## Outputs

| Output | Mode | Description |
|--------|------|-------------|
| `output-path` | both | Path to the generated report file. |
| `has-changes` | diff | `"true"` when the diff detected any change, `"false"` otherwise. Empty in `review` mode. |
| `has-findings` | review | `"true"` when at least one finding was emitted at any severity, `"false"` otherwise. Empty in `diff` mode. |
| `has-blocking-findings` | review | `"true"` when the user-pass CLI returned `rc=10` (a finding reached the `--deny` threshold). Always `"false"` when `deny` is empty. Empty in `diff` mode. |
| `summary-breaking` | review | Number of `breaking` findings from the report summary. |
| `summary-caution` | review | Number of `caution` findings. |
| `summary-warning` | review | Number of `warning` findings. |
| `summary-info` | review | Number of `info` findings. |

> [!NOTE]
> **Why `has-findings` exists alongside the rc-driven `has-blocking-findings`**: the CLI
> only returns `rc=10` when `--deny` is set and a finding reaches the threshold. With no
> `--deny`, or with `--deny breaking` against warning-only findings, the CLI returns
> `rc=0`. The action reads the JSON summary out-of-band so workflows can branch on
> "any findings exist" independently of the deny gate.

---

## PR comment recipe

The action only writes a file. Posting that file to a pull request is intentionally left
to the workflow so each project can choose its own comment strategy
(sticky / one-shot / artifact-only) and token model.

The reference workflow at
[`action/examples/relune-review.yaml`](examples/relune-review.yaml) is a copy-paste sticky
template that uses `peter-evans/find-comment` + `peter-evans/create-or-update-comment`,
re-runs `find` on every push so resolved findings clear the previous comment, and skips
fork PRs (where `GITHUB_TOKEN` is read-only). It assumes a canonical schema at
`db/schema/schema.sql`; edit that path and the `paths:` filter to match your project.

For the diff equivalent, see
[`docs/examples/migration-diff.yaml`](../docs/examples/migration-diff.yaml).

---

## Exit codes

The action's exit code follows the underlying CLI exit code plus a small amount of
mode-specific translation. The relevant CLI exit codes are:

| CLI rc | Meaning |
|--------|---------|
| `0` | Success. In `diff` mode, no changes were detected. In `review` mode, no findings exist or all findings are below `--deny`. |
| `2` | Usage error (bad flags, unknown rule id, etc.). |
| `3` | `--fail-on-warning` path. The action does not pass that flag, so this should not occur in practice; if the CLI returns it anyway the action exits with the CLI's rc. |
| `10` | `diff` detected changes (with `--exit-code`) **or** `review` produced a finding at or above `--deny`. |
| other | Internal error. |

### Diff mode

| CLI rc | `has-changes` | Action exit |
|--------|---------------|-------------|
| `0` | `"false"` | success |
| `10` | `"true"` | success — the action treats this as the "changes detected" signal, not a failure. |
| other | n/a | `::error::relune diff failed with exit code N` and exit with the CLI's rc. |

### Review mode

The action runs `relune review` twice (see [Two-pass execution](#two-pass-execution)). The
**user pass** drives `has-blocking-findings`; the **summary pass** drives `has-findings`
and the per-severity counts.

| User-pass rc | `has-findings` | `has-blocking-findings` | Action exit |
|--------------|----------------|-------------------------|-------------|
| `0`, summary total `0` | `"false"` | `"false"` | success |
| `0`, summary total ≥ 1 | `"true"` | `"false"` | success |
| `10` (only when `--deny` is set) | `"true"` | `"true"` | success when `fail-on-blocking: "false"` (default), or `exit 10` when `fail-on-blocking: "true"` |
| `1` / `2` / `3` / other | n/a | n/a | `::error::relune review failed with exit code N` and exit with the CLI's rc. |

If the summary pass itself returns non-zero (it should always be `0` because `--deny` is
not passed), the action treats that as an internal error and fails with that rc — better
to fail loudly than to publish wrong outputs.

---

## Two-pass execution

`mode: review` runs the CLI twice for the same input:

1. **User pass** — honors all of `--deny`, `--rules`, `--except-rule`, `--except-table`,
   and `--dialect`, writes the user-visible report at `output-path` in the requested
   format. The exit code drives `has-blocking-findings`.
2. **Summary pass** — same filters and dialect, but with `--deny` stripped and `--format
   json` directed at a runner-temp path. The action parses the summary counts to populate
   `has-findings` and `summary-*` outputs. The exit code must be `0`.

Why two passes:

- The CLI returns `rc=10` whether a single `breaking` finding fired or fifty `info`
  findings did, depending on `--deny`. The summary pass is the only reliable way to
  separate "any findings" from "blocking findings" without forcing every workflow to set
  `--deny info`.
- Stripping `--deny` from the summary pass is required: the CLI returns `rc=10` after
  writing output when the threshold is met, which under `set -e` would abort the script
  before the summary outputs are written.

The two passes share the same parse + diff cost, which is sub-second on schemas in the
hundreds of tables. A future CLI flag (e.g. `--emit-summary <PATH>`) can collapse this to
a single pass.

---

## Using a locally built binary

When testing action changes before a release, build relune in an earlier job and pass the
binary via `binary-path` to skip the install step:

```yaml
- name: Build relune
  run: cargo build -p relune-cli --release

- uses: mhiro2/relune/action@v0
  with:
    mode: review
    before: base.sql
    after: head.sql
    binary-path: target/release/relune
```

---

## Troubleshooting

### Install step fails with "Failed to resolve latest relune version"

The install step calls the GitHub Releases API to resolve `latest`. On busy public runners
this can be rate-limited when the workflow does not pass `GITHUB_TOKEN`. Pin to an
explicit version to bypass the API call:

```yaml
with:
  version: "0.8.0"
```

### Install step fails with "Unsupported runner OS / architecture"

The install script supports `RUNNER_OS=Linux|macOS` and `RUNNER_ARCH=X64|ARM64` only. Use
`runs-on: ubuntu-latest` or `macos-latest`. Windows runners are not supported because
pre-built binaries are not published for Windows; build relune from source and pass it
via `binary-path` if you need Windows.

### `dialect: auto` picks the wrong SQL dialect

The CLI infers the dialect from the file contents. When the heuristic guesses wrong (for
example, MySQL-style backticks in a file that should be parsed as PostgreSQL), set
`dialect` explicitly:

```yaml
with:
  dialect: postgres
```

`dialect` applies to both `diff` and `review`.

### `format: svg` or `format: html` rejected in review mode

The review CLI does not produce visual output. Use `markdown` (default), `text`, or
`json`. The action does not validate `format` itself — the CLI returns a usage error
that the action surfaces verbatim.

### `has-findings` is `"true"` but `has-blocking-findings` is `"false"`

This is expected when `deny` is empty, or when all findings are below the deny threshold.
The action keeps `has-findings` independent of `--deny` so workflows can branch on
"surface any findings as a comment" without having to gate the merge on them.

### Old "blocking" PR comment lingers after fixes

`has-findings == 'false'` does not mean "do nothing" — the comment step in
[`action/examples/relune-review.yaml`](examples/relune-review.yaml) always runs the find
step and replaces the body with a clean "No risk findings" message so the previous warning
is overwritten. If you write your own comment step, keep that pattern.
