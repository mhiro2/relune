---
name: relune
description: Visualize, inspect, lint, diff, review, and export database schemas using the relune CLI. Use when working with SQL DDL files, database ERDs, schema reviews, migration diffs, migration risk reviews, or generating diagram-as-code output (Mermaid, D2, DOT).
---

# Relune

Understand, visualize, and review database schemas from the command line.

## Why relune

- **Visualize schemas** -- render ERDs as SVG (static) or HTML (interactive pan/zoom/search/filters)
- **Document schemas** -- generate Markdown documentation covering tables, columns, keys, indexes, views, and enums
- **Inspect structure** -- summarize tables, columns, types, constraints, and relationships
- **Lint for issues** -- detect missing primary keys, FK index gaps, naming inconsistencies, orphan tables
- **Diff revisions** -- compare before/after schemas with text or visual diffs
- **Review migrations** -- flag dropped references, narrowing types, NOT NULL on existing data, missing FK indexes, and other migration risks across `info`, `warning`, `caution`, `breaking` severities
- **Export anywhere** -- generate Mermaid, D2, Graphviz DOT, or normalized JSON
- **Multi-dialect** -- PostgreSQL, MySQL, MariaDB, SQLite
- **Multiple inputs** -- SQL files, inline SQL, schema JSON, live database introspection

## Installation

macOS:

```bash
brew install --cask mhiro2/tap/relune
```

Linux: download the latest `relune_*_linux_*` archive from the GitHub Releases page and place `relune` on your `PATH`.

## Quick Start

```bash
# Render an ERD as SVG
relune render --sql schema.sql -o erd.svg

# Interactive HTML viewer
relune render --sql schema.sql --format html -o erd.html

# Generate Markdown documentation
relune doc --sql schema.sql -o schema.md

# Summarize the schema
relune inspect --sql schema.sql

# Check for issues
relune lint --sql schema.sql

# Compare two schema versions
relune diff --before old.sql --after new.sql

# Review a migration for safety risks
relune review --before old.sql --after new.sql
```

## Input Sources

Every command requires at least one input. Combine with any subcommand.

| Input | Flag | Notes |
|-------|------|-------|
| SQL file | `--sql <FILE>` | DDL file (max 8 MiB) |
| Inline SQL | `--sql-text '<DDL>'` | Quick one-off (not available on `lint`) |
| Schema JSON | `--schema-json <FILE>` | From a previous `relune export` |
| Live database | `--db-url <URL>` | Read-only introspection (`postgres://`, `mysql://`, `mariadb://`, `sqlite:`) |
| SQL dialect | `--dialect auto\|postgres\|mysql\|sqlite` | For SQL parsing (default: `auto`) |

## Global Options

Place these before the subcommand.

| Option | Description |
|--------|-------------|
| `-c`, `--config <FILE>` | TOML config file; merges with flags (flags win) |
| `--color auto\|always\|never` | Terminal styling |
| `-v`, `--verbose` | More log output (repeatable: `-v` info, `-vv` debug, `-vvv` trace) |
| `-q`, `--quiet` | Less non-error output |

## Commands

### render

Generate ERD visualizations.

```bash
relune render --sql schema.sql -o erd.svg
relune render --sql schema.sql --format html -o erd.html
relune render --sql schema.sql --focus orders --depth 2 -o orders.svg
relune render --sql schema.sql --layout force-directed --edge-style curved --theme dark -o erd.svg
relune render --sql schema.sql --group-by schema -o grouped.svg
relune render --sql schema.sql --include users --include orders -o subset.svg
relune render --config relune.toml --sql schema.sql --viewpoint billing -o billing.svg
relune render --db-url 'postgres://user:pass@localhost:5432/mydb' -o erd.svg
```

