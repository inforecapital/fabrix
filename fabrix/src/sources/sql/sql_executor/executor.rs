//! Database executor

use async_trait::async_trait;
use sqlx::{MySqlPool, PgPool, SqlitePool};

use super::{
    conn_e_err, conn_n_err, loader::LoaderTransaction, types::string_try_into_value_type,
    FabrixDatabaseLoader, LoaderPool, SqlConnInfo,
};
use crate::{
    adt, DataFrame, DdlMutation, DdlQuery, DmlMutation, DmlQuery, Series, SqlBuilder, SqlError,
    SqlResult, Value, ValueType, D1,
};

#[async_trait]
pub trait Helper {
    /// get primary key from a table
    async fn get_primary_key(&self, table_name: &str) -> SqlResult<String>;

    /// get schema from a table
    async fn get_table_schema(&self, table_name: &str) -> SqlResult<Vec<adt::TableSchema>>;

    /// get existing ids, supposing that the primary key is a single column, and the value is a string
    async fn get_existing_ids(&self, table_name: &str, ids: &Series) -> SqlResult<D1>;
}

/// An engin is an interface to describe sql executor's business logic
#[async_trait]
pub trait SqlEngine: Helper {
    /// connect to the database
    async fn connect(&mut self) -> SqlResult<()>;

    /// disconnect from the database
    async fn disconnect(&mut self) -> SqlResult<()>;

    /// insert data into a table
    async fn insert(&self, table_name: &str, data: DataFrame) -> SqlResult<u64>;

    /// update data in a table
    async fn update(&self, table_name: &str, data: DataFrame) -> SqlResult<u64>;

    /// save data into a table
    /// saving strategy:
    /// 1. Replace: no matter the table is exist, create a new table
    /// 1. Append: if the table is exist, append data to the table, otherwise failed
    /// 1. Upsert: update and insert
    /// 1. Fail: if the table is exist, do nothing, otherwise create a new table
    async fn save(
        &self,
        table_name: &str,
        data: DataFrame,
        strategy: &adt::SaveStrategy,
    ) -> SqlResult<usize>;

    /// delete data from an existing table.
    async fn delete(&self, delete: &adt::Delete) -> SqlResult<u64>;

    /// get data from db. If the table has primary key, DataFrame's index will be the primary key
    async fn select(&self, select: &adt::Select) -> SqlResult<DataFrame>;
}

/// Executor is the core struct of db mod.
/// It plays a role of CRUD and provides data manipulation functionality.
pub struct SqlExecutor {
    driver: SqlBuilder,
    conn_str: String,
    pool: Option<Box<dyn FabrixDatabaseLoader>>,
}

impl SqlExecutor {
    /// constructor
    pub fn new(conn_info: SqlConnInfo) -> Self {
        SqlExecutor {
            driver: conn_info.driver.clone(),
            conn_str: conn_info.to_string(),
            pool: None,
        }
    }

    /// constructor, from str
    pub fn from_str(conn_str: &str) -> Self {
        let mut s = conn_str.split(":");
        let driver = match s.next() {
            Some(v) => v.into(),
            None => SqlBuilder::Sqlite,
        };
        SqlExecutor {
            driver,
            conn_str: conn_str.to_string(),
            pool: None,
        }
    }
}

#[async_trait]
impl Helper for SqlExecutor {
    async fn get_primary_key(&self, table_name: &str) -> SqlResult<String> {
        conn_n_err!(self.pool);
        let que = self.driver.get_primary_key(table_name);
        let schema = [ValueType::String];
        let res = self
            .pool
            .as_ref()
            .unwrap()
            .fetch_optional_with_schema(&que, &schema)
            .await?;

        if let Some(v) = res {
            if let Some(k) = v.first() {
                return Ok(try_value_into_string(k)?);
            }
        }

        Err(SqlError::new_common_error("primary key not found"))
    }

