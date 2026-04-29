# dogfood

Canonical "application schema" used to verify the
[`mhiro2/relune/action`](../action/README.md) review workflow against this
repository itself.

[`schema.sql`](schema.sql) is treated as the live schema. Every pull request
that touches it triggers
[`.github/workflows/dogfood-review.yaml`](../.github/workflows/dogfood-review.yaml),
which builds `relune-cli`, runs `relune review` against the base ref, and
posts the report as a sticky PR comment.

The directory is **only** used for end-to-end verification of the GitHub
Action — it is not consumed by any crate, the CLI, or the playground.
