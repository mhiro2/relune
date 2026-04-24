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
viewpoint = "billing"
group_by = "none"
focus = "orders"
depth = 2
include = ["users", "orders"]
exclude = ["schema_migrations"]

[inspect]
format = "text"
fail_on_warning = false

[export]
format = "schema-json"
viewpoint = "billing"
group_by = "schema"
layout = "hierarchical"
edge_style = "curved"
direction = "top-to-bottom"
focus = "orders"
depth = 1
include = ["users", "orders"]
exclude = ["schema_migrations"]
fail_on_warning = false

[doc]
fail_on_warning = false

[lint]
dialect = "postgres"
deny = "warning"
fail_on_warning = false

[diff]
format = "json"
dialect = "postgres"
fail_on_warning = false

[viewpoints.billing]
focus = "orders"
depth = 2
group_by = "schema"
include = ["users", "orders", "payments"]
exclude = ["audit_*"]
```

```bash
relune --config relune.toml render --sql schema.sql -o erd.svg
```

---

## `[render]`

| Key | Values |
|-----|--------|
| `format` | `svg`, `html`, `graph-json`, `schema-json` |
| `dialect` | `auto`, `postgres`, `mysql`, `sqlite` |
| `theme` | `light`, `dark` |
| `layout` | `hierarchical`, `force-directed` |
| `edge_style` | `straight`, `orthogonal`, `curved` |
| `direction` | `top-to-bottom`, `left-to-right`, `right-to-left`, `bottom-to-top` |
| `viewpoint` | Name from `[viewpoints.<name>]` |
| `group_by` | `none`, `schema`, `prefix` |
| `focus` | Table name |
| `depth` | Unsigned integer |
| `include` / `exclude` | String arrays |
| `show_legend`, `show_stats` | Booleans; `--stats` on the CLI forces `show_stats` only |
| `fail_on_warning` | Boolean; treat warning diagnostics as failures |

`layout`, `edge_style`, and `direction` can be set in the file and overridden with CLI flags. See `ReluneConfig::merge_render_args` in `crates/relune-cli/src/config.rs` for exact precedence.

Semantic validation is also applied after merge:

- `depth` must be at least `1`
- `depth` can only be set when `focus` is present
- table names in `focus`, `include`, and `exclude` must be non-empty and must not have surrounding whitespace
- the same table cannot appear in both `include` and `exclude`
- if `include` is non-empty, it must contain the focused table
- the focused table cannot also appear in `exclude`

If `viewpoint` is set, Relune applies the selected named preset before CLI flags. The precedence for view-related settings is:

1. CLI flags such as `--viewpoint`, `--focus`, `--include`
2. Selected `[viewpoints.<name>]`
3. Command defaults in `[render]`

---

## `[inspect]`

| Key | Values |
|-----|--------|
| `format` | `text`, `json` |
| `dialect` | `auto`, `postgres`, `mysql`, `sqlite` |
| `fail_on_warning` | Boolean; treat warning diagnostics as failures |

---

## `[export]`

| Key | Values |
|-----|--------|
| `format` | `schema-json`, `graph-json`, `layout-json`, `mermaid`, `d2`, `dot` |
| `dialect` | `auto`, `postgres`, `mysql`, `sqlite` |
| `viewpoint` | Name from `[viewpoints.<name>]` |
| `group_by` | `none`, `schema`, `prefix` |
| `layout` | `hierarchical`, `force-directed` |
| `edge_style` | `straight`, `orthogonal`, `curved` |
| `direction` | `top-to-bottom`, `left-to-right`, `right-to-left`, `bottom-to-top` |
| `focus`, `depth` | Same as CLI |
| `include` / `exclude` | Same as CLI |
| `fail_on_warning` | Boolean; treat warning diagnostics as failures |

`export.format` can be set in the config file and overridden with `--format`. If neither config nor CLI provides a format, the command fails fast. As with `render`, `export.depth` requires `export.focus`, and focused table names must be non-empty after trimming.

`export.viewpoint` uses the same precedence rule as `render`: CLI flags override the selected viewpoint, and the selected viewpoint overrides plain `[export]` focus/filter/grouping defaults.

---

## `[viewpoints.<name>]`

Named viewpoints let you reuse the same focus, filter, and grouping rules across `render` and `export`.

| Key | Values |
|-----|--------|
| `group_by` | `none`, `schema`, `prefix` |
| `focus` | Table name |
| `depth` | Unsigned integer |
| `include` / `exclude` | String arrays |

Use them with `render.viewpoint`, `export.viewpoint`, `relune render --viewpoint <NAME>`, or `relune export --viewpoint <NAME>`.

---

## `[doc]`

| Key | Values |
|-----|--------|
| `dialect` | `auto`, `postgres`, `mysql`, `sqlite` |
| `fail_on_warning` | Boolean; treat warning diagnostics as failures |

---

## `[lint]`

| Key | Values |
|-----|--------|
| `format` | `text`, `json` |
| `dialect` | `auto`, `postgres`, `mysql`, `sqlite` |
| `profile` | `default`, `strict` |
| `rules` | Array of kebab-case rule IDs to run instead of the profile defaults |
| `exclude_rules` | Array of kebab-case rule IDs to remove from the active set |
| `categories` | Array of `structure`, `relationships`, `naming`, `documentation` |
| `except_tables` | Array of table patterns to suppress from the report |
| `deny` | `error`, `warning`, `info`, `hint` — minimum severity for a non-zero exit when not overridden by `--deny` |
| `fail_on_warning` | Boolean; treat warning diagnostics as failures when `deny` is unset |

`default` is the balanced schema review profile. `strict` adds full column comment coverage checks. CLI flags override config values when provided, and array settings follow the same rule: if you pass any CLI values for `rules`, `exclude_rules`, `categories`, or `except_tables`, those values replace the config list for that key.

---

## `[diff]`

| Key | Values |
|-----|--------|
| `format` | `text`, `json`, `markdown`, `svg`, `html` |
| `dialect` | `auto`, `postgres`, `mysql`, `sqlite` |
| `fail_on_warning` | Boolean; treat warning diagnostics as failures |

`diff` still requires the before/after inputs on the CLI. The config file supplies defaults for `--format`, `--dialect`, and `--fail-on-warning`, and CLI flags override them when provided. File-based `diff` inputs are detected by content, so schema JSON copied to a non-`.json` filename is still treated as schema JSON.

---

## Authoritative reference

The TOML schema and merge rules are defined in code:

- `crates/relune-cli/src/config.rs` — structure, load, merge  
- `fixtures/config/*.toml` — examples used in tests  