    async fn get_table_schema(&self, table_name: &str) -> SqlResult<Vec<adt::TableSchema>> {
        conn_n_err!(self.pool);
        let que = self.driver.check_table_schema(table_name);
        let schema = [ValueType::String, ValueType::String, ValueType::String];
        let res = self
            .pool
            .as_ref()
            .unwrap()
            .fetch_all_with_schema(&que, &schema)
            .await?
            .into_iter()
            .map(|v| {
                let type_str = try_value_into_string(&v[1])?.to_uppercase();
                let dtype =
                    string_try_into_value_type(&self.driver, &type_str).unwrap_or(ValueType::Null);
                let is_nullable = if try_value_into_string(&v[2])? == "YES" {
                    true
                } else {
                    false
                };

                let res = adt::TableSchema {
                    name: try_value_into_string(&v[0])?,
                    dtype,
                    is_nullable,
                };

                Ok(res)
            })
            .collect::<SqlResult<Vec<adt::TableSchema>>>()?;

        Ok(res)
    }

    async fn get_existing_ids(&self, table_name: &str, ids: &Series) -> SqlResult<D1> {
        conn_n_err!(self.pool);
        let que = self.driver.select_existing_ids(table_name, ids)?;
        let schema = [ids.dtype()];
        let res = self
            .pool
            .as_ref()
            .unwrap()
            .fetch_all_with_schema(&que, &schema)
            .await?
            .iter_mut()
            .map(|v| v.remove(0))
            .collect::<Vec<Value>>();

        Ok(res)
    }
}

#[async_trait]
impl SqlEngine for SqlExecutor {
    async fn connect(&mut self) -> SqlResult<()> {
        conn_e_err!(self.pool);
        match self.driver {
            SqlBuilder::Mysql => MySqlPool::connect(&self.conn_str).await.map(|pool| {
                self.pool = Some(Box::new(LoaderPool::from(pool)));
            })?,
            SqlBuilder::Postgres => PgPool::connect(&self.conn_str).await.map(|pool| {
                self.pool = Some(Box::new(LoaderPool::from(pool)));
            })?,
            SqlBuilder::Sqlite => SqlitePool::connect(&self.conn_str).await.map(|pool| {
                self.pool = Some(Box::new(LoaderPool::from(pool)));
            })?,
        }
        Ok(())
    }

    async fn disconnect(&mut self) -> SqlResult<()> {
        conn_n_err!(self.pool);
        self.pool.as_ref().unwrap().disconnect().await;
        Ok(())
    }

    async fn insert(&self, table_name: &str, data: DataFrame) -> SqlResult<u64> {
        conn_n_err!(self.pool);
        let que = self.driver.insert(table_name, data, false)?;
        let res = self.pool.as_ref().unwrap().execute(&que).await?;

        Ok(res.rows_affected)
    }

    async fn update(&self, table_name: &str, data: DataFrame) -> SqlResult<u64> {
        conn_n_err!(self.pool);
        let index_field = data.index_field();
        let index_option = adt::IndexOption::try_from(&index_field)?;
        let que = self.driver.update(table_name, data, &index_option)?;

        let res = self
            .pool
            .as_ref()
            .unwrap()
            .execute_many(&que)
            .await?
            .rows_affected;

        Ok(res)
    }

