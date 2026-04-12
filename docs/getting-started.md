# Getting started

## Install

[Browser playground](https://mhiro2.github.io/relune/) — no install required.

macOS:

```bash
brew install --cask mhiro2/tap/relune
```

Linux:

Download the latest `relune_*_linux_*` archive from the GitHub Releases page and place `relune` on your `PATH`.

## First commands

Render an SVG (default format):

```bash
relune render --sql schema.sql -o erd.svg
```

Self-contained **HTML** viewer (pan/zoom, search, filters):

```bash
relune render --sql schema.sql --format html -o erd.html
```

`render` draws tables, views, and PostgreSQL enum types when they are present in the schema.

Generate **Markdown documentation**:

```bash
relune doc --sql schema.sql -o schema.md
```

Summarize the schema in the terminal:

```bash
relune inspect --sql schema.sql
```

## Live database introspection

Point at a database URL instead of a SQL file (supported where the `relune-introspect` adapter allows):

```bash
relune render --db-url 'postgres://user:pass@localhost:5432/dbname' -o erd.svg
```

Dialects and URL schemes follow CLI help (`relune render --help`).
For PostgreSQL and MySQL/MariaDB, Relune applies a 30 second introspection statement deadline by default.
Remote TCP connections also require TLS by default; Unix sockets and loopback-only local connections are left untouched.

## Next steps

- [CLI reference](cli-reference.md) — all commands and flags
- [Configuration](configuration.md) — shared defaults in `relune.toml`
- [Public playground](https://mhiro2.github.io/relune/) — run the WASM build in your browser