| Option | Values | Default |
|--------|--------|---------|
| `-f`, `--format` | `svg`, `html`, `graph-json`, `schema-json` | `svg` |
| `-o`, `--out` | Output file path | stdout (requires `--stdout` on terminals) |
| `--layout` | `hierarchical`, `force-directed` | `hierarchical` |
| `--edge-style` | `straight`, `orthogonal`, `curved` | `orthogonal` |
| `--direction` | `top-to-bottom`, `left-to-right`, `right-to-left`, `bottom-to-top` | `top-to-bottom` |
| `--theme` | `light`, `dark` | `light` |
| `--viewpoint` | Named preset from `[viewpoints.<name>]` in config | -- |
| `--focus` | Table name to center on | -- |
| `--depth` | Neighbor depth (requires `--focus`) | `1` |
| `--group-by` | `none`, `schema`, `prefix` | `none` |
| `--include` | Repeatable allowlist | -- |
| `--exclude` | Repeatable denylist | -- |
| `--stats` | Print statistics to stderr | -- |
| `--fail-on-warning` | Non-zero exit on warnings | -- |

Validation rules:
- `--depth` requires `--focus`
- The focused table cannot be excluded
- If `--include` is set, it must contain the focused table
- The same table cannot appear in both `--include` and `--exclude`

Named viewpoints are applied before explicit CLI view flags. Effective precedence is: CLI flags > selected viewpoint > command defaults from `[render]`.

### doc

Generate Markdown documentation for a schema.

```bash
relune doc --sql schema.sql -o schema.md
relune doc --sql schema.sql
relune doc --db-url 'postgres://user:pass@localhost:5432/mydb' -o schema.md
```

| Option | Values | Default |
|--------|--------|---------|
| `-o`, `--out` | Output file path | stdout |
| `--fail-on-warning` | Non-zero exit on warnings | -- |

### inspect

Show schema summary or table details.

```bash
relune inspect --sql schema.sql
relune inspect --sql schema.sql --table orders
relune inspect --sql schema.sql --table orders --format json
relune inspect --sql schema.sql --table orders --format json -o inspect.json
relune inspect --db-url 'postgres://user:pass@localhost:5432/mydb'
```

| Option | Values | Default |
|--------|--------|---------|
| `--table` | Table name for detail view | -- (shows summary) |
| `--summary` | Force summary mode | -- |
| `--format` | `text`, `json` | `text` |
| `-o`, `--out` | Output file path | stdout |
| `--fail-on-warning` | Non-zero exit on warnings | -- |

### export

Emit normalized JSON or diagram-as-code text. `--format` is required.

```bash
relune export --sql schema.sql --format mermaid -o erd.mmd
relune export --sql schema.sql --format d2 -o erd.d2
relune export --sql schema.sql --format dot -o erd.dot
relune export --sql schema.sql --format schema-json -o schema.json
relune export --sql schema.sql --format graph-json -o graph.json
relune export --sql schema.sql --format layout-json --layout force-directed -o layout.json
relune export --sql schema.sql --format mermaid --focus orders --depth 2 -o orders.mmd
relune export --config relune.toml --sql schema.sql --format graph-json --viewpoint billing -o billing.json
```

| Format | Description |
|--------|-------------|
| `schema-json` | Normalized schema as JSON |
| `graph-json` | Graph representation (nodes/edges) as JSON |
| `layout-json` | Positioned graph with coordinates and `routing_debug` metadata |
| `mermaid` | Mermaid `erDiagram` -- renders in GitHub/GitLab Markdown |
| `d2` | D2 diagram source |
| `dot` | Graphviz DOT source |

Supports `--layout`, `--edge-style`, `--direction`, `--viewpoint`, `--focus`, `--depth`, `--group-by`, `--include`, and `--exclude` for graph-backed exports. `layout-json` includes graph-level detour counts plus per-edge side, slot, and channel metadata, which makes route diffs easier to audit alongside SVG/HTML output.
`--fail-on-warning` is also available when export diagnostics should fail automation.

### Named viewpoints in config

```toml
[viewpoints.billing]
focus = "orders"
depth = 1
group_by = "schema"
include = ["orders", "order_items", "payments"]
exclude = ["audit_*"]

[render]
viewpoint = "billing"
```

Use viewpoints when you want the same boundary to be reused across `render` and `export`.

### lint

Detect structural issues and anti-patterns. Note: `--sql-text` is not available for this command.