    async fn save(
        &self,
        table_name: &str,
        data: DataFrame,
        strategy: &adt::SaveStrategy,
    ) -> SqlResult<usize> {
        conn_n_err!(self.pool);

        match strategy {
            adt::SaveStrategy::FailIfExists => {
                // check if table exists
                let ck_str = self.driver.check_table_exists(table_name);
                // BEWARE: use fetch_optional instead of fetch_one is because `check_table_exists`
                // will only return one row or none
                if let Some(_) = self.pool.as_ref().unwrap().fetch_optional(&ck_str).await? {
                    return Err(SqlError::new_common_error(
                        "table already exist, table cannot be saved",
                    ));
                }

                // start a transaction
                let txn = self.pool.as_ref().unwrap().begin_transaction().await?;

                create_and_insert(&self.driver, txn, table_name, data).await
            }
            adt::SaveStrategy::Replace => {
                // check if table exists
                let ck_str = self.driver.check_table_exists(table_name);

                // start a transaction
                let mut txn = self.pool.as_ref().unwrap().begin_transaction().await?;

                // BEWARE: use fetch_optional instead of fetch_one is because `check_table_exists`
                // will only return one row or none
                if let Some(_) = self.pool.as_ref().unwrap().fetch_optional(&ck_str).await? {
                    let del_str = self.driver.delete_table(table_name);
                    txn.execute(&del_str).await?;
                }

                create_and_insert(&self.driver, txn, table_name, data).await
            }
            adt::SaveStrategy::Append => {
                // insert to an existing table and ignore primary key
                // this action is supposed that primary key can be auto generated
                let que = self.driver.insert(table_name, data, true)?;
                let res = self.pool.as_ref().unwrap().execute(&que).await?;

                Ok(res.rows_affected as usize)
            }
            adt::SaveStrategy::Upsert => {
                // get existing ids from selected table
                let existing_ids = self.get_existing_ids(table_name, data.index()).await?;
                let existing_ids = Series::from_values_default_name(existing_ids, false)?;

                // declare a df for inserting
                let mut df_to_insert = data;
                // popup a df for updating
                let df_to_update = df_to_insert.popup_rows(&existing_ids)?;

                let r1 = self.insert(&table_name, df_to_insert).await?;
                let r2 = self.update(&table_name, df_to_update).await?;

                Ok((r1 + r2) as usize)
            }
        }
    }

    async fn delete(&self, delete: &adt::Delete) -> SqlResult<u64> {
        conn_n_err!(self.pool);
        let que = self.driver.delete(delete);
        let res = self.pool.as_ref().unwrap().execute(&que).await?;

        Ok(res.rows_affected)
    }

    async fn select(&self, select: &adt::Select) -> SqlResult<DataFrame> {
        conn_n_err!(self.pool);

        // Generally, primary key always exists, and in this case, use it as index.
        // Otherwise, use default index.
        let mut df = match self.get_primary_key(&select.table).await {
            Ok(pk) => {
                let mut new_select = select.clone();
                add_primary_key_to_select(&pk, &mut new_select);
                let que = self.driver.select(&new_select);
                let res = self.pool.as_ref().unwrap().fetch_all_to_rows(&que).await?;
                DataFrame::from_rows(res)?
            }
            Err(_) => {
                let que = self.driver.select(select);
                let res = self.pool.as_ref().unwrap().fetch_all(&que).await?;
                DataFrame::from_row_values(res)?
            }
        };
        df.set_column_names(&select.columns_name(true))?;

        Ok(df)
    }
}

/// select primary key and other columns from a table
fn add_primary_key_to_select(primary_key: &String, select: &mut adt::Select) {
    select
        .columns
        .insert(0, adt::ColumnAlias::Simple(primary_key.to_owned()));
}

/// `Value` -> String
fn try_value_into_string(value: &Value) -> SqlResult<String> {
    match value {
        Value::String(v) => Ok(v.to_owned()),
        _ => Err(SqlError::new_common_error("value is not a string")),
    }
}

/// create table and insert data
async fn create_and_insert<'a>(
    driver: &SqlBuilder,
    mut txn: LoaderTransaction<'a>,
    table_name: &str,
    data: DataFrame,
) -> SqlResult<usize> {
    // transaction starts
    let mut affected_rows = 0;

    // create table string
    let fi = data.index_field();
    let index_option = adt::IndexOption::try_from(&fi)?;
    let create_str = driver.create_table(table_name, &data.fields(), Some(&index_option));

    // create table
    match txn.execute(&create_str).await {
        Ok(res) => {
            affected_rows += res.rows_affected;
        }
        Err(e) => {
            return Err(e);
        }
    }

    // insert string
    let insert_str = driver.insert(table_name, data, false)?;

    // insert data
    match txn.execute(&insert_str).await {
        Ok(res) => {
            affected_rows += res.rows_affected;
        }
        Err(e) => {
            txn.rollback().await?;
            return Err(e);
        }
    }

    // commit transaction
    txn.commit().await?;

    Ok(affected_rows as usize)
}

#[cfg(test)]
mod test_executor {

    use super::*;
    use crate::{df, series, value, DateTime};

