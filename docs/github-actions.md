# GitHub Actions

Use the `mhiro2/relune/action` composite action to run `relune diff` in CI and surface schema changes on pull requests.

> **Supported runners:** Linux (`ubuntu-latest`) and macOS (`macos-latest`). Windows runners are not supported because pre-built binaries are not available for Windows.

---

## Quick start

```yaml
- uses: mhiro2/relune/action@v0
  id: diff
  with:
    before: base-schema.sql
    after: head-schema.sql
```

The action writes a Markdown diff report (by default) and exposes two outputs:

| Output | Description |
|--------|-------------|
| `has-changes` | `"true"` if schema changes were detected |
| `output-path` | Path to the generated diff file |

---

## Preparing schema files

`relune diff` compares two single-file schemas (SQL DDL or schema JSON). If your project uses incremental migrations, you need a step that produces the final schema for each ref before calling the action.

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

See [`docs/examples/migration-diff.yaml`](examples/migration-diff.yaml) for a full workflow that generates schemas from both the base and head ref.

---

## Action inputs

| Input | Required | Default | Description |
|-------|----------|---------|-------------|
| `version` | no | `latest` | Relune version to install (e.g. `0.7.0`). Ignored when `binary-path` is set. |
| `before` | **yes** | — | Path to the baseline schema file (SQL or schema JSON). |
| `after` | **yes** | — | Path to the updated schema file (SQL or schema JSON). |
| `format` | no | `markdown` | Output format: `text`, `json`, `markdown`, `svg`, or `html`. |
| `output-path` | no | auto | Path for the diff output file. Defaults to `relune-diff.{md,svg,html,json,txt}` based on format. |
| `binary-path` | no | — | Path to a pre-built `relune` binary. Skips the install step — useful for testing unreleased builds. |

## Action outputs

| Output | Description |
|--------|-------------|
| `has-changes` | `"true"` if the diff detected changes, `"false"` otherwise. |
| `output-path` | Path to the generated diff file. |

---

## Using a locally built binary

When testing action changes before a release, build relune in an earlier job and pass the binary via `binary-path`:

```yaml
- name: Build relune
  run: cargo build -p relune-cli --release

- uses: mhiro2/relune/action@v0
  with:
    before: base.sql
    after: head.sql
    binary-path: target/release/relune
```

---

## Fork pull requests

This action itself only runs `relune diff` and writes a file — it does not post comments or interact with the GitHub API.

However, if your workflow posts PR comments (e.g. via `peter-evans/create-or-update-comment`), be aware that `on: pull_request` from a fork has a read-only `GITHUB_TOKEN` and **cannot** write comments on the base repository.

Options:

1. **Accept the limitation** — the diff artifact is still uploaded; the comment step simply fails for fork PRs.
2. **Use a `workflow_run` trigger** — split into two workflows: one that generates the artifact (`pull_request`), and one that posts the comment (`workflow_run` with `pull-requests: write`).

---

## Sample workflows

| File | Description |
|------|-------------|
| [`migration-diff.yaml`](examples/migration-diff.yaml) | Full example: diff migrations, post Markdown PR comment, optional HTML artifact. |
| [`schema-diff-artifact.yaml`](examples/schema-diff-artifact.yaml) | Minimal example: generate an SVG diff artifact. |

Copy a sample into your repository's `.github/workflows/` directory and adjust the schema generation steps to match your tooling.
