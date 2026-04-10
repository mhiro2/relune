//! Shared catalog fetching helpers.

use crate::common::{RawColumn, RawEnum, RawForeignKey, RawIndex, RawSchema, RawTable, RawView};
use crate::error::IntrospectError;

/// Builds a [`RawSchema`] from fetched catalog sections.
#[must_use]
pub(crate) const fn raw_schema(
    tables: Vec<RawTable>,
    columns: Vec<RawColumn>,
    foreign_keys: Vec<RawForeignKey>,
    indexes: Vec<RawIndex>,
    views: Vec<RawView>,
    enums: Vec<RawEnum>,
) -> RawSchema {
    RawSchema {
        tables,
        columns,
        foreign_keys,
        indexes,
        views,
        enums,
    }
}

/// Fetches shared catalog sections in parallel for dialects with global queries per section.
pub(crate) trait ParallelCatalogReader {
    /// Fetches all user tables.
    async fn fetch_tables(&self) -> Result<Vec<RawTable>, IntrospectError>;

    /// Fetches all user columns.
    async fn fetch_columns(&self) -> Result<Vec<RawColumn>, IntrospectError>;

    /// Fetches all foreign keys.
    async fn fetch_foreign_keys(&self) -> Result<Vec<RawForeignKey>, IntrospectError>;

    /// Fetches all indexes.
    async fn fetch_indexes(&self) -> Result<Vec<RawIndex>, IntrospectError>;

    /// Fetches all views.
    async fn fetch_views(&self) -> Result<Vec<RawView>, IntrospectError>;

    /// Fetches all enum-like definitions.
    async fn fetch_enums(&self) -> Result<Vec<RawEnum>, IntrospectError>;

    /// Fetches all catalog sections concurrently and assembles a [`RawSchema`].
    async fn fetch_all(&self) -> Result<RawSchema, IntrospectError> {
        let (tables, columns, foreign_keys, indexes, views, enums) = tokio::try_join!(
            self.fetch_tables(),
            self.fetch_columns(),
            self.fetch_foreign_keys(),
            self.fetch_indexes(),
            self.fetch_views(),
            self.fetch_enums()
        )?;

        Ok(raw_schema(
            tables,
            columns,
            foreign_keys,
            indexes,
            views,
            enums,
        ))
    }
}
