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

---

## Quick start

### Diff (default)

Render a Markdown diff for two schema files. `mode` defaults to `diff`, so passing it is
optional but recommended for clarity.

```yaml
- uses: mhiro2/relune/action@54221dc4b373100b19f3e8a5d302cfe580844630 # v0.11.0
  id: diff
  with:
    mode: diff
    before: base-schema.sql
    after: head-schema.sql
```

### Review

Run the migration risk review and gate on `breaking` findings.

```yaml
- uses: mhiro2/relune/action@54221dc4b373100b19f3e8a5d302cfe580844630 # v0.11.0
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
| `version` | no | `latest` | Relune version to install (for example `0.11.0`). Ignored when `binary-path` is set. |
| `before` | **yes** | — | Path to the baseline schema file (SQL DDL or schema JSON). |
| `after` | **yes** | — | Path to the updated schema file (SQL DDL or schema JSON). |
| `format` | no | `markdown` | Output format. Mode-specific accepted values: see below. |
| `output-path` | no | auto | Path for the generated file. Defaults are derived from `mode` and `format` (see [Output paths](#output-paths)). |
| `dialect` | no | `auto` | SQL dialect: `auto`, `postgres`, `mysql`, or `sqlite`. Applied to **both** `diff` and `review`. `auto` lets the CLI infer the dialect from the file contents. In `review` mode, `auto` also promotes to a concrete dialect when both `before` and `after` parse to the same one (so SQL-only workflows usually pick up lock-risk caution rules without setting `dialect`); pin `dialect: postgres` or `dialect: mysql` to force it. See [Lock-risk findings](#lock-risk-findings). |
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
| `has-blocking-findings` | review | `"true"` when the CLI returned `rc=10` (a finding reached the `--deny` threshold). Always `"false"` when `deny` is empty. Empty in `diff` mode. |
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

`mode: review` invokes `relune review` once with `--emit-summary` (see
[Single-pass execution](#single-pass-execution)). The CLI's exit code drives
`has-blocking-findings`; the summary file drives `has-findings` and the per-severity
counts.

| CLI rc | `has-findings` | `has-blocking-findings` | Action exit |
|--------|----------------|-------------------------|-------------|
| `0`, summary total `0` | `"false"` | `"false"` | success |
| `0`, summary total ≥ 1 | `"true"` | `"false"` | success |
| `10` (only when `--deny` is set) | `"true"` | `"true"` | success when `fail-on-blocking: "false"` (default), or `exit 10` when `fail-on-blocking: "true"` |
| `1` / `2` / `3` / other | n/a | n/a | `::error::relune review failed with exit code N` and exit with the CLI's rc. |

`--emit-summary` writes the JSON summary even when the user-visible run short-circuits
with `rc=10`, so the action can populate `has-findings` and `summary-*` consistently
with the deny gate.

---

## Single-pass execution

`mode: review` runs the CLI once. `relune review --emit-summary <PATH>` writes the
user-visible report at `output-path` in the requested format **and** writes the full
JSON payload to a runner-temp path in the same pass. The action reads counts from that
JSON to populate `has-findings` / `summary-*`, and uses the CLI rc to populate
`has-blocking-findings`.

Why a separate summary file is still needed:

- `has-blocking-findings` follows `--deny`: the CLI returns `rc=10` only when a finding
  reaches the configured threshold. Workflows that want to surface "any findings exist"
  without forcing `--deny info` need an out-of-band counts source — the summary JSON
  fills that role independently of the deny rc.
- `--emit-summary` is guaranteed to write the file even when `--deny` short-circuits
  the user-visible run, so a single invocation is enough to drive every output.

---

## Lock-risk findings

`mode: review` activates the lock-risk caution rules whenever the effective dialect
resolves to `postgres` or `mysql`. They surface state changes whose naive execution
acquires a problematic lock — `CREATE INDEX` on an existing table, `ADD FOREIGN KEY`
on an existing table, and `ALTER COLUMN TYPE` on a non-equivalent type for both
dialects, plus PK rotation / column drops that require a table rewrite for
`dialect: mysql` (`risk/rewrite-table` is MySQL-only because that engine forces a
full table rebuild; PostgreSQL handles those edits without a rewrite). Each match
becomes a `caution` finding in the PR comment.

With `dialect: auto` (the default), the CLI promotes `auto` to the parser-resolved
dialect whenever both `before` and `after` parse to the same one — so SQL-only
workflows pick up lock-risk caution rules without any extra configuration. Pin
`dialect: postgres` or `dialect: mysql` to force the resolution (for example when
inputs are schema-JSON, which carries no parser-side dialect signal). When the two
sides resolve to different dialects, the run stays inactive and emits a `REVIEW002`
warning so the skip is visible in the report.

To gate the merge on lock-risk findings, pair the activation with `deny: caution`. The
caution band is opinionated, so reach for `except-rules` (e.g. `risk/alter-column-type`)
when a specific rule produces too much noise for your project. The action does not
read `relune.toml`, so `[review.severity_overrides]` only applies when you invoke
`relune review` directly (for example, from a custom step that runs the binary
yourself).

> [!NOTE]
> **Lock-risk caution rules read schema state, not migration SQL.**
> Lock-risk caution rules are based on **schema state-change diff**, not on the migration
> SQL itself. They flag a state change that — if executed naively — would acquire a
> problematic lock; they **do not** read your migration script and cannot detect that
> you wrote `CREATE INDEX CONCURRENTLY` or `ALGORITHM=INPLACE`. Treat the caution as a
> "make sure you used the safe variant" reminder.

---

## Using a locally built binary

When testing action changes before a release, build relune in an earlier job and pass the
binary via `binary-path` to skip the install step:

```yaml
- name: Build relune
  run: cargo build -p relune-cli --release

- uses: mhiro2/relune/action@54221dc4b373100b19f3e8a5d302cfe580844630 # v0.11.0
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
  version: "0.11.0"
```

### Install step fails with "Unsupported runner OS / architecture"

The install script supports `RUNNER_OS=Linux|macOS` and `RUNNER_ARCH=X64|ARM64` only. Use
`runs-on: ubuntu-latest` or `macos-latest`. Windows runners are not supported because
pre-built binaries are not published for Windows; build relune from source and pass it
via `binary-path` if you need Windows.

### Install step fails with "Checksum mismatch"

The install script verifies the downloaded archive's SHA-256 against the
`checksums.txt` published alongside the release. This catches corrupted downloads and is
a basic integrity check, but `checksums.txt` is fetched from the same release URL — it
is not a substitute for signature verification. A mismatch most often means a transient
download corruption (retry the job); investigate before bypassing. To skip the install
step entirely, build relune from source and pass it via `binary-path`.

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
