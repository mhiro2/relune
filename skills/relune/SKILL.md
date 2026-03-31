---
name: relune
description: Visualize, inspect, lint, diff, and export database schemas using the relune CLI. Use when working with SQL DDL files, database ERDs, schema reviews, migration diffs, or generating diagram-as-code output (Mermaid, D2, DOT).
---

# Relune

Understand, visualize, and review database schemas from the command line.

## Why relune

- **Visualize schemas** -- render ERDs as SVG (static) or HTML (interactive pan/zoom/search/filters)
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
relune render --db-url 'postgres://user:pass@localhost:5432/mydb' -o erd.svg
```

| Option | Values | Default |
|--------|--------|---------|
| `-f`, `--format` | `svg`, `html`, `graph-json`, `schema-json` | `svg` |
| `-o`, `--out` | Output file path | stdout (requires `--stdout` on terminals) |
| `--layout` | `hierarchical`, `force-directed` | `hierarchical` |
| `--edge-style` | `straight`, `orthogonal`, `curved` | `straight` |
| `--direction` | `top-to-bottom`, `left-to-right`, `right-to-left`, `bottom-to-top` | `top-to-bottom` |
| `--theme` | `light`, `dark` | `light` |
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

### inspect

Show schema summary or table details.

```bash
relune inspect --sql schema.sql
relune inspect --sql schema.sql --table orders
relune inspect --sql schema.sql --table orders --format json
relune inspect --db-url 'postgres://user:pass@localhost:5432/mydb'
```

| Option | Values | Default |
|--------|--------|---------|
| `--table` | Table name for detail view | -- (shows summary) |
| `--summary` | Force summary mode | -- |
| `--format` | `text`, `json` | `text` |

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
```

| Format | Description |
|--------|-------------|
| `schema-json` | Normalized schema as JSON |
| `graph-json` | Graph representation (nodes/edges) as JSON |
| `layout-json` | Positioned graph with coordinates as JSON |
| `mermaid` | Mermaid `erDiagram` -- renders in GitHub/GitLab Markdown |
| `d2` | D2 diagram source |
| `dot` | Graphviz DOT source |

Supports `--layout`, `--edge-style`, `--direction`, `--focus`, `--depth`, `--group-by` for positioned exports.

### lint

Detect structural issues and anti-patterns. Note: `--sql-text` is not available for this command.

```bash
relune lint --sql schema.sql
relune lint --sql schema.sql --format json
relune lint --sql schema.sql --deny warning
relune lint --sql schema.sql --rules no-primary-key --rules missing-foreign-key-index
relune lint --db-url 'postgres://user:pass@localhost:5432/mydb'
```

| Option | Values | Default |
|--------|--------|---------|
| `--format` | `text`, `json` | `text` |
| `--rules` | Repeatable; run only these rules (kebab-case IDs) | all rules |
| `--deny` | `error`, `warning`, `info`, `hint` -- min severity for non-zero exit | -- |

Rule categories: primary keys, orphan tables, naming conventions, FK indexes, nullable FK risks.

### diff

Compare two schema revisions. Both before and after inputs are required.

```bash
relune diff --before old.sql --after new.sql
relune diff --before old.sql --after new.sql --format json -o diff.json
relune diff --before old.sql --after new.sql --format html -o diff.html
relune diff --before old.sql --after new.sql --format svg -o diff.svg
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
| `-f`, `--format` | `text`, `json`, `svg`, `html` | `text` |
| `-o`, `--out` | Output file path | stdout |
| `--dialect` | `auto`, `postgres`, `mysql`, `sqlite` | `auto` |

File inputs are auto-detected by content (schema JSON works even without `.json` extension).

## Common Workflows

### Schema review

Combine inspect, lint, and render for a full schema audit:

```bash
relune inspect --sql schema.sql                          # overview
relune lint --sql schema.sql                             # find issues
relune render --sql schema.sql --format html -o erd.html # visualize
relune inspect --sql schema.sql --table <TABLE>          # drill into flagged tables
```

### Migration review

Diff before/after schemas and lint the result:

```bash
relune diff --before old.sql --after new.sql                       # text diff
relune diff --before old.sql --after new.sql --format html -o d.html # visual diff
relune lint --sql new.sql                                          # lint new schema
relune render --sql new.sql --focus <CHANGED_TABLE> --depth 1 -o area.svg
```

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

[export]
format = "schema-json"

[lint]
deny = "warning"

[diff]
format = "json"
dialect = "postgres"
```

Merge order: built-in defaults -> config file -> CLI arguments.

## Troubleshooting

### Terminal requires --stdout

When rendering SVG or HTML without `-o`, interactive terminals require `--stdout` to emit raw output. Use `-o` to write to a file instead.

### Input too large

Relune rejects SQL files and schema JSON larger than 8 MiB.

### Dialect detection issues

Use `--dialect postgres|mysql|sqlite` to force a specific SQL dialect when auto-detection fails.
