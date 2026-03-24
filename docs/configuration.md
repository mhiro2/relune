# Configuration

Relune optionally reads a **TOML** file passed with **`-c` / `--config`**. Values from the file are merged with **CLI flags**; flags take precedence where both apply.

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
| `group_by` | `none`, `schema`, `prefix` |
| `focus` | Table name |
| `depth` | Unsigned integer |
| `include` / `exclude` | String arrays |
| `show_legend`, `show_stats` | Booleans; `--stats` on the CLI also forces stats-style behavior when merging |

`layout` and `edge_style` can be set in the file and overridden with CLI flags. See `ReluneConfig::merge_render_args` in `crates/relune-cli/src/config.rs` for exact precedence.

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
| `focus`, `depth` | Same as CLI |

**`export` and `--format`:** The CLI requires `--format` on each `export` run, so you always choose the output format on the command line. The file can still supply **`group_by`**, **`layout`**, **`edge_style`**, **`focus`**, and **`depth`** when you do not override them with flags.

---

## `[lint]`

| Key | Values |
|-----|--------|
| `deny` | `error`, `warning`, `info`, `hint` — minimum severity for a non-zero exit when not overridden by `--deny` |

---

## `[diff]`

| Key | Values |
|-----|--------|
| `format` | `text`, `json` |
| `dialect` | `auto`, `postgres`, `mysql`, `sqlite` |

`diff` still requires the before/after inputs on the CLI. The config file supplies defaults for `--format` and `--dialect` only, and CLI flags override them when provided.

---

## Authoritative reference

The TOML schema and merge rules are defined in code:

- `crates/relune-cli/src/config.rs` — structure, load, merge  
- `fixtures/config/*.toml` — examples used in tests  
