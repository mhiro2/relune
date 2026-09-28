//! Deterministic generator for large, realistically shaped schemas, shared
//! by the large-schema regression tests and the layout benchmark.

use relune_core::{
    Column, ColumnId, ColumnSemantics, ForeignKey, ReferentialAction, Schema, Table, TableId,
};

/// Minimal linear congruential generator, so the same `seed` always yields
/// the same schema without pulling in a random-number crate.
struct Lcg(u64);

impl Lcg {
    const fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    /// Returns a value in `0..bound`.
    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next()).expect("u31 fits in usize") % bound
    }
}

const SCHEMAS: [&str; 4] = ["public", "billing", "auth", "analytics"];

/// Builds `table_count` tables spread over a few schemas. Tables form
/// modules of related tables around local hubs, with a handful of global
/// hubs every module references, occasional reference cycles, and about one
/// table in eight without foreign keys — the mix seen in large
/// application schemas.
pub fn synthetic_schema(table_count: usize, seed: u64) -> Schema {
    let mut rng = Lcg(seed);
    let name = |index: usize| format!("t{index:04}");
    let schema_of = |index: usize| SCHEMAS[index / 64 % SCHEMAS.len()];
    let hubs = table_count.min(4);

    let tables = (0..table_count)
        .map(|index| {
            let column = |id: usize, name: String, is_primary_key: bool| Column {
                id: ColumnId(u64::try_from(id).expect("column id fits in u64")),
                name,
                data_type: if is_primary_key { "bigint" } else { "text" }.to_string(),
                nullable: !is_primary_key,
                is_primary_key,
                comment: None,
                enum_values: None,
                semantics: ColumnSemantics::default(),
            };
            let mut columns = vec![column(1, "id".to_string(), true)];
            for i in 0..2 + rng.below(8) {
                columns.push(column(columns.len() + 1, format!("attribute_{i}"), false));
            }

            let mut targets = Vec::new();
            if index >= hubs && rng.below(8) != 0 {
                let module_start = index - index % 16;
                // Most tables hang off their module's first table ...
                targets.push(if module_start == index {
                    rng.below(hubs)
                } else {
                    module_start
                });
                // ... some also reference a neighbour or a global hub ...
                if rng.below(3) == 0 {
                    targets.push(module_start + rng.below(index - module_start + 1));
                }
                if rng.below(4) == 0 {
                    targets.push(rng.below(hubs));
                }
                // ... and a few point forward, closing reference cycles.
                if rng.below(40) == 0 {
                    targets.push(rng.below(table_count));
                }
            }

            let foreign_keys = targets
                .into_iter()
                .enumerate()
                .map(|(i, target)| {
                    let from = format!("{}_{i}_id", name(target));
                    columns.push(column(columns.len() + 1, from.clone(), false));
                    ForeignKey {
                        name: None,
                        from_columns: vec![from],
                        to_schema: Some(schema_of(target).to_string()),
                        to_table: name(target),
                        to_columns: vec!["id".to_string()],
                        on_delete: ReferentialAction::NoAction,
                        on_update: ReferentialAction::NoAction,
                    }
                })
                .collect();

            Table {
                id: TableId(u64::try_from(index + 1).expect("table id fits in u64")),
                stable_id: format!("{}.{}", schema_of(index), name(index)),
                schema_name: Some(schema_of(index).to_string()),
                name: name(index),
                columns,
                foreign_keys,
                indexes: vec![],
                primary_key_name: None,
                check_constraints: Vec::new(),
                comment: None,
            }
        })
        .collect();

    Schema {
        tables,
        views: vec![],
        enums: vec![],
    }
}
