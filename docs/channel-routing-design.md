# Channel Routing Audit And Cost Design

This note fixes the candidate scope and scoring policy for obstacle-aware
channel selection. The current baseline is captured by
`crates/relune-app/tests/fixture_render_audit.rs`, which exports `layout-json`
and renders SVG/HTML for the main SQL fixtures with hierarchical,
top-to-bottom, orthogonal routing.

## Audit scope

- Fixtures:
  `simple_blog.sql`, `join_heavy.sql`, `cyclic_fk.sql`, `multi_schema.sql`,
  `ecommerce.sql`
- Surfaces:
  `layout-json`, SVG, HTML
- Snapshot source:
  `crates/relune-app/tests/snapshots/fixture_render_audit__*.snap`

## Fixture findings

- `simple_blog.sql`
  - Baseline case with no same-rank edge, reverse edge, or tight-clearance route.
  - One skip-level route (`comments[user_id] -> users[id]`) already stretches to
    `2.00x` the direct distance, so long-route cost must stay visible even in
    clean layouts.
- `join_heavy.sql`
  - Primary dense-routing fixture. The current baseline has 12 routes under
    `24px` clearance, with multiple tenant edges reaching `0px`.
  - Several long detours remain, including `tasks -> users` near `4.9x` and
    `projects -> tenants` near `3.8x`.
  - Parallel edges exist but stay small (`max_size = 2`), so the immediate
    Phase 4a focus is channel quality before bundling.
- `cyclic_fk.sql`
  - Main reverse-flow and same-rank stress fixture.
  - The current baseline has 4 reverse edges and 4 same-rank edges.
  - `organizations_ref[owner_user_id] -> users[id]` reaches 10 bends and
    `0px` clearance, so reverse-edge candidates must search beyond the first
    channel center.
- `multi_schema.sql`
  - No routing edges today, but it remains useful as a renderer smoke fixture
    for zero-edge layouts across `layout-json`, SVG, and HTML.
- `ecommerce.sql`
  - Medium-density fixture that still exposes one `0px` clearance edge and one
    `2.13x` detour (`addresses[customer_id] -> customers[id]`).
  - Includes one same-rank self-reference (`categories[parent_id] -> categories[id]`).

## Candidate generation

Candidate search stays local and deterministic. Phase 4a should not start with
global optimization.

- Inter-rank edges
  - Base candidate:
    the center of the rank gap already used by the simple channel router
  - Search offsets:
    `[0, -24, 24, -48, 48]`
  - Search area:
    only inside the source/target rank gap
- Same-rank edges
  - Base candidate:
    the corridor between the two node boxes on the perpendicular axis
  - Search offsets:
    `[0, -32, 32, -64, 64, -96, 96]`
  - Search area:
    only around the shared-rank corridor between the endpoint boxes
- Reverse edges
  - Base candidate:
    the nearest inter-rank corridor that still preserves the assigned endpoint
    sides
  - Search offsets:
    `[0, -24, 24, -48, 48, -72, 72]`
  - Search area:
    first outside the dense center, then progressively farther on the same axis

## Evaluation strategy

Phase 4a should score candidates edge by edge with deterministic greedy
selection in a stable edge order.

- Stable edge order
  - lower source rank
  - lower target rank
  - source node id
  - target node id
  - source column list
  - target column list
- Hard constraints
  - No non-endpoint node crossing
  - No violation of the assigned endpoint side policy
- Soft cost weights
  - clearance penalty: `16`
  - total length: `1`
  - bend penalty: `48`
  - center deviation: `2`
  - congestion penalty: `40`
- Tie-break order
  - fewer hard-constraint violations
  - lower weighted soft cost
  - lower clearance penalty
  - shorter total length
  - fewer bends
  - smaller center deviation
  - lower congestion penalty
  - lower stable input order

The same numbers are encoded in `crates/relune-layout/src/channel.rs`.

## Detour retirement rule

`detour_around_obstacles` started as a fallback until obstacle-aware channel
selection landed. The implementation now keeps detour out of the default
non-self-loop path, while preserving self-loop handling and an explicit debug
fallback via `RELUNE_ENABLE_DETOUR_FALLBACK`. After Phase 4a:

1. Measure detour activation for every non-self-loop edge across the fixture
   audit test set.
2. Remove the generic detour pass if the activation count is zero.
3. If any activation remains, restrict detour usage to self-loop handling or an
   explicit debug fallback path instead of the default routing path.

The removal is only valid after the fixture audit snapshots and geometry
invariants still pass.
