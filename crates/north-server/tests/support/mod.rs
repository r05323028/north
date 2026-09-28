use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DbBackend, DbErr, ExecResult,
    FromQueryResult, QueryResult, Statement, TryGetable, Value,
};
use std::{
    env,
    ffi::OsString,
    marker::PhantomData,
    sync::{Mutex, MutexGuard, OnceLock},
};

static ENVIRONMENT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[allow(dead_code)]
pub fn test_otp_key() -> north_persistence::OtpKey {
    north_persistence::OtpKey::from_hex(
        "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
    )
    .expect("valid test OTP key")
}

pub async fn database(url: &str, max_connections: u32) -> Result<DatabaseConnection, DbErr> {
    let mut options = ConnectOptions::new(url);
    options.max_connections(max_connections);
    Database::connect(options).await
}

#[derive(Clone, Copy)]
pub struct TestDatabaseOptions {
    max_connections: u32,
}

impl TestDatabaseOptions {
    pub fn new() -> Self {
        Self {
            max_connections: 10,
        }
    }

    pub fn max_connections(mut self, max_connections: u32) -> Self {
        self.max_connections = max_connections;
        self
    }

    pub async fn connect(self, url: &str) -> Result<DatabaseConnection, DbErr> {
        database(url, self.max_connections).await
    }
}

pub fn query(sql: impl Into<String>) -> Query {
    Query {
        sql: sql.into(),
        values: Vec::new(),
    }
}

pub fn query_as<T>(sql: impl Into<String>) -> QueryAs<T> {
    QueryAs {
        query: query(sql),
        marker: PhantomData,
    }
}

pub fn query_tuple<T>(sql: impl Into<String>) -> QueryTuple<T> {
    QueryTuple {
        query: query(sql),
        marker: PhantomData,
    }
}

pub fn query_scalar<T>(sql: impl Into<String>) -> QueryScalar<T> {
    QueryScalar {
        query: query(sql),
        marker: PhantomData,
    }
}

pub struct Query {
    sql: String,
    values: Vec<Value>,
}

impl Query {
    pub fn bind<V: Into<Value>>(mut self, value: V) -> Self {
        self.values.push(value.into());
        self
    }

    fn statement(self) -> Statement {
        Statement::from_sql_and_values(DbBackend::Postgres, self.sql, self.values)
    }

    pub async fn execute<C: ConnectionTrait>(self, connection: &C) -> Result<ExecResult, DbErr> {
        connection.execute_raw(self.statement()).await
    }

    pub async fn fetch_optional<C: ConnectionTrait>(
        self,
        connection: &C,
    ) -> Result<Option<QueryResult>, DbErr> {
        connection.query_one_raw(self.statement()).await
    }

    pub async fn fetch_one<C: ConnectionTrait>(self, connection: &C) -> Result<QueryResult, DbErr> {
        self.fetch_optional(connection)
            .await?
            .ok_or_else(|| DbErr::RecordNotFound("query returned no rows".to_owned()))
    }

    pub async fn fetch_all<C: ConnectionTrait>(
        self,
        connection: &C,
    ) -> Result<Vec<QueryResult>, DbErr> {
        connection.query_all_raw(self.statement()).await
    }
}

pub struct QueryAs<T> {
    query: Query,
    marker: PhantomData<T>,
}

impl<T> QueryAs<T> {
    pub fn bind<V: Into<Value>>(mut self, value: V) -> Self {
        self.query = self.query.bind(value);
        self
    }
}

impl<T: FromQueryResult> QueryAs<T> {
    pub async fn fetch_optional<C: ConnectionTrait>(
        self,
        connection: &C,
    ) -> Result<Option<T>, DbErr> {
        T::find_by_statement(self.query.statement())
            .one(connection)
            .await
    }

    pub async fn fetch_one<C: ConnectionTrait>(self, connection: &C) -> Result<T, DbErr> {
        self.fetch_optional(connection)
            .await?
            .ok_or_else(|| DbErr::RecordNotFound("query returned no rows".to_owned()))
    }

    pub async fn fetch_all<C: ConnectionTrait>(self, connection: &C) -> Result<Vec<T>, DbErr> {
        T::find_by_statement(self.query.statement())
            .all(connection)
            .await
    }
}

pub trait TupleRow: Sized {
    fn from_row(row: &QueryResult) -> Result<Self, DbErr>;
}

macro_rules! impl_tuple_row {
    ($($type:ident:$index:tt),+ $(,)?) => {
        impl<$($type: TryGetable),+> TupleRow for ($($type,)+) {
            fn from_row(row: &QueryResult) -> Result<Self, DbErr> {
                Ok(($(row.try_get_by_index($index)?,)+))
            }
        }
    };
}

impl_tuple_row!(A:0, B:1);
impl_tuple_row!(A:0, B:1, C:2);
impl_tuple_row!(A:0, B:1, C:2, D:3);
impl_tuple_row!(A:0, B:1, C:2, D:3, E:4);
impl_tuple_row!(A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7);

pub struct QueryTuple<T> {
    query: Query,
    marker: PhantomData<T>,
}

impl<T> QueryTuple<T> {
    pub fn bind<V: Into<Value>>(mut self, value: V) -> Self {
        self.query = self.query.bind(value);
        self
    }
}

impl<T: TupleRow> QueryTuple<T> {
    pub async fn fetch_one<C: ConnectionTrait>(self, connection: &C) -> Result<T, DbErr> {
        let row = self.query.fetch_one(connection).await?;
        T::from_row(&row)
    }

    pub async fn fetch_all<C: ConnectionTrait>(self, connection: &C) -> Result<Vec<T>, DbErr> {
        self.query
            .fetch_all(connection)
            .await?
            .iter()
            .map(T::from_row)
            .collect()
    }
}

pub struct QueryScalar<T> {
    query: Query,
    marker: PhantomData<T>,
}

impl<T> QueryScalar<T> {
    pub fn bind<V: Into<Value>>(mut self, value: V) -> Self {
        self.query = self.query.bind(value);
        self
    }
}

impl<T: TryGetable> QueryScalar<T> {
    pub async fn fetch_optional<C: ConnectionTrait>(
        self,
        connection: &C,
    ) -> Result<Option<T>, DbErr> {
        connection
            .query_one_raw(self.query.statement())
            .await?
            .map(|row| row.try_get_by_index(0))
            .transpose()
    }

    pub async fn fetch_one<C: ConnectionTrait>(self, connection: &C) -> Result<T, DbErr> {
        self.fetch_optional(connection)
            .await?
            .ok_or_else(|| DbErr::RecordNotFound("query returned no rows".to_owned()))
    }

    pub async fn fetch_all<C: ConnectionTrait>(self, connection: &C) -> Result<Vec<T>, DbErr> {
        connection
            .query_all_raw(self.query.statement())
            .await?
            .iter()
            .map(|row| row.try_get_by_index(0))
            .collect()
    }
}

pub struct ScopedEnvVar {
    name: &'static str,
    previous: Option<OsString>,
    _guard: MutexGuard<'static, ()>,
}

impl ScopedEnvVar {
    pub fn set(name: &'static str, value: &str) -> Self {
        let guard = ENVIRONMENT_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = env::var_os(name);
        env::set_var(name, value);
        Self {
            name,
            previous,
            _guard: guard,
        }
    }
}

impl Drop for ScopedEnvVar {
    fn drop(&mut self) {
        match self.previous.as_deref() {
            Some(value) => env::set_var(self.name, value),
            None => env::remove_var(self.name),
        }
    }
}