    const CONN1: &'static str = "mysql://root:secret@localhost:3306/dev";
    const CONN2: &'static str = "postgres://root:secret@localhost:5432/dev";
    const CONN3: &'static str = "sqlite://dev.sqlite";

    // table name
    const TABLE_NAME: &str = "dev";

    #[tokio::test]
    async fn test_connection() {
        let mut exc = SqlExecutor::from_str(CONN1);

        exc.connect().await.expect("connection is ok");
    }

    #[tokio::test]
    async fn test_get_primary_key() {
        let mut exc = SqlExecutor::from_str(CONN1);

        exc.connect().await.expect("connection is ok");

        println!("{:?}", exc.get_primary_key("dev").await);
    }

    #[tokio::test]
    async fn test_save_fail_if_exists() {
        // df
        let df = df![
            "ord";
            "names" => ["Jacob", "Sam", "James", "Lucas", "Mia"],
            "ord" => [10,11,12,20,22],
            "val" => [Some(10.1), None, Some(8.0), Some(9.5), Some(10.8)],
            "dt" => [
                DateTime(chrono::NaiveDate::from_ymd(2016, 1, 8).and_hms(9, 10, 11)),
                DateTime(chrono::NaiveDate::from_ymd(2017, 1, 7).and_hms(9, 10, 11)),
                DateTime(chrono::NaiveDate::from_ymd(2018, 1, 6).and_hms(9, 10, 11)),
                DateTime(chrono::NaiveDate::from_ymd(2019, 1, 5).and_hms(9, 10, 11)),
                DateTime(chrono::NaiveDate::from_ymd(2020, 1, 4).and_hms(9, 10, 11)),
            ]
        ]
        .unwrap();

        let save_strategy = adt::SaveStrategy::FailIfExists;

        // mysql
        let mut exc = SqlExecutor::from_str(CONN1);
        exc.connect().await.expect("connection is ok");

        let res = exc.save(TABLE_NAME, df.clone(), &save_strategy).await;
        println!("{:?}", res);

        // postgres
        let mut exc = SqlExecutor::from_str(CONN2);
        exc.connect().await.expect("connection is ok");

        let res = exc.save(TABLE_NAME, df.clone(), &save_strategy).await;
        println!("{:?}", res);

        // sqlite
        let mut exc = SqlExecutor::from_str(CONN3);
        exc.connect().await.expect("connection is ok");

        let res = exc.save(TABLE_NAME, df.clone(), &save_strategy).await;
        println!("{:?}", res);
    }

    #[tokio::test]
    async fn test_save_replace() {
        // df
        let df = df![
            "ord";
            "names" => ["Jacob", "Sam", "James", "Lucas", "Mia", "Livia"],
            "ord" => [10,11,12,20,22,31],
            "val" => [Some(10.1), None, Some(8.0), Some(9.5), Some(10.8), Some(11.2)],
            "note" => [Some("FS"), Some("OP"), Some("TEC"), None, Some("SS"), None],
            "dt" => [
                DateTime(chrono::NaiveDate::from_ymd(2016, 1, 8).and_hms(9, 10, 11)),
                DateTime(chrono::NaiveDate::from_ymd(2017, 1, 7).and_hms(9, 10, 11)),
                DateTime(chrono::NaiveDate::from_ymd(2018, 1, 6).and_hms(9, 10, 11)),
                DateTime(chrono::NaiveDate::from_ymd(2019, 1, 5).and_hms(9, 10, 11)),
                DateTime(chrono::NaiveDate::from_ymd(2020, 1, 4).and_hms(9, 10, 11)),
                DateTime(chrono::NaiveDate::from_ymd(2020, 1, 3).and_hms(9, 10, 11)),
            ]
        ]
        .unwrap();

        let save_strategy = adt::SaveStrategy::Replace;

        // mysql
        let mut exc = SqlExecutor::from_str(CONN1);
        exc.connect().await.expect("connection is ok");

        let res = exc.save(TABLE_NAME, df.clone(), &save_strategy).await;
        println!("{:?}", res);

        // postgres
        let mut exc = SqlExecutor::from_str(CONN2);
        exc.connect().await.expect("connection is ok");

        let res = exc.save(TABLE_NAME, df.clone(), &save_strategy).await;
        println!("{:?}", res);

        // sqlite
        let mut exc = SqlExecutor::from_str(CONN3);
        exc.connect().await.expect("connection is ok");

        let res = exc.save(TABLE_NAME, df.clone(), &save_strategy).await;
        println!("{:?}", res);
    }

