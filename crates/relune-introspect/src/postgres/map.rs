//! Mapping module for converting raw `PostgreSQL` catalog data to `relune-core` `Schema` types.
//!
//! This module re-exports the common mapping functions shared across all
//! database backends.

pub use crate::common::map_to_schema;
