//! Typed query expressions shared by Flux's ECS and database executors.
//!
//! A query expression describes a record type, its portable predicate, and the
//! database representation of the same predicate. Construct expressions with the
//! `query!` and `query_one!` macros, then choose an executor explicitly with
//! `Commands::query`/`query_one` or `Commands::db_query`/`db_query_one`.

use std::marker::PhantomData;

use crate::prelude::*;

/// Cardinality marker for an expression that may return zero or more records.
#[derive(Debug, Clone, Copy, Default)]
pub struct QueryMany;

/// Cardinality marker for an expression that returns at most one record.
#[derive(Debug, Clone, Copy, Default)]
pub struct QueryOne;

/// A typed query expression that can be interpreted by multiple backends.
///
/// `F` is the Bevy ECS filter used by ECS execution. Portable expressions use
/// the default `()` filter; ECS-only expressions may set it to `With<T>`, `Without<T>`,
/// `Changed<T>`, or a tuple of Bevy query filters.
pub struct QueryExpr<T, C, V, P, F = ()> {
    pub(crate) statement: String,
    pub(crate) variables: V,
    pub(crate) predicate: P,
    pub(crate) limit: Option<usize>,
    pub(crate) marker: PhantomData<fn() -> (T, C, F)>,
}

impl<T, C, V, P, F> QueryExpr<T, C, V, P, F> {
    pub fn new(statement: String, variables: V, predicate: P, limit: Option<usize>) -> Self {
        Self {
            statement,
            variables,
            predicate,
            limit,
            marker: PhantomData,
        }
    }
}

/// Return the database table name used by Flux for a record type.
pub fn record_table<T: FluxRecord>() -> &'static str {
    T::short_type_path()
}