```bash
relune lint --sql schema.sql
relune lint --sql schema.sql --format json
relune lint --sql schema.sql --format json -o lint.json
relune lint --sql schema.sql --profile strict --rule-category documentation
relune lint --sql schema.sql --deny warning
relune lint --sql schema.sql --rules no-primary-key --rules missing-foreign-key-index
relune lint --sql schema.sql --exclude-rules missing-table-comment --except-table audit_*
relune lint --db-url 'postgres://user:pass@localhost:5432/mydb'
```

| Option | Values | Default |
|--------|--------|---------|
| `--format` | `text`, `json` | `text` |
| `-o`, `--out` | Output file path | stdout |
| `--profile` | `default`, `strict` | `default` |
| `--rules` | Repeatable; run only these rules (kebab-case IDs) | all rules |
| `--exclude-rules` | Repeatable; remove rules from the active set | -- |
| `--rule-category` | Repeatable; keep `structure`, `relationships`, `naming`, `documentation` | all categories |
| `--except-table` | Repeatable table pattern suppression | -- |
| `--deny` | `error`, `warning`, `info`, `hint` -- min severity for non-zero exit | -- |
| `--fail-on-warning` | Non-zero exit on warning diagnostics | -- |

Rule categories cover structure, relationships, naming conventions, and documentation. `strict` adds column comment coverage on top of the default schema review profile.
`--deny` applies to lint issues and parse diagnostics together, so warning-level parser diagnostics now fail the command when the configured threshold includes warnings.

### diff

Compare two schema revisions. Both before and after inputs are required.

```bash
relune diff --before old.sql --after new.sql
relune diff --before old.sql --after new.sql --format json -o diff.json
relune diff --before old.sql --after new.sql --format html -o diff.html
relune diff --before old.sql --after new.sql --format svg -o diff.svg
relune diff --before old.sql --after new.sql --format html --stdout > diff.html
relune diff \
  --before-sql-text 'CREATE TABLE users (id INT PRIMARY KEY);' \
  --after-sql-text 'CREATE TABLE users (id INT PRIMARY KEY, name TEXT NOT NULL);'
relune diff --before-schema-json old.json --after-schema-json new.json
```

| Side | Flags |
|------|-------|
| Before | `--before <FILE>`, `--before-sql-text '<DDL>'`, `--before-schema-json <FILE>` |
| After | `--after <FILE>`, `--after-sql-text '<DDL>'`, `--after-schema-json <FILE>` |

| Option | Values | Default |
|--------|--------|---------|
| `-f`, `--format` | `text`, `json`, `markdown`, `svg`, `html` | `text` |
| `-o`, `--out` | Output file path | stdout (`svg`/`html` on terminals require `--stdout`) |
| `--stdout` | Allow raw `svg`/`html` on interactive stdout | off |
| `--dialect` | `auto`, `postgres`, `mysql`, `sqlite` | `auto` |
| `--exit-code` | Exit with code 10 if schema changes are detected (like `git diff --exit-code`) | off |
| `--fail-on-warning` | Non-zero exit on warnings | -- |

File inputs are auto-detected by content (schema JSON works even without `.json` extension).

### review

Compare a `before` schema with an `after` schema and emit migration risk findings, grouped into `info`, `warning`, `caution`, and `breaking` severities. Both before and after inputs are required (except in `--list-rules` mode).

```bash
relune review --before old.sql --after new.sql
relune review --before old.sql --after new.sql --format markdown -o review.md
relune review --before old.sql --after new.sql --format json -o review.json
relune review --before old.sql --after new.sql --deny breaking
relune review --before old.sql --after new.sql --except-rule fk-without-index
relune review --before old.sql --after new.sql --except-table audit_*
relune review --before old.sql --after new.sql --exit-code
relune review --before old.sql --after new.sql --deny breaking --emit-summary review.json
relune review --list-rules                       # text catalog of every rule
relune review --list-rules --format json         # JSON catalog (for CI / docs)
```

| Side | Flags |
|------|-------|
| Before | `--before <FILE>`, `--before-sql-text '<DDL>'`, `--before-schema-json <FILE>` |
| After | `--after <FILE>`, `--after-sql-text '<DDL>'`, `--after-schema-json <FILE>` |

