# GitHub Actions

Use the `mhiro2/relune/action` composite action to run `relune diff` or `relune review`
in CI and surface schema changes — and migration risks — on pull requests.

This guide focuses on **how to wire the action into a workflow**. For a full reference of
every input, output, and exit code, see [`action/README.md`](../action/README.md).

> [!NOTE]
> **Supported runners:** Linux (`ubuntu-latest`) and macOS (`macos-latest`). Windows
> runners are not supported because pre-built binaries are not published for Windows.

> [!IMPORTANT]
> **Version pin:** until the first tagged release is published, `@v0` does not resolve.
> Use `@main` or a full commit SHA until the tag exists. The snippets below assume the
> standard post-release `@v0` pin.

---

## Quick start

### Diff — render schema changes

```yaml
- uses: mhiro2/relune/action@v0
  id: diff
  with:
    mode: diff   # default; passing it explicitly is recommended for clarity
    before: base-schema.sql
    after: head-schema.sql
```

The action writes `relune-diff.md` (default) and exposes `has-changes` / `output-path`
outputs that downstream steps can use to post a PR comment or upload an artifact.

### Review — surface migration risks

```yaml
- uses: mhiro2/relune/action@v0
  id: review
  with:
    mode: review
    before: base-schema.sql
    after: head-schema.sql
    deny: breaking
```

`review` mode emits `has-findings` / `has-blocking-findings` / `summary-*` outputs and
writes `relune-review.md` by default. By default it does **not** fail the action when
blocking findings are detected so a follow-up step can still post a PR comment; flip
`fail-on-blocking: "true"` to gate the merge instead.

The full input / output reference lives in [`action/README.md`](../action/README.md).

---

## Preparing schema files

Both modes compare two single-file schemas (SQL DDL or schema JSON). If your project uses
incremental migrations, you need a step that produces the final schema for each ref before
calling the action.

**Rails**

```yaml
- run: bin/rails db:schema:dump SCHEMA=schema.sql
```

**Flyway / Liquibase**

```yaml
- run: flyway migrate && pg_dump --schema-only > schema.sql
```

**Plain SQL migrations**

```yaml
- run: cat migrations/*.sql > schema.sql
```

**pg_dump from a live database**

```yaml
- run: pg_dump --schema-only "$DATABASE_URL" > schema.sql
```

For a workflow that materializes the schema from both the base and the head ref, see
[`docs/examples/migration-diff.yaml`](examples/migration-diff.yaml). The same materialize
step works for review — only the action invocation differs.

---

## Reviewing migrations on pull requests

The recommended pattern for `mode: review` is a single sticky comment that is created on
first push and edited in place on subsequent pushes — including the case where findings
are resolved, so the previous "blocking" comment is replaced with a clean message instead
of being left behind.

[`action/examples/relune-review.yaml`](../action/examples/relune-review.yaml) is a
copy-paste workflow that implements this pattern. The points worth pulling out for any
custom workflow are:

- **`peter-evans/find-comment` runs unconditionally.** When `has-findings` flips back to
  `false`, the upsert step still rewrites the existing comment with "No risk findings"
  rather than leaving the prior warning in place.
- **`peter-evans/create-or-update-comment` is given `body-path:`, not `body:`.** Action
  inputs do not expand `$(cat ...)`; ship multi-line markdown via a file path.
- **`concurrency` is keyed on the PR number.** Pushing twice in quick succession cancels
  the in-flight run so an older job cannot resurrect a stale "blocking" comment after a
  newer run has finished.
- **Fork PRs are skipped at the job level.** `GITHUB_TOKEN` is read-only on fork PRs
  against public repositories, so the comment step would 403 even with
  `pull-requests: write`. See [Fork pull requests](#fork-pull-requests) below for
  alternatives if you need to support them.
- **External actions are pinned to a commit SHA** with the version recorded in a trailing
  comment. This is the same convention used by this repository's own workflows.

Set `fail-on-blocking: "true"` to make the action exit non-zero when blocking findings are
detected. Keep it `"false"` (the default) when you want the comment to land before the
job fails — typical for review workflows where the value comes from the PR comment, not
from the failing check.

For diffs, [`docs/examples/migration-diff.yaml`](examples/migration-diff.yaml) provides
the equivalent comment-on-PR pattern using `actions/github-script`.

---

## Fork pull requests

The action itself only runs `relune` and writes a file — it does not post comments or
interact with the GitHub API.

However, `on: pull_request` from a fork has a read-only `GITHUB_TOKEN` and **cannot**
write comments on the base repository. This affects both diff and review workflows that
post PR comments.

Options:

1. **Skip fork PRs.** The review sample uses a job-level
   `if: github.event.pull_request.head.repo.full_name == github.repository` so the entire
   job is skipped for fork PRs. If you still want to produce review or diff artifacts for
   forks, split the analysis and the comment step into separate jobs and put the guard
   only on the comment job.
2. **Use a `workflow_run` trigger.** Split into two workflows: one that generates the
   artifact (`pull_request`), and one that posts the comment (`workflow_run` with
   `pull-requests: write`).
3. **Use `pull_request_target`.** Only consider this if you understand the security
   implications — the workflow runs with the base repository's secrets against
   fork-controlled code.

---

## Sample workflows

| File | Mode | Description |
|------|------|-------------|
| [`docs/examples/migration-diff.yaml`](examples/migration-diff.yaml) | diff | Materialize schemas from base + head, post a sticky Markdown diff PR comment, optional HTML artifact. |
| [`docs/examples/schema-diff-artifact.yaml`](examples/schema-diff-artifact.yaml) | diff | Minimal example: generate an SVG diff artifact. |
| [`action/examples/relune-review.yaml`](../action/examples/relune-review.yaml) | review | Sticky PR comment for migration risk review, with clean handling for resolved findings and fork PRs. |

Copy a sample into your repository's `.github/workflows/` directory and adjust the schema
generation steps and paths to match your tooling.
