<p align="center">
  <img src="assets/logo.png" width="256" height="256" alt="relune logo">
</p>

<h1 align="center">Relune</h1>

<p align="center">
  <strong>Schema visualization and analysis for developers.</strong><br>
  Relune renders, inspects, exports, lints, and diffs database schemas from SQL DDL and live database metadata.
</p>

---

## Why Relune

Relune is not just an ERD viewer.

It is a schema toolchain for understanding and working with database structure across multiple workflows:

- visualize schemas as SVG or interactive HTML
- inspect structure from SQL or live databases
- export diagrams to text-friendly formats for docs and code review
- lint schema quality issues
- diff schema changes between versions
- emit structured JSON for automation and downstream tooling

The goal is to make schemas easier to explore, review, document, and evolve.

## Features

### Diagram rendering

Generate schema diagrams as:

- **SVG** for static documentation and README assets
- **HTML** for interactive exploration with pan, zoom, search, and filters
- render **tables, views, and PostgreSQL enums** with distinct node/edge styles

### Layout and edge control

Tune readability depending on the shape of your schema:

- **Hierarchical** and **force-directed** layouts
- **Straight**, **orthogonal**, and **curved** edge routing

### Focus and filtering

Reduce noise in large schemas:

- focus on a table
- control traversal depth
- group by schema or prefix
- include or exclude selected tables

### Text-based exports

Produce review-friendly outputs for docs and pull requests:

- **Mermaid**
- **D2**
- **Graphviz DOT**

### Inspection and diagnostics

Understand schema shape and detect common issues:

- schema summaries and structural stats
- missing primary keys
- foreign-key index gaps
- naming inconsistencies
- other lint-style diagnostics

### Diff and machine-readable output

Compare schema revisions and integrate with automation:

- text diff output
- JSON output for CI and tooling

### Multiple input sources

Work from whichever source fits your workflow:

- SQL files
- inline SQL
- schema JSON
- live databases:
  - PostgreSQL
  - MySQL / MariaDB
  - SQLite

### Rust core, portable interfaces

Relune is built in Rust with a reusable core designed for:

- native CLI workflows
- browser-facing WASM environments
- future editor and tooling integrations

## Installation

### Homebrew

```bash
brew install --cask mhiro2/tap/relune
```

### Prebuilt binaries

Download the latest release from the GitHub Releases page and place relune on your PATH.

## Quick Start

```bash
# Render an SVG
relune render --sql schema.sql -o erd.svg

# Interactive HTML viewer
relune render --sql schema.sql --format html -o erd.html

# Focus on the “orders” table with depth 2
relune render --sql schema.sql --focus orders --depth 2 -o orders.svg

# Use a force-directed layout with orthogonal edges
relune render --sql schema.sql --layout force-directed --edge-style orthogonal -o erd-force.svg

# Summarize the schema
relune inspect --sql schema.sql

# Export as Mermaid
relune export --sql schema.sql --format mermaid -o erd.mmd

# Lint the schema
relune lint --sql schema.sql

# Compare two schemas
relune diff --before old.sql --after new.sql
```

Run `relune --help` or `relune <command> --help` for the full option list.

## Documentation

| Document | Contents |
|----------|----------|
| [Getting started](docs/getting-started.md) | Installation, first commands, live database introspection |
| [CLI reference](docs/cli-reference.md) | Commands and flags |
| [Configuration](docs/configuration.md) | `relune.toml` and merge rules |

## License

MIT