| Option | Values | Default |
|--------|--------|---------|
| `-f`, `--format` | `text`, `markdown`, `json` | `text` |
| `-o`, `--out` | Output file path | stdout |
| `--dialect` | `auto`, `postgres`, `mysql`, `sqlite` | `auto` |
| `--rules <RULE>` | Repeatable; run only these rules (`risk/<id>` or bare `<id>`) | all rules |
| `--except-rule <RULE>` | Repeatable; remove rules from the active set | -- |
| `--except-table <PATTERN>` | Repeatable; suppress findings for matching tables (`*` glob) | -- |
| `--deny` | `info`, `warning`, `caution`, `breaking` -- min severity for non-zero exit | -- |
| `--exit-code` | Exit `10` when any findings are emitted | off |
| `--list-rules` | List every rule (with default severity and description) and exit; honors `--format text\|json` only | off |
| `--emit-summary <PATH>` | Always write the full review JSON to `PATH`, even when `--deny` short-circuits with rc=10 | off |

Rule IDs are kebab-case under the `risk/` namespace; for example `risk/drop-column-referenced`, `risk/add-not-null-on-existing`, `risk/fk-without-index`. `--list-rules` is the canonical source of every rule for CI / docs automation. `--emit-summary` is intended for CI jobs that need the structured report in a single pass (PR comment generation that still wants `--deny` to gate the build); reusing the `--out` path is rejected as a usage error.

## Common Workflows

### Schema review

Combine doc, inspect, lint, and render for a full schema audit:

```bash
relune doc --sql schema.sql -o schema.md                 # documentation
relune inspect --sql schema.sql                          # overview
relune lint --sql schema.sql                             # find issues
relune render --sql schema.sql --format html -o erd.html # visualize
relune inspect --sql schema.sql --table <TABLE>          # drill into flagged tables
```

### Migration review

Diff before/after schemas, surface migration safety risks, and lint the result:

```bash
relune diff --before old.sql --after new.sql                          # text diff
relune diff --before old.sql --after new.sql --format markdown        # GFM for PR comments
relune diff --before old.sql --after new.sql --format html -o d.html  # visual diff
relune diff --before old.sql --after new.sql --exit-code              # exit 10 if changes
relune review --before old.sql --after new.sql                        # migration risk findings
relune review --before old.sql --after new.sql --deny breaking        # gate CI on breaking risks
relune review --list-rules                                            # catalog of every review rule
relune lint --sql new.sql                                             # lint new schema
relune render --sql new.sql --focus <CHANGED_TABLE> --depth 1 -o area.svg
```

### Playground viewpoint presets

The public playground also exposes example-specific named viewpoints. Pick a built-in example, switch the `Viewpoint` control, and the playground will apply the corresponding focus, filter, and grouping preset while keeping the selection in the URL.

### Playground risk review view

The playground's compare workbench has a fifth output mode, **Risk review**, alongside the existing visual / text / markdown / JSON views. Paste a baseline schema on the left, the proposed migration on the right, and switch the compare-format control to `Risk review` — the playground calls the same review pipeline as the CLI through WASM and renders severity badges, finding cards (rule ID, target, message, mitigation), and a collapsible suppressed-findings panel. The `Copy JSON` / `Download JSON` actions emit the same shape as `relune review --format json`, so playground exports drop into the same tooling as CLI output. The view is JSON-only in this release; rule allowlists, table exceptions, and `--deny` are not yet exposed in the UI.

### Embed ERDs in documentation

Export as Mermaid for GitHub/GitLab Markdown:

```bash
relune export --sql schema.sql --format mermaid -o docs/erd.mmd
```

### CI quality gate

Fail the build on lint warnings:

```bash
relune lint --sql schema.sql --deny warning
```

### GitHub Actions

A composite action is available at `mhiro2/relune/action` (Linux and macOS runners). It runs either `relune diff` or `relune review` against two schema files and exposes structured outputs for follow-up steps such as PR comments.