    #[tokio::test]
    async fn test_save_append() {
        // df
        let df = df![
            "ord";
            "names" => ["Fila", "Ada", "Kevin"],
            "ord" => [25,17,32],
            "val" => [None, Some(7.1), Some(2.4)],
            "note" => [Some(""), Some("M"), None],
            "dt" => [
                DateTime(chrono::NaiveDate::from_ymd(2010, 2, 5).and_hms(9, 10, 11)),
                DateTime(chrono::NaiveDate::from_ymd(2011, 2, 4).and_hms(9, 10, 11)),
                DateTime(chrono::NaiveDate::from_ymd(2012, 2, 3).and_hms(9, 10, 11)),
            ]
        ]
        .unwrap();

        let save_strategy = adt::SaveStrategy::Append;

        // mysql
        let mut exc = SqlExecutor::from_str(CONN1);
        exc.connect().await.expect("connection is ok");

        let res = exc.save(TABLE_NAME, df.clone(), &save_strategy).await;
        println!("{:?}", res);

        // postgres
        let mut exc = SqlExecutor::from_str(CONN2);
        exc.connect().await.expect("connection is ok");

        let res = exc.save(TABLE_NAME, df.clone(), &save_strategy).await;
        println!("{:?}", res);

        // sqlite
        let mut exc = SqlExecutor::from_str(CONN3);
        exc.connect().await.expect("connection is ok");

        let res = exc.save(TABLE_NAME, df.clone(), &save_strategy).await;
        println!("{:?}", res);
    }

    #[tokio::test]
    async fn test_save_upsert() {
        // df
        let df = df![
            "ord";
            "ord" => [10,15,20],
            "val" => [Some(12.7), Some(7.1), Some(8.9)],
        ]
        .unwrap();

        let save_strategy = adt::SaveStrategy::Upsert;

        // mysql
        let mut exc = SqlExecutor::from_str(CONN1);
        exc.connect().await.expect("connection is ok");

        let res = exc.save(TABLE_NAME, df.clone(), &save_strategy).await;
        println!("{:?}", res);

        // postgres
        let mut exc = SqlExecutor::from_str(CONN2);
        exc.connect().await.expect("connection is ok");

        let res = exc.save(TABLE_NAME, df.clone(), &save_strategy).await;
        println!("{:?}", res);

        // sqlite
        let mut exc = SqlExecutor::from_str(CONN3);
        exc.connect().await.expect("connection is ok");

        let res = exc.save(TABLE_NAME, df.clone(), &save_strategy).await;
        println!("{:?}", res);
    }

    #[tokio::test]
    async fn test_delete() {
        let delete = adt::Delete {
            table: TABLE_NAME.to_owned(),
            filter: vec![
                adt::Expression::Simple(adt::Condition {
                    column: "ord".to_owned(),
                    equation: adt::Equation::Equal(value!(15)),
                }),
                adt::Expression::Conjunction(adt::Conjunction::OR),
                adt::Expression::Nest(vec![
                    adt::Expression::Simple(adt::Condition {
                        column: "names".to_owned(),
                        equation: adt::Equation::Equal(value!("Livia")),
                    }),
                    adt::Expression::Conjunction(adt::Conjunction::AND),
                    adt::Expression::Simple(adt::Condition {
                        column: "val".to_owned(),
                        equation: adt::Equation::Greater(value!(10.0)),
                    }),
                ]),
            ],
        };

        // mysql
        let mut exc = SqlExecutor::from_str(CONN1);
        exc.connect().await.expect("connection is ok");

        let res = exc.delete(&delete).await;
        println!("{:?}", res);

        // postgres
        let mut exc = SqlExecutor::from_str(CONN2);
        exc.connect().await.expect("connection is ok");

        let res = exc.delete(&delete).await;
        println!("{:?}", res);

        // sqlite
        let mut exc = SqlExecutor::from_str(CONN3);
        exc.connect().await.expect("connection is ok");

        let res = exc.delete(&delete).await;
        println!("{:?}", res);
    }

