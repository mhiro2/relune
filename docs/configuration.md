# Configuration

Relune optionally reads a **TOML** file passed with **`-c` / `--config`**. Values from the file are merged with **CLI flags**; flags take precedence where both apply.

Unknown keys are rejected during config load. Typoed fields fail fast instead of being ignored.

**Merge order**

1. Built-in defaults  
2. Config file  
3. CLI arguments  

## Example

A full example lives in the repository at `fixtures/config/valid_full.toml`. Minimal pattern:

```toml
[render]
format = "svg"
theme = "light"
layout = "force-directed"
edge_style = "orthogonal"
direction = "left-to-right"
group_by = "none"
focus = "orders"
depth = 2
include = ["users", "orders"]
exclude = ["schema_migrations"]

[inspect]
format = "text"

[export]
format = "schema-json"
group_by = "schema"
layout = "hierarchical"
edge_style = "curved"
direction = "top-to-bottom"
focus = "orders"
depth = 1

[lint]
deny = "warning"

[diff]
format = "json"
dialect = "postgres"
```

```bash
relune --config relune.toml render --sql schema.sql -o erd.svg
```

---

## `[render]`

| Key | Values |
|-----|--------|
| `format` | `svg`, `html`, `graph-json`, `schema-json` |
| `theme` | `light`, `dark` |
| `layout` | `hierarchical`, `force-directed` |
| `edge_style` | `straight`, `orthogonal`, `curved` |
| `direction` | `top-to-bottom`, `left-to-right`, `right-to-left`, `bottom-to-top` |
| `group_by` | `none`, `schema`, `prefix` |
| `focus` | Table name |
| `depth` | Unsigned integer |
| `include` / `exclude` | String arrays |
| `show_legend`, `show_stats` | Booleans; `--stats` on the CLI forces `show_stats` only |

`layout`, `edge_style`, and `direction` can be set in the file and overridden with CLI flags. See `ReluneConfig::merge_render_args` in `crates/relune-cli/src/config.rs` for exact precedence.

Semantic validation is also applied after merge:

- `depth` must be at least `1`
- `depth` can only be set when `focus` is present
- table names in `focus`, `include`, and `exclude` must be non-empty and must not have surrounding whitespace
- the same table cannot appear in both `include` and `exclude`
- if `include` is non-empty, it must contain the focused table
- the focused table cannot also appear in `exclude`

---

## `[inspect]`

| Key | Values |
|-----|--------|
| `format` | `text`, `json` |

---

## `[export]`

| Key | Values |
|-----|--------|
| `format` | `schema-json`, `graph-json`, `layout-json`, `mermaid`, `d2`, `dot` |
| `group_by` | `none`, `schema`, `prefix` |
| `layout` | `hierarchical`, `force-directed` |
| `edge_style` | `straight`, `orthogonal`, `curved` |
| `direction` | `top-to-bottom`, `left-to-right`, `right-to-left`, `bottom-to-top` |
| `focus`, `depth` | Same as CLI |

`export.format` can be set in the config file and overridden with `--format`. If neither config nor CLI provides a format, the command fails fast. As with `render`, `export.depth` requires `export.focus`, and focused table names must be non-empty after trimming.

---

## `[lint]`

| Key | Values |
|-----|--------|
| `format` | `text`, `json` |
| `deny` | `error`, `warning`, `info`, `hint` — minimum severity for a non-zero exit when not overridden by `--deny` |

---

## `[diff]`

| Key | Values |
|-----|--------|
| `format` | `text`, `json`, `svg`, `html` |
| `dialect` | `auto`, `postgres`, `mysql`, `sqlite` |

`diff` still requires the before/after inputs on the CLI. The config file supplies defaults for `--format` and `--dialect` only, and CLI flags override them when provided. File-based `diff` inputs are detected by content, so schema JSON copied to a non-`.json` filename is still treated as schema JSON.

---

## Authoritative reference

The TOML schema and merge rules are defined in code:

- `crates/relune-cli/src/config.rs` — structure, load, merge  
- `fixtures/config/*.toml` — examples used in tests  