```yaml
# diff mode (default) -- render a schema diff
- uses: mhiro2/relune/action@241c85bcf2b8de4e8c3c19491cad67898671817c # v0.10.0
  id: diff
  with:
    before: base-schema.sql
    after: head-schema.sql
    format: markdown        # text, json, markdown, svg, html

- if: steps.diff.outputs.has-changes == 'true'
  uses: actions/github-script@v7
  with:
    script: |
      const body = require('fs').readFileSync('${{ steps.diff.outputs.output-path }}', 'utf8');
      // ... create or update PR comment
```

```yaml
# review mode -- run the migration risk review
- uses: mhiro2/relune/action@241c85bcf2b8de4e8c3c19491cad67898671817c # v0.10.0
  id: review
  with:
    mode: review
    before: base-schema.sql
    after: head-schema.sql
    deny: breaking          # gate on breaking findings
    fail-on-blocking: false # let the next step post the comment first

- if: steps.review.outputs.has-findings == 'true'
  uses: actions/github-script@v7
  with:
    script: |
      const body = require('fs').readFileSync('${{ steps.review.outputs.output-path }}', 'utf8');
      // ... post sticky review comment

# Fail the job after the comment has been posted
- if: steps.review.outputs.has-blocking-findings == 'true'
  run: exit 1
```

Common inputs: `version`, `mode` (`diff` | `review`), `before`, `after`, `format`, `output-path`, `dialect`, `binary-path`.

Review-only inputs: `deny` (`info` | `warning` | `caution` | `breaking`), `rules`, `except-rules`, `except-tables` (newline-separated lists), `fail-on-blocking` (`"true"` | `"false"`).

| Output | diff mode | review mode |
|--------|-----------|-------------|
| `output-path` | path to rendered diff | path to review report |
| `has-changes` | `"true"` / `"false"` | empty |
| `has-findings` | empty | `"true"` if any finding |
| `has-blocking-findings` | empty | `"true"` when CLI returned rc=10 |
| `summary-breaking` / `summary-caution` / `summary-warning` / `summary-info` | empty | per-severity counts |

Internally, review mode shells `relune review --emit-summary` once: `has-findings` and `summary-*` are derived from the structured JSON; `has-blocking-findings` follows the CLI exit code so it stays in sync with `--deny`.

See `docs/github-actions.md` for full reference and sample workflows; a complete review workflow lives at `action/examples/relune-review.yaml`.

## Configuration

Use a TOML config file for shared defaults. CLI flags override config values.

```bash
relune --config relune.toml render --sql schema.sql -o erd.svg
```

```toml
[render]
format = "svg"
theme = "light"
layout = "hierarchical"
edge_style = "curved"
direction = "top-to-bottom"
group_by = "none"
include = ["users", "orders"]
exclude = ["schema_migrations"]

[inspect]
format = "text"
fail_on_warning = false

[export]
format = "schema-json"
fail_on_warning = false

[doc]
fail_on_warning = false

[lint]
deny = "warning"
fail_on_warning = false

[diff]
format = "markdown"
dialect = "postgres"
fail_on_warning = false

[review]
format = "text"
dialect = "postgres"
deny = "breaking"
except_rules = ["fk-without-index"]
except_tables = ["audit_*"]

# Per-rule severity overrides. Key is the full `risk/<id>` rule ID.
[review.severity_overrides."risk/add-not-null-on-existing"]
severity = "info"
```

Merge order: built-in defaults -> config file -> CLI arguments.

`[review.severity_overrides."<rule-id>"]` is applied **after** rule evaluation and before summary aggregation, so `summary` counts and the `--deny` decision both reflect the overridden severity. Unknown rule IDs are a usage error. See `docs/configuration.md` for the full reference.

## Troubleshooting

### Terminal requires --stdout

When rendering or diffing as SVG or HTML without `-o`, interactive terminals require `--stdout` to emit raw output. Use `-o` to write to a file instead.

### Input too large

Relune rejects SQL files and schema JSON files larger than 8 MiB before loading them into memory.

### Dialect detection issues

Use `--dialect postgres|mysql|sqlite` to force a specific SQL dialect when auto-detection fails.
