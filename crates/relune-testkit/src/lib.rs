//! Test utilities for relune.
//!
//! Provides builder patterns for constructing test fixtures and
//! utility functions for test assertions.

use relune_core::{
    Column, ColumnId, Enum, ForeignKey, Index, ReferentialAction, Schema, Table, TableId, View,
};

/// Normalizes SVG content by trimming whitespace and removing empty lines.
pub fn normalize_svg(svg: &str) -> String {
    svg.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Builder for constructing [`Schema`] test fixtures.
#[derive(Default)]
pub struct SchemaBuilder {
    tables: Vec<Table>,
    views: Vec<View>,
    enums: Vec<Enum>,
    next_table_id: u64,
}

impl SchemaBuilder {
    /// Creates a new empty schema builder.
    #[must_use]
    pub fn new() -> Self {
        Self {
            next_table_id: 1,
            ..Default::default()
        }
    }

    /// Adds a table using a [`TableBuilder`].
    #[must_use]
    pub fn table(mut self, name: &str, f: impl FnOnce(TableBuilder) -> TableBuilder) -> Self {
        let id = TableId(self.next_table_id);
        self.next_table_id += 1;
        let builder = f(TableBuilder::new(id, name));
        self.tables.push(builder.build());
        self
    }

    /// Adds a view.
    #[must_use]
    pub fn view(mut self, name: &str, columns: Vec<Column>, definition: Option<&str>) -> Self {
        self.views.push(View {
            id: name.to_string(),
            schema_name: None,
            name: name.to_string(),
            columns,
            definition: definition.map(String::from),
        });
        self
    }

    /// Adds an enum type.
    #[must_use]
    pub fn enum_type(mut self, name: &str, values: &[&str]) -> Self {
        self.enums.push(Enum {
            id: name.to_string(),
            schema_name: None,
            name: name.to_string(),
            values: values.iter().map(|v| (*v).to_string()).collect(),
        });
        self
    }

    /// Builds the schema.
    #[must_use]
    pub fn build(self) -> Schema {
        Schema {
            tables: self.tables,
            views: self.views,
            enums: self.enums,
        }
    }
}

/// Builder for constructing [`Table`] test fixtures.
pub struct TableBuilder {
    table: Table,
    next_column_id: u64,
}

impl TableBuilder {
    /// Creates a new table builder.
    #[must_use]
    pub fn new(id: TableId, name: &str) -> Self {
        Self {
            table: Table {
                id,
                stable_id: name.to_string(),
                schema_name: None,
                name: name.to_string(),
                columns: Vec::new(),
                foreign_keys: Vec::new(),
                indexes: Vec::new(),
                comment: None,
            },
            next_column_id: 1,
        }
    }

    /// Adds a column with the given name and data type.
    #[must_use]
    pub fn column(mut self, name: &str, data_type: &str) -> Self {
        self.table.columns.push(Column {
            id: ColumnId(self.next_column_id),
            name: name.to_string(),
            data_type: data_type.to_string(),
            nullable: true,
            is_primary_key: false,
            comment: None,
        });
        self.next_column_id += 1;
        self
    }

    /// Adds a primary key column.
    #[must_use]
    pub fn pk(mut self, name: &str, data_type: &str) -> Self {
        self.table.columns.push(Column {
            id: ColumnId(self.next_column_id),
            name: name.to_string(),
            data_type: data_type.to_string(),
            nullable: false,
            is_primary_key: true,
            comment: None,
        });
        self.next_column_id += 1;
        self
    }

    /// Adds a foreign key to another table.
    #[must_use]
    pub fn fk(mut self, to_table: &str, from_columns: &[&str], to_columns: &[&str]) -> Self {
        self.table.foreign_keys.push(ForeignKey {
            name: None,
            from_columns: from_columns.iter().map(|c| (*c).to_string()).collect(),
            to_schema: None,
            to_table: to_table.to_string(),
            to_columns: to_columns.iter().map(|c| (*c).to_string()).collect(),
            on_delete: ReferentialAction::NoAction,
            on_update: ReferentialAction::NoAction,
        });
        self
    }

    /// Adds a named foreign key.
    #[must_use]
    pub fn named_fk(
        mut self,
        name: &str,
        to_table: &str,
        from_columns: &[&str],
        to_columns: &[&str],
    ) -> Self {
        self.table.foreign_keys.push(ForeignKey {
            name: Some(name.to_string()),
            from_columns: from_columns.iter().map(|c| (*c).to_string()).collect(),
            to_schema: None,
            to_table: to_table.to_string(),
            to_columns: to_columns.iter().map(|c| (*c).to_string()).collect(),
            on_delete: ReferentialAction::NoAction,
            on_update: ReferentialAction::NoAction,
        });
        self
    }

    /// Adds an index.
    #[must_use]
    pub fn index(mut self, name: Option<&str>, columns: &[&str], is_unique: bool) -> Self {
        self.table.indexes.push(Index {
            name: name.map(String::from),
            columns: columns.iter().map(|c| (*c).to_string()).collect(),
            is_unique,
        });
        self
    }

    /// Sets the schema name.
    #[must_use]
    pub fn schema(mut self, schema_name: &str) -> Self {
        self.table.schema_name = Some(schema_name.to_string());
        let qualified = format!("{}.{}", schema_name, self.table.name);
        self.table.stable_id = qualified;
        self
    }

    /// Builds the table.
    #[must_use]
    pub fn build(self) -> Table {
        self.table
    }
}

/// Creates a simple blog schema for testing.
///
/// Contains: users, posts, comments tables with FK relationships.
#[must_use]
pub fn blog_schema() -> Schema {
    SchemaBuilder::new()
        .table("users", |t| {
            t.pk("id", "integer")
                .column("name", "text")
                .column("email", "text")
        })
        .table("posts", |t| {
            t.pk("id", "integer")
                .column("title", "text")
                .column("body", "text")
                .column("author_id", "integer")
                .fk("users", &["author_id"], &["id"])
        })
        .table("comments", |t| {
            t.pk("id", "integer")
                .column("body", "text")
                .column("post_id", "integer")
                .column("user_id", "integer")
                .fk("posts", &["post_id"], &["id"])
                .fk("users", &["user_id"], &["id"])
        })
        .build()
}

/// Creates a minimal schema with a single table.
#[must_use]
pub fn single_table_schema(name: &str) -> Schema {
    SchemaBuilder::new()
        .table(name, |t| t.pk("id", "integer"))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blog_schema() {
        let schema = blog_schema();
        assert_eq!(schema.tables.len(), 3);
        assert_eq!(schema.tables[0].name, "users");
        assert_eq!(schema.tables[1].name, "posts");
        assert_eq!(schema.tables[1].foreign_keys.len(), 1);
        assert_eq!(schema.tables[2].name, "comments");
        assert_eq!(schema.tables[2].foreign_keys.len(), 2);
    }

    #[test]
    fn test_schema_builder() {
        let schema = SchemaBuilder::new()
            .table("t1", |t| t.pk("id", "int"))
            .enum_type("status", &["active", "inactive"])
            .build();
        assert_eq!(schema.tables.len(), 1);
        assert_eq!(schema.enums.len(), 1);
        assert_eq!(schema.enums[0].values, vec!["active", "inactive"]);
    }
}
