//! Typed query expressions shared by Flux's ECS and database executors.
//!
//! A query expression describes a record type, its portable predicate, and the
//! database representation of the same predicate. Construct expressions with the
//! `query!` and `query_one!` macros, then choose an executor explicitly with
//! `Commands::query`/`query_one` or `Commands::db_query`/`db_query_one`.

use std::marker::PhantomData;

use crate::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryComparison { Eq, Ne, Lt, Le, Gt, Ge }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryCondition {
    pub field: String,
    pub comparison: QueryComparison,
    pub parameter: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryPlan {
    pub conditions: Vec<QueryCondition>,
    pub limit: Option<usize>,
}

#[derive(Default)]
pub struct QueryBindings {
    pub(crate) values: std::collections::BTreeMap<String, serde_json::Value>,
    pub(crate) error: Option<String>,
}

impl QueryBindings {
    pub fn insert(&mut self, name: String, value: &impl serde::Serialize) {
        match serde_json::to_value(value) {
            Ok(value) => { self.values.insert(name, value); }
            Err(error) => { self.error = Some(error.to_string()); }
        }
    }
}

pub trait QueryParameters {
    fn into_parameters(self) -> anyhow::Result<std::collections::BTreeMap<String, serde_json::Value>>;
}

impl QueryParameters for QueryBindings {
    fn into_parameters(self) -> anyhow::Result<std::collections::BTreeMap<String, serde_json::Value>> {
        anyhow::ensure!(self.error.is_none(), "Query binding serialization failed: {}", self.error.unwrap_or_default());
        Ok(self.values)
    }
}

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
    pub(crate) plan: Option<QueryPlan>,
    pub(crate) variables: V,
    pub(crate) predicate: P,
    pub(crate) limit: Option<usize>,
    pub(crate) marker: PhantomData<fn() -> (T, C, F)>,
}

impl<T, C, V, P, F> QueryExpr<T, C, V, P, F> {
    pub fn new(plan: Option<QueryPlan>, variables: V, predicate: P, limit: Option<usize>) -> Self {
        Self {
            plan,
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
