# CLI reference

Global options (before the subcommand):

| Option | Description |
|--------|-------------|
| `-c`, `--config <FILE>` | Optional TOML config; merges with flags (see [Configuration](configuration.md)) |
| `--color auto\|always\|never` | Terminal styling |
| `-v`, `--verbose` | More log output (repeatable: `-v` info, `-vv` debug, `-vvv` trace with span events) |
| `-q`, `--quiet` | Less non-error output |

Every command requires **at least one input**. Typical inputs:

| Input | Flag | Notes |
|-------|------|--------|
| SQL file | `--sql <FILE>` | DDL |
| SQL string | `--sql-text '<DDL>'` | Not available on `lint` |
| Normalized schema JSON | `--schema-json <FILE>` | From a previous export |
| Live DB | `--db-url <URL>` | Read-only introspection |
| SQL dialect | `--dialect auto\|postgres\|mysql\|sqlite` | For SQL parsing |

Output path: **`-o` / `--out`** writes a file. `render` still prints to stdout when piped, but for interactive terminals it now requires **`--stdout`** to emit raw SVG/HTML directly.

For SQL files and schema JSON files, Relune currently rejects inputs larger than **8 MiB**.

---

## `render`

Generate SVG, HTML, or JSON representations of the ERD. SVG/HTML outputs include tables, views, and PostgreSQL enum types. For SQL-defined views, Relune preserves the full view definition and extracts columns from either an explicit `CREATE VIEW ... (cols...)` list or simple top-level `SELECT` items; more complex queries may render the view without inferred columns.

**Formats** (`-f` / `--format`): `svg` (default), `html`, `graph-json`, `schema-json`.

When rendering `svg` or `html` without `-o`, interactive terminals require `--stdout`; otherwise Relune asks you to choose a file output path or explicitly opt in to raw stdout.

**View options:**

| Option | Description |
|--------|-------------|
| `--focus <TABLE>` | Center on a table |
| `--depth <N>` | Neighbor depth for focus (default `1`) |
| `--group-by none\|schema\|prefix` | Group tables |
| `--include <TABLE>` | Repeatable allowlist |
| `--exclude <TABLE>` | Repeatable denylist |
| `--theme light\|dark` | Visual theme |
| `--layout hierarchical\|force-directed` | Layout algorithm |
| `--direction top-to-bottom\|left-to-right\|right-to-left\|bottom-to-top` | Primary flow direction |
| `--edge-style straight\|orthogonal\|curved` | Edge rendering style |

**Other:** `--stats` (stderr statistics), `--fail-on-warning` (non-zero on warnings).

`render` validates focus/filter combinations before running:

- `--depth` requires `--focus`
- the focused table cannot also be excluded
- if `--include` is set, it must contain the focused table
- the same table cannot appear in both `--include` and `--exclude`

```bash
relune render --sql schema.sql -o erd.svg
relune render --sql schema.sql --format html -o erd.html
relune render --sql schema.sql --format html --stdout > erd.html
relune render --sql schema.sql --focus orders --depth 2 -o orders.svg
relune render --sql schema.sql --group-by schema -o grouped.svg
relune render --sql schema.sql --layout force-directed --edge-style orthogonal -o force.svg
relune render --sql schema.sql --include users --include orders -o subset.svg
relune render --schema-json schema.json -o from-json.svg
```

---

## `inspect`

Show a schema summary or details for one table.

| Option | Description |
|--------|-------------|
| `--table <NAME>` | Table to inspect; omit for summary |
| `--summary` | Force summary mode |
| `--format text\|json` | Output encoding |

```bash
relune inspect --sql schema.sql
relune inspect --sql schema.sql --table orders
relune inspect --sql schema.sql --table orders --format json
```

---

## `export`

Emit normalized JSON or diagram text. **`--format` is required.**

**Formats:**

| Format | Description |
|--------|-------------|
| `schema-json` | Normalized schema as JSON |
| `graph-json` | Graph representation (nodes/edges) as JSON |
| `layout-json` | Positioned graph with coordinates plus `routing_debug` metadata |
| `mermaid` | Mermaid `erDiagram` — renders in GitHub/GitLab Markdown |
| `d2` | [D2](https://d2lang.com/) diagram source |
| `dot` | Graphviz DOT source |

Supports `--focus`, `--depth`, `--group-by`, `--layout`, `--direction`, and `--edge-style` like `render` for positioned exports. `export` applies the same `focus`/`depth` validation rule as `render`, so `--depth` requires `--focus`.

`layout-json` includes graph-level `routing_debug.non_self_loop_detour_activations` and per-edge `routing_debug` fields for source/target side policy, slot indices, slot counts, row offsets, and selected channel coordinates.

```bash
relune export --sql schema.sql --format schema-json -o schema.json
relune export --sql schema.sql --format graph-json -o graph.json
relune export --sql schema.sql --format layout-json -o layout.json
relune export --sql schema.sql --format layout-json --layout force-directed --edge-style orthogonal -o layout-force.json
relune export --sql schema.sql --format mermaid -o erd.mmd
relune export --sql schema.sql --format d2 -o erd.d2
relune export --sql schema.sql --format dot -o erd.dot
```

Routing debug comparison workflow:

```bash
relune export --sql schema.sql --format layout-json > layout.json
relune render --sql schema.sql --format svg -o erd.svg
relune render --sql schema.sql --format html -o erd.html
```

---

## `lint`

Run built-in rules on the schema. Inputs: **`--sql`**, **`--schema-json`**, or **`--db-url`** (no `--sql-text` on this command).

| Option | Description |
|--------|-------------|
| `--format text\|json` | Report format |
| `--rules <RULE>` | Repeatable; run only these rules |
| `--deny error\|warning\|info\|hint` | Minimum severity for non-zero exit |

```bash
relune lint --sql schema.sql
relune lint --sql schema.sql --format json
relune lint --sql schema.sql --deny warning
relune lint --sql schema.sql --rules no-primary-key --rules missing-foreign-key-index
```

Rule IDs are **kebab-case** (for example `missing-foreign-key-index`, `non-snake-case-identifier`). Categories include primary keys, orphan tables, naming, FK indexes, nullable FK risks, and related heuristics.

---

## `diff`

Compare two schemas. Provide **before** and **after** inputs independently (each side uses one of the following).

**Before:** `--before <FILE>`, `--before-sql-text '<DDL>'`, or `--before-schema-json <FILE>`.

**After:** `--after <FILE>`, `--after-sql-text '<DDL>'`, or `--after-schema-json <FILE>`.

When `--before <FILE>` or `--after <FILE>` is used, Relune inspects the file contents and treats schema JSON as schema JSON even if the extension is not `.json`.

| Option | Description |
|--------|-------------|
| `-f`, `--format text\|json\|svg\|html` | Output format |
| `-o`, `--out <FILE>` | Optional file (else stdout) |
| `--dialect` | For SQL parsing on both sides |

```bash
relune diff --before old_schema.sql --after new_schema.sql
relune diff --before old.sql --after new.sql --format json -o diff.json
relune diff --before old.sql --after new.sql --format html -o diff.html
relune --config relune.toml diff --before old.sql --after new.sql
```
