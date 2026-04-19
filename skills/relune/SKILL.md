---
name: relune
description: Visualize, inspect, lint, diff, and export database schemas using the relune CLI. Use when working with SQL DDL files, database ERDs, schema reviews, migration diffs, or generating diagram-as-code output (Mermaid, D2, DOT).
---

# Relune

Understand, visualize, and review database schemas from the command line.

## Why relune

- **Visualize schemas** -- render ERDs as SVG (static) or HTML (interactive pan/zoom/search/filters)
- **Document schemas** -- generate Markdown documentation covering tables, columns, keys, indexes, views, and enums
- **Inspect structure** -- summarize tables, columns, types, constraints, and relationships
- **Lint for issues** -- detect missing primary keys, FK index gaps, naming inconsistencies, orphan tables
- **Diff revisions** -- compare before/after schemas with text or visual diffs
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

Diff before/after schemas and lint the result:

```bash
relune diff --before old.sql --after new.sql                          # text diff
relune diff --before old.sql --after new.sql --format markdown        # GFM for PR comments
relune diff --before old.sql --after new.sql --format html -o d.html  # visual diff
relune diff --before old.sql --after new.sql --exit-code              # exit 10 if changes
relune lint --sql new.sql                                             # lint new schema
relune render --sql new.sql --focus <CHANGED_TABLE> --depth 1 -o area.svg
```

### Playground viewpoint presets

The public playground also exposes example-specific named viewpoints. Pick a built-in example, switch the `Viewpoint` control, and the playground will apply the corresponding focus, filter, and grouping preset while keeping the selection in the URL.

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

A composite action is available at `mhiro2/relune/action` (Linux and macOS runners).

```yaml
- uses: mhiro2/relune/action@v0
  id: diff
  with:
    before: base-schema.sql
    after: head-schema.sql
    format: markdown        # text, json, markdown, svg, html

# Post comment only when changes are detected
- if: steps.diff.outputs.has-changes == 'true'
  uses: actions/github-script@v7
  with:
    script: |
      const body = require('fs').readFileSync('${{ steps.diff.outputs.output-path }}', 'utf8');
      // ... create or update PR comment
```

Action inputs: `version`, `before`, `after`, `format`, `output-path`, `binary-path`.
Action outputs: `has-changes` (`"true"` / `"false"`), `output-path`.

See `docs/github-actions.md` for full reference and sample workflows.

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
```

Merge order: built-in defaults -> config file -> CLI arguments.

## Troubleshooting

### Terminal requires --stdout

When rendering or diffing as SVG or HTML without `-o`, interactive terminals require `--stdout` to emit raw output. Use `-o` to write to a file instead.

### Input too large

Relune rejects SQL files and schema JSON files larger than 8 MiB before loading them into memory.

### Dialect detection issues

Use `--dialect postgres|mysql|sqlite` to force a specific SQL dialect when auto-detection fails.
