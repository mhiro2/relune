# Architecture

How Relune is structured: crate boundaries, data flow, and rules for keeping the CLI and WASM targets aligned.

---

## Table of contents

1. [Goals and constraints](#1-goals-and-constraints)
2. [Layers and crates](#2-layers-and-crates)
3. [Request pipeline](#3-request-pipeline)
4. [Domain model (core)](#4-domain-model-core)
5. [Dependency rules](#5-dependency-rules)
6. [Input adapters](#6-input-adapters)
7. [Output adapters](#7-output-adapters)
8. [Configuration](#8-configuration)
9. [Diagnostics](#9-diagnostics)
10. [Layout](#10-layout)
11. [Rendering](#11-rendering)
12. [WASM boundary](#12-wasm-boundary)
13. [CLI](#13-cli)
14. [Security notes](#14-security-notes)
15. [Product evolution](#15-product-evolution)
16. [Checklist for new work](#16-checklist-for-new-work)

---

## 1. Goals and constraints

Relune is a **reusable schema graph engine** with multiple delivery surfaces (CLI, WASM).

**Central constraint:** domain and pipeline logic must stay **target-agnostic**. No `std::fs` in core crates, no `wasm-bindgen` below the WASM crate, no ad-hoc DB drivers outside introspection. This leads to three design rules:

1. **Explicit intermediate models** — schema → graph → layout → render, each testable in isolation
2. **Thin surfaces** — CLI and WASM deserialize requests, call `relune-app`, and serialize results
3. **DTO-style boundaries** — public APIs expose Relune-owned types, not parser ASTs or `petgraph` internals

---

## 2. Layers and crates

```text
┌──────────────────────────────────────────────────────────┐
│ Surfaces                                                 │
│   relune-cli              relune-wasm                    │
└────────────────────────────┬─────────────────────────────┘
                             │
                             ▼
┌──────────────────────────────────────────────────────────┐
│ Application                                              │
│   relune-app — validation, config merge, orchestration   │
└────────────────────────────┬─────────────────────────────┘
                             │
         ┌───────────────────┼───────────────────┐
         ▼                   ▼                   ▼
┌──────────────────┐ ┌──────────────────┐ ┌──────────────────┐
│ Domain / logic   │ │ Input            │ │ Output           │
│ relune-core      │ │ relune-parser-   │ │ relune-render-   │
│ relune-layout    │ │   sql            │ │   theme          │
│                  │ │ relune-introspect│ │ relune-render-   │
│                  │ │                  │ │   svg / html     │
└──────────────────┘ └──────────────────┘ └──────────────────┘
```

| Crate | Role |
|-------|------|
| `relune-core` | Normalized schema model, graph construction, filters, lint, diff, shared types |
| `relune-layout` | Hierarchical and force-directed layout, edge routing, overlay annotations, text diagram export (Mermaid, D2, DOT) |
| `relune-parser-sql` | DDL → `Schema` (PostgreSQL, MySQL, SQLite; auto-detection) |
| `relune-introspect` | Live DB metadata → `Schema` (PostgreSQL, MySQL/MariaDB, SQLite; native builds only) |
| `relune-render-theme` | Shared theme palette and render-facing theme DTOs used by SVG and HTML renderers |
| `relune-render-svg` | Layout → SVG string |
| `relune-render-html` | Layout → self-contained HTML + embedded SVG + viewer scripts |
| `relune-app` | Use-cases: parse/introspect, render, doc, export, lint, diff wiring |
| `relune-cli` | Args, config TOML, stdin/stdout/files, exit codes |
| `relune-wasm` | `wasm-bindgen` façade, JSON in/out |
| `relune-testkit` | Shared test helpers (tests only) |

Repository layout (abbreviated):

```text
crates/
  relune-core/ relune-layout/ relune-parser-sql/ relune-introspect/
  relune-render-theme/ relune-render-svg/ relune-render-html/
  relune-app/ relune-cli/ relune-wasm/ relune-testkit/
fixtures/          # golden inputs and snapshots
docs/              # user-facing guides
```

---

## 3. Request pipeline

### Native CLI

```text
SQL file | SQL text | schema JSON | db URL  +  optional relune.toml
    → relune-cli (I/O, load config)
    → relune-app (choose adapter, build pipeline)
    → Schema → graph → layout → (+ optional overlay) → SVG | HTML | Markdown | JSON | diagram text
    → file or stdout
```

### WASM

```text
SQL text | schema JSON | options (from JS)
    → relune-wasm
    → relune-app (same pipeline where applicable)
    → string/JSON result to JS
```

Introspection and filesystem access stay on the **native** side; WASM uses in-memory inputs.

---

## 4. Domain model (core)

Types live in `relune-core` (see `model.rs`, `graph.rs`, and related modules).

**`Schema`** — Top-level container: `tables`, `views`, `enums`. Supports `validate()` for structural consistency (duplicate names, FK column references, etc.).

**`Table`** — `TableId`, `stable_id`, optional `schema_name`, `name`, `columns`, `foreign_keys`, `indexes`, optional `comment`.

**`Column`** — `ColumnId`, `name`, `data_type`, `nullable`, `is_primary_key`, optional `comment`.

**`ForeignKey`** — Optional constraint `name`, `from_columns`, `to_table`, `to_columns`, `on_delete` / `on_update` (`ReferentialAction`).

**`View`** — Parsed and introspected across all three dialects. Stored with the original SQL definition.

**`Enum`** — PostgreSQL uses named enum types (`CREATE TYPE ... AS ENUM`). MySQL has no schema-level enum type, but live introspection lifts `ENUM(...)` / `SET(...)` column definitions into `Schema.enums` so they can participate in graphing and diffs. SQLite does not contribute enum metadata.

**Derived artifacts** flow through the pipeline:

- **Graph** — nodes and edges with stable identities (input to layout)
- **Positioned graph** — coordinates and edge paths (output of layout)
- **Render primitives** — boxes, paths, labels, grouping (consumed by SVG/HTML renderers)

---

## 5. Dependency rules

```text
relune-cli  ──► relune-app ──► relune-core
                  │    ├── relune-layout
                  │    ├── relune-parser-sql
                  │    ├── relune-introspect   (native)
                  │    ├── relune-render-theme
                  │    ├── relune-render-svg
                  │    └── relune-render-html
relune-wasm ───► relune-app
```

- **`relune-core`** must not depend on CLI, WASM, renderers, or parsers.
- **`relune-layout`** depends on `relune-core` (not the reverse).
- **`relune-render-theme`** is the shared palette layer for renderers.
- **`relune-render-*`** may depend on `relune-core`, layout outputs, and `relune-render-theme`.
- **`relune-app`** composes adapters; avoid duplicating domain rules that belong in core or layout.
- **`relune-testkit`** is for tests; it must not become a default production dependency of shipped crates.

---

## 6. Input adapters

Supported paths into a `Schema`:

| Source | Crate / module |
|--------|----------------|
| SQL DDL string or file | `relune-parser-sql` |
| Normalized schema JSON | Deserialized directly into `relune-core` types |
| Live database | `relune-introspect` (PostgreSQL, MySQL/MariaDB, SQLite) |

`relune-app` selects the adapter from the request (CLI or WASM DTO). Parsing is **pure text**; introspection uses **read-only** metadata queries. PostgreSQL/MySQL/MariaDB introspection applies a default 30 second statement deadline, and remote TCP connections require TLS by default. Native file-backed SQL and schema JSON inputs are size-limited before reading.

---

## 7. Output adapters

| Output | Producer |
|--------|----------|
| Shared palettes / theme DTOs | `relune-render-theme` |
| SVG | `relune-render-svg` |
| Self-contained HTML | `relune-render-html` |
| Markdown documentation | `relune-app` (doc use-case, schema → Markdown) |
| `schema-json` / `graph-json` / `layout-json` | Core + layout serialization |
| Mermaid `erDiagram`, D2, Graphviz DOT | `relune-layout` (text from the same positioned graph) |

---

## 8. Configuration

CLI merges **defaults → TOML file → flags** for command settings (`render`, `inspect`, `doc`, `export`, `lint`, `diff`). Implementation: `crates/relune-cli/src/config.rs`. Required inputs still come from the CLI. Named `[viewpoints.<name>]` presets provide reusable focus/filter/grouping bundles for `render` and `export`, and are applied between command defaults and explicit CLI flags. After merge, render/export apply semantic validation for focus depth and filter combinations, and diff file inputs are classified by content instead of extension alone.

---

## 9. Diagnostics

Diagnostics are a first-class stream: parse errors, recoverable warnings, unsupported DDL, layout notices, and lint findings. Each carries a **stable code** and severity suitable for CI (`--fail-on-warning`, `--deny`). Partial success (warnings + output) is preferred over hard failure for exploratory use.

`LintIssue` separates **stable identifiers** (`table_id` — matches `Table::stable_id`) from **display names** (`table_name` — human-readable, schema-qualified). Renderers and overlay builders use `table_id` to map issues to diagram nodes without ambiguity in multi-schema environments; CLI and text output use `table_name` for readability.

---

## 10. Layout

`relune-layout` owns graph layout, overlay annotations, and text diagram exports (Mermaid, D2, DOT). It provides hierarchical and force-directed node placement plus orthogonal backbones for routed edges; renderers can display those routes as orthogonal or curved paths, while `straight` edges are emitted as direct source-to-target segments. Force-directed mode still uses rank-guided hierarchical seeding to keep requested flow directions stable, then mirrors/swaps the final placement for reversed or horizontal directions without changing the user-facing spacing semantics. Separating it from `relune-core` keeps a clear boundary between the semantic graph and geometry, and allows targeted benchmarks.

Phases: build layout graph → grouping/focus → layout algorithm → coordinates → **auto-tune spacing** → **global port assignment** → **obstacle-aware channel selection** → **parallel edge bundling** → **self-loop detour handling** → **label collision avoidance** → bounds. Handles cycles, join tables, views, enum references, and multi-schema namespacing.

**Routing model** — Layout returns one canonical orthogonal backbone for routed edges. Hierarchical routing uses `port -> stub -> channel -> stub -> port`, then scores inter-rank, same-rank, and reverse-flow channel candidates in a deterministic greedy edge order. Candidate scoring treats obstacle hits, endpoint-side violations, and primary-direction backtracking against rank order as hard constraints before weighted soft costs for clearance, route length, bend count, center deviation, and channel congestion. After route selection, nearby parallel edges on the same channel may share a bundled trunk for readability. `orthogonal` and `curved` reuse this backbone, while `straight` skips control points and renders as a direct source-to-target segment.

**Quality passes** — After backbone routing, `nudge_label` shifts edge labels away from overlapping nodes. `detour_around_obstacles` is no longer part of the non-self-loop path and is only used for self-loop handling, while routing keeps a detour activation count for any non-self-loop edge whose final backbone still intersects padded obstacles. Additionally, `auto_tuned` adjusts horizontal/vertical spacing based on node count and edge density before coordinate assignment, and port slot offsets keep parallel edges stable on each node side. `layout-json` exposes this routing state through graph-level and per-edge `routing_debug` metadata so fixture diffs can explain side policy, slot assignment, and selected channel coordinates directly.

Fixture-level routing regressions are audited in
`crates/relune-app/tests/fixture_render_audit.rs`, which snapshots `layout-json`
and rendered outputs across the main SQL fixtures.

**Overlay** (`overlay` module) — A `DiagramOverlay` attaches annotations (lint warnings, diff status, etc.) to nodes and edges by stable ID, without modifying the positioned graph itself. Renderers accept an optional overlay and apply visual cues (badges, border colors, tooltips) when present. When no overlay is provided the diagram renders normally.

---

## 11. Rendering

- **Theme** (`relune-render-theme`) — Shared palettes and theme-facing DTOs consumed by both renderers. `ThemeColors` includes `glow_color` and `glow_particle` fields so hover/highlight effects adapt to light and dark themes.
- **SVG** (`relune-render-svg`) — Geometry, edge paths, labels, themes, optional embedded CSS. Tables, views, and enums share one positioned graph and are styled by node/edge kind. When a `DiagramOverlay` is provided, the renderer applies severity-colored borders and stroke overrides on affected nodes/edges, adds count badges at the top-right corner of annotated nodes, appends overlay annotation details to `<title>` tooltips, and adds CSS classes (`overlay-error`, `overlay-warning`, etc.) for downstream styling. Visual conventions: header-to-body transition uses a per-node gradient fade; column metadata (PK, FK, IX) is rendered as uniform rounded-rect badges; edge arrow uses an open-chevron marker with `userSpaceOnUse` for constant size; cardinality markers use enlarged viewBoxes for density resilience.
- **HTML** (`relune-render-html`) — Wraps SVG with interactive behavior (pan/zoom, search, filters, grouping toggles, highlights) and embeds node/edge kind metadata for client-side features. Hover uses a lightweight popover plus subtle 1-hop preview, while click promotes a node into fixed selection and opens the detail drawer. When a `DiagramOverlay` is provided, annotations are serialized into the `issues` field of table and edge metadata (JSON), the detail drawer gains a "Health" section listing each issue with severity badge and optional hint, and the object browser displays severity-indicator badges with issue counts on affected tables. Viewer logic is TypeScript under `crates/relune-render-html/ts/`; bundled JS is committed under `crates/relune-render-html/src/js/` and consumed via `include_str!`. Node + pnpm are required for renderer development when regenerating those bundles, but Rust builds consume the committed assets without installing frontend dependencies at build time.

The two crates are separate to keep low-level vector output apart from document bundling and JS tooling.

---

## 12. WASM boundary

- Export a **small, stable** API surface (prefer request/response JSON or a few entrypoints).
- No DB networking or filesystem in the WASM graph path.
- Deserialize into the same DTOs `relune-app` uses on native.
- The public GitHub Pages playground is a thin static client over `relune-wasm`; it must not fork rendering logic from the CLI path.

---

## 13. CLI

`relune-cli` should stay **thin**: argument parsing, config load, reading inputs, calling `relune-app`, writing outputs, mapping errors to exit codes. Parsing, layout, and rendering belong in other crates.

---

## 14. Security notes

- **SQL DDL mode** — Parsing only; never executes SQL.
- **Introspection** — Read-only metadata; document required DB privileges.
- **HTML** — Self-contained output; escape untrusted names in SVG/HTML layers (maintain parity when adding fields).

---

## 15. Product evolution

```text
ERD generator → schema explorer → diff/lint in CI → editor integrations
```

The explicit intermediate models and crate boundaries exist to support this path without rewriting the core.

---

## 16. Checklist for new work

- Does it keep **core logic target-agnostic**?
- Is it **deterministically testable** (fixtures, snapshots)?
- Are **public types** Relune-owned, not leaked third-party internals?
- If it cannot run on WASM, is it **isolated** (e.g. behind `relune-introspect` / CLI only)?
- Does **business logic** land in `relune-core` / `relune-layout` rather than the CLI?
- Does it help users **understand large schemas** (focus, grouping, stable exports), not only “more pixels”?
