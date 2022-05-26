//! Fabrix SqlBuilder ADT

use std::str::FromStr;

use itertools::Itertools;
use serde::{Deserialize, Serialize};

use crate::{FieldInfo, Series, SqlError, SqlResult, Value, ValueType};

// ================================================================================================
// Schema
// ================================================================================================

/// Table Schema
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TableSchema {
    pub name: String,
    pub dtype: ValueType,
    pub is_nullable: bool,
}

impl From<FieldInfo> for TableSchema {
    fn from(fi: FieldInfo) -> Self {
        TableSchema {
            name: fi.name,
            dtype: fi.dtype,
            is_nullable: true,
        }
    }
}

// ================================================================================================
// Constraint
// ================================================================================================

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum ConstraintType {
    Check,
    NotNull,
    Unique,
    PrimaryKey,
    ForeignKey,
}

impl FromStr for ConstraintType {
    type Err = SqlError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "CHECK" => Ok(ConstraintType::Check),
            "NOT NULL" => Ok(ConstraintType::NotNull),
            "UNIQUE" => Ok(ConstraintType::Unique),
            "PRIMARY KEY" => Ok(ConstraintType::PrimaryKey),
            "FOREIGN KEY" => Ok(ConstraintType::ForeignKey),
            _ => Err(SqlError::new_common_error("invalid constraint type")),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TableConstraint {
    pub constraint_name: String,
    pub constraint_type: ConstraintType,
}

impl TableConstraint {
    pub fn new(constraint_name: String, constraint_type: ConstraintType) -> Self {
        TableConstraint {
            constraint_name,
            constraint_type,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ColumnConstraint {
    pub constraint_name: String,
    pub column_name: String,
}

impl ColumnConstraint {
    pub fn new(name: String, column: String) -> Self {
        ColumnConstraint {
            constraint_name: name,
            column_name: column,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ColumnIndex {
    pub index_name: String,
    pub column_name: String,
}

impl ColumnIndex {
    pub fn new(name: String, column: String) -> Self {
        ColumnIndex {
            index_name: name,
            column_name: column,
        }
    }
}

// ================================================================================================
// Order
// ================================================================================================

/// order type
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Order {
    Asc(String),
    Desc(String),
}

impl Order {
    pub fn name(&self) -> &str {
        match self {
            Order::Asc(name) => name,
            Order::Desc(name) => name,
        }
    }
}

// ================================================================================================
// Index
// ================================================================================================

/// index with its' unique name, table belonged, and related index/ indices
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Index {
    pub name: String,
    pub table: String,
    pub columns: Vec<Order>,
}

// ================================================================================================
// Foreign Key
// ================================================================================================

/// foreign key direction
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ForeignKeyDir {
    pub table: String,
    pub column: String,
}

/// foreign key action
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum ForeignKeyAction {
    Restrict,
    Cascade,
    SetNull,
    NoAction,
    SetDefault,
}

impl Default for ForeignKeyAction {
    fn default() -> Self {
        ForeignKeyAction::NoAction
    }
}

/// foreign key with its' unique name, from & to table relations, and actions
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ForeignKey {
    pub name: String,
    pub from: ForeignKeyDir,
    pub to: ForeignKeyDir,
    pub on_delete: ForeignKeyAction,
    pub on_update: ForeignKeyAction,
}

// ================================================================================================
// Column
// ================================================================================================

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]

pub struct NameAlias {
    pub from: String,
    pub to: String,
}

/// column name, can be alias. used it in `select`
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(untagged)]
pub enum ColumnAlias {
    Simple(String),
    Alias(String, String), // (from, to)
}

impl ColumnAlias {
    pub fn original_name(&self) -> String {
        match self {
            ColumnAlias::Simple(s) => s.to_owned(),
            ColumnAlias::Alias(s, _) => s.to_owned(),
        }
    }

    pub fn name(&self) -> String {
        match self {
            ColumnAlias::Simple(s) => s.to_owned(),
            ColumnAlias::Alias(_, s) => s.to_owned(),
        }
    }
}

impl From<&str> for ColumnAlias {
    fn from(s: &str) -> Self {
        ColumnAlias::Simple(s.to_owned())
    }
}

impl From<(&str, &str)> for ColumnAlias {
    fn from((from, to): (&str, &str)) -> Self {
        ColumnAlias::Alias(from.to_owned(), to.to_owned())
    }
}

// ================================================================================================
// AlterTable
// ================================================================================================

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum AlterTable {
    Add {
        table: String,
        column: String,
        dtype: ValueType,
        is_nullable: bool,
    },
    Delete {
        table: String,
        column: String,
    },
    Modify {
        table: String,
        column: String,
        dtype: ValueType,
        is_nullable: bool,
    },
}

// ================================================================================================
// Expression & Expressions (filter)
// ================================================================================================

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Conjunction {
    AND,
    OR,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Equation {
    Not,
    Equal(Value),
    NotEqual(Value),
    Greater(Value),
    GreaterEqual(Value),
    Less(Value),
    LessEqual(Value),
    In(Vec<Value>),
    Between((Value, Value)),
    Like(String),
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Condition {
    pub column: String,
    pub equation: Equation,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub(crate) enum Expression {
    Conjunction(Conjunction),
    Simple(Condition),
    Nest(Vec<Expression>),
}

impl From<Expressions> for Expression {
    fn from(v: Expressions) -> Self {
        Expression::Nest(v.0)
    }
}

impl From<Conjunction> for Expression {
    fn from(c: Conjunction) -> Self {
        Expression::Conjunction(c)
    }
}

impl From<Condition> for Expression {
    fn from(c: Condition) -> Self {
        Expression::Simple(c)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Expressions(pub(crate) Vec<Expression>);

// ================================================================================================
// Expression builder
// A finite state machine used for building expressions
// ================================================================================================

// AND/OR
pub struct ConjunctionState {
    stack: Vec<Expression>,
}

// Simple expression
pub struct SimpleState {
    stack: Vec<Expression>,
}

// Nested expression
pub struct NestState {
    stack: Vec<Expression>,
}

impl From<Condition> for SimpleState {
    fn from(c: Condition) -> Self {
        SimpleState {
            stack: vec![Expression::Simple(c)],
        }
    }
}

impl From<Expressions> for NestState {
    fn from(val: Expressions) -> Self {
        NestState { stack: val.0 }
    }
}

// Trait represents transition from one state to another
pub trait ExpressionTransit<T, S> {
    fn append(self, state: T) -> S;

    fn finish(self) -> Expressions;
}

// Simple -> Conjunction
impl ExpressionTransit<Conjunction, ConjunctionState> for SimpleState {
    fn append(mut self, state: Conjunction) -> ConjunctionState {
        self.stack.push(Expression::from(state));
        ConjunctionState { stack: self.stack }
    }

    fn finish(self) -> Expressions {
        Expressions(self.stack)
    }
}

// Nest -> Conjunction
impl ExpressionTransit<Conjunction, ConjunctionState> for NestState {
    fn append(mut self, state: Conjunction) -> ConjunctionState {
        self.stack.push(Expression::from(state));
        ConjunctionState { stack: self.stack }
    }

    fn finish(self) -> Expressions {
        Expressions(self.stack)
    }
}

// Conjunction -> Simple
impl ExpressionTransit<Condition, SimpleState> for ConjunctionState {
    fn append(mut self, state: Condition) -> SimpleState {
        self.stack.push(Expression::from(state));
        SimpleState { stack: self.stack }
    }

    fn finish(self) -> Expressions {
        Expressions(self.stack)
    }
}

// Conjunction -> Nest
impl ExpressionTransit<Expressions, NestState> for ConjunctionState {
    fn append(mut self, state: Expressions) -> NestState {
        self.stack.push(Expression::from(state));
        NestState { stack: self.stack }
    }

    fn finish(self) -> Expressions {
        Expressions(self.stack)
    }
}

#[derive(Default)]
pub struct ExpressionsBuilder;

impl ExpressionsBuilder {
    pub fn new() -> Self {
        ExpressionsBuilder
    }

    pub fn from_condition(value: Condition) -> SimpleState {
        value.into()
    }

    pub fn from_expressions(value: Expressions) -> NestState {
        value.into()
    }

    pub fn append(self, value: Condition) -> SimpleState {
        value.into()
    }
}

// ================================================================================================
// Select
// ================================================================================================

/// Select statement
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Select {
    pub table: String,
    pub columns: Vec<ColumnAlias>,
    pub filter: Option<Expressions>,
    pub order: Option<Vec<Order>>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub include_primary_key: Option<bool>,
}

impl Select {
    pub fn new<T: Into<String>>(table: T) -> Self {
        Select {
            table: table.into(),
            columns: vec![],
            filter: None,
            order: None,
            limit: None,
            offset: None,
            include_primary_key: None,
        }
    }

    pub fn get_table(&self) -> &str {
        &self.table
    }

    pub fn get_columns(&self) -> &[ColumnAlias] {
        &self.columns
    }

    pub fn get_filter(&self) -> Option<&Expressions> {
        self.filter.as_ref()
    }

    pub fn get_order(&self) -> Option<&[Order]> {
        self.order.as_deref()
    }

    pub fn get_limit(&self) -> Option<usize> {
        self.limit
    }

    pub fn get_offset(&self) -> Option<usize> {
        self.offset
    }

    pub fn get_include_primary_key(&self) -> Option<bool> {
        self.include_primary_key
    }

    pub fn columns_name(&self, alias: bool) -> Vec<String> {
        self.columns
            .iter()
            .map(|c| if alias { c.name() } else { c.original_name() })
            .collect_vec()
    }

    pub fn columns<T>(mut self, columns: &[T]) -> Self
    where
        ColumnAlias: From<T>,
        T: Copy,
    {
        self.columns.extend(columns.iter().map(|c| (*c).into()));
        self
    }

    pub fn filter(mut self, filter: &Expressions) -> Self {
        self.filter = Some(filter.to_owned());
        self
    }

    pub fn order(mut self, order: &[Order]) -> Self {
        self.order = Some(order.to_owned());
        self
    }

    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn offset(mut self, offset: usize) -> Self {
        self.offset = Some(offset);
        self
    }

    pub fn include_primary_key(mut self, include: bool) -> Self {
        self.include_primary_key = Some(include);
        self
    }
}

// ================================================================================================
// Delete
// ================================================================================================

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Delete {
    pub table: String,
    pub filter: Expressions,
}

impl Delete {
    pub fn new(table: String) -> Self {
        Delete {
            table,
            filter: Expressions::default(),
        }
    }

    pub fn filter(&mut self, filter: &Expressions) -> &mut Self {
        self.filter = filter.to_owned();
        self
    }
}

// ================================================================================================
// SaveStrategy
// ================================================================================================

/// saving strategy for `save` function
/// Variants:
/// - `FailIfExists`: if table exists, do nothing
/// - `Replace`: drop if exists, create new table
/// - `Append`: ignore primary key, append to an existing table; index will be ignored
/// - `Upsert`: if table exists: insert if id not exists, update if id exists; index will not be ignored
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum SaveStrategy {
    FailIfExists,
    Replace,
    // Index is always ignored
    Append,
    // Index is always used
    Upsert,
}

// ================================================================================================
// IndexType & IndexOption
// ================================================================================================

/// index type is used for defining Sql column type
#[derive(Debug, Clone)]
pub enum IndexType {
    Int,
    BigInt,
    Uuid,
}

impl Default for IndexType {
    fn default() -> Self {
        IndexType::Int
    }
}

impl From<&str> for IndexType {
    fn from(v: &str) -> Self {
        match &v.to_lowercase()[..] {
            "int" | "i" => IndexType::Int,
            "bigint" | "b" => IndexType::BigInt,
            "uuid" | "u" => IndexType::Uuid,
            _ => IndexType::Int,
        }
    }
}

/// index option
///
/// Only be used in `create_table`
#[derive(Debug, Clone)]
pub struct IndexOption {
    pub name: String,
    pub index_type: IndexType,
}

impl Default for IndexOption {
    fn default() -> Self {
        IndexOption {
            name: "_id".to_string(),
            index_type: IndexType::default(),
        }
    }
}

impl IndexOption {
    pub fn new<N, T>(name: N, index_type: T) -> Self
    where
        N: Into<String>,
        T: Into<IndexType>,
    {
        let name = name.into();
        let index_type: IndexType = index_type.into();
        IndexOption { name, index_type }
    }

    pub fn try_from_series(series: &Series) -> SqlResult<Self> {
        let dtype = series.dtype();
        let index_type = match dtype {
            ValueType::U8 => Ok(IndexType::Int),
            ValueType::U16 => Ok(IndexType::Int),
            ValueType::U32 => Ok(IndexType::Int),
            ValueType::U64 => Ok(IndexType::BigInt),
            ValueType::I8 => Ok(IndexType::Int),
            ValueType::I16 => Ok(IndexType::Int),
            ValueType::I32 => Ok(IndexType::Int),
            ValueType::I64 => Ok(IndexType::BigInt),
            ValueType::Uuid => Ok(IndexType::Uuid),
            ValueType::F32 => Ok(IndexType::Int),
            ValueType::F64 => Ok(IndexType::BigInt),
            _ => Err(SqlError::new_common_error(format!(
                "{:?} is not an appropriate index type",
                dtype
            ))),
        }?;

        Ok(IndexOption {
            name: series.name().to_owned(),
            index_type,
        })
    }
}

impl TryFrom<FieldInfo> for IndexOption {
    type Error = SqlError;

    fn try_from(value: FieldInfo) -> Result<Self, Self::Error> {
        let dtype = value.dtype();
        let index_type = match dtype {
            ValueType::U8 => Ok(IndexType::Int),
            ValueType::U16 => Ok(IndexType::Int),
            ValueType::U32 => Ok(IndexType::Int),
            ValueType::U64 => Ok(IndexType::BigInt),
            ValueType::I8 => Ok(IndexType::Int),
            ValueType::I16 => Ok(IndexType::Int),
            ValueType::I32 => Ok(IndexType::Int),
            ValueType::I64 => Ok(IndexType::BigInt),
            ValueType::Uuid => Ok(IndexType::Uuid),
            ValueType::F32 => Ok(IndexType::Int),
            ValueType::F64 => Ok(IndexType::BigInt),
            _ => Err(SqlError::new_common_error(format!(
                "{:?} cannot convert to index type",
                dtype
            ))),
        }?;

        Ok(IndexOption {
            name: value.name().to_owned(),
            index_type,
        })
    }
}

// ================================================================================================
// ExecutionResult
// ================================================================================================

pub struct ExecutionResult {
    pub rows_affected: u64,
}

impl From<u64> for ExecutionResult {
    fn from(v: u64) -> Self {
        ExecutionResult { rows_affected: v }
    }
}

#[cfg(test)]
mod test_sql_adt {
    use super::*;

    #[test]
    fn build_filter() {
        let a = ExpressionsBuilder::from_condition(Condition {
            column: String::from("name"),
            equation: Equation::Equal("foo".into()),
        })
        .append(Conjunction::AND)
        .append(Condition {
            column: String::from("age"),
            equation: Equation::Equal(10.into()),
        })
        .append(Conjunction::OR)
        .append(Condition {
            column: String::from("age"),
            equation: Equation::Equal(20.into()),
        })
        .finish();

        let b = ExpressionsBuilder::from_condition(Condition {
            column: String::from("name"),
            equation: Equation::Equal("bar".into()),
        })
        .append(Conjunction::OR)
        .append(a)
        .finish();

        println!("{:?}", b);
    }
}