    #[tokio::test]
    async fn test_select_primary_key() {
        let mut exc = SqlExecutor::from_str(CONN1);
        exc.connect().await.expect("connection is ok");

        let res = exc.get_primary_key(TABLE_NAME).await;
        println!("{:?}", res);

        let mut exc = SqlExecutor::from_str(CONN2);
        exc.connect().await.expect("connection is ok");

        let res = exc.get_primary_key(TABLE_NAME).await;
        println!("{:?}", res);

        let mut exc = SqlExecutor::from_str(CONN3);
        exc.connect().await.expect("connection is ok");

        let res = exc.get_primary_key(TABLE_NAME).await;
        println!("{:?}", res);
    }

    #[tokio::test]
    async fn test_select() {
        let select = adt::Select {
            table: "dev".to_owned(),
            columns: vec![
                adt::ColumnAlias::Simple("names".to_owned()),
                adt::ColumnAlias::Simple("val".to_owned()),
                adt::ColumnAlias::Simple("note".to_owned()),
                adt::ColumnAlias::Simple("dt".to_owned()),
                adt::ColumnAlias::Alias(adt::NameAlias {
                    from: "ord".to_owned(),
                    to: "ID".to_owned(),
                }),
            ],
            ..Default::default()
        };

        // mysql
        let mut exc = SqlExecutor::from_str(CONN1);
        exc.connect().await.expect("connection is ok");

        let df = exc.select(&select).await.unwrap();
        println!("{:?}", df);

        // postgres
        let mut exc = SqlExecutor::from_str(CONN2);
        exc.connect().await.expect("connection is ok");

        let df = exc.select(&select).await.unwrap();
        println!("{:?}", df);

        // sqlite
        let mut exc = SqlExecutor::from_str(CONN3);
        exc.connect().await.expect("connection is ok");

        let df = exc.select(&select).await.unwrap();
        println!("{:?}", df);
    }

    #[tokio::test]
    async fn test_fu() {
        use futures::future::TryFutureExt;

        let future = async { Ok::<i32, i32>(1) };
        let future = future.and_then(|x| async move { Ok::<i32, i32>(x + 3) });

        let res = future.await;

        println!("{:?}", res);
    }

    #[tokio::test]
    async fn test_get_table_schema() {
        // mysql
        let mut exc = SqlExecutor::from_str(CONN1);
        exc.connect().await.expect("connection is ok");

        let schema = exc.get_table_schema(TABLE_NAME).await.unwrap();
        println!("{:?}\n", schema);

        // pg
        let mut exc = SqlExecutor::from_str(CONN2);
        exc.connect().await.expect("connection is ok");

        let schema = exc.get_table_schema(TABLE_NAME).await.unwrap();
        println!("{:?}\n", schema);

        // sqlite
        let mut exc = SqlExecutor::from_str(CONN3);
        exc.connect().await.expect("connection is ok");

        let schema = exc.get_table_schema(TABLE_NAME).await.unwrap();
        println!("{:?}\n", schema);
    }

    #[tokio::test]
    async fn test_get_existing_ids() {
        let ids = series!("ord" => [10,11,14,20,21]);

        // mysql
        let mut exc = SqlExecutor::from_str(CONN1);
        exc.connect().await.expect("connection is ok");

        let res = exc.get_existing_ids(TABLE_NAME, &ids).await.unwrap();
        println!("{:?}", res);

        // pg
        let mut exc = SqlExecutor::from_str(CONN2);
        exc.connect().await.expect("connection is ok");

        let res = exc.get_existing_ids(TABLE_NAME, &ids).await.unwrap();
        println!("{:?}", res);

        // sqlite
        let mut exc = SqlExecutor::from_str(CONN3);
        exc.connect().await.expect("connection is ok");

        let res = exc.get_existing_ids(TABLE_NAME, &ids).await.unwrap();
        println!("{:?}", res);
    }
}