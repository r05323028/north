use sea_orm::{
    ConnectionTrait, DbBackend, DbErr, ExecResult, FromQueryResult, Statement, TryGetable, Value,
};
use std::marker::PhantomData;

pub(crate) fn query(sql: impl Into<String>) -> Query {
    Query {
        sql: sql.into(),
        values: Vec::new(),
    }
}

pub(crate) fn query_as<T>(sql: impl Into<String>) -> QueryAs<T> {
    QueryAs {
        query: query(sql),
        marker: PhantomData,
    }
}

pub(crate) fn query_scalar<T>(sql: impl Into<String>) -> QueryScalar<T> {
    QueryScalar {
        query: query(sql),
        marker: PhantomData,
    }
}

pub(crate) struct Query {
    sql: String,
    values: Vec<Value>,
}

impl Query {
    pub(crate) fn bind<V: Into<Value>>(mut self, value: V) -> Self {
        self.values.push(value.into());
        self
    }

    fn into_statement(self) -> Statement {
        Statement::from_sql_and_values(DbBackend::Postgres, self.sql, self.values)
    }

    pub(crate) async fn execute<C: ConnectionTrait>(
        self,
        connection: &C,
    ) -> Result<ExecResult, DbErr> {
        connection.execute_raw(self.into_statement()).await
    }
}

pub(crate) struct QueryAs<T> {
    query: Query,
    marker: PhantomData<T>,
}

impl<T> QueryAs<T> {
    pub(crate) fn bind<V: Into<Value>>(mut self, value: V) -> Self {
        self.query = self.query.bind(value);
        self
    }
}

impl<T: FromQueryResult> QueryAs<T> {
    pub(crate) async fn fetch_optional<C: ConnectionTrait>(
        self,
        connection: &C,
    ) -> Result<Option<T>, DbErr> {
        T::find_by_statement(self.query.into_statement())
            .one(connection)
            .await
    }

    pub(crate) async fn fetch_one<C: ConnectionTrait>(self, connection: &C) -> Result<T, DbErr> {
        self.fetch_optional(connection)
            .await?
            .ok_or_else(|| DbErr::RecordNotFound("query returned no rows".to_owned()))
    }

    pub(crate) async fn fetch_all<C: ConnectionTrait>(
        self,
        connection: &C,
    ) -> Result<Vec<T>, DbErr> {
        T::find_by_statement(self.query.into_statement())
            .all(connection)
            .await
    }
}

pub(crate) struct QueryScalar<T> {
    query: Query,
    marker: PhantomData<T>,
}

impl<T> QueryScalar<T> {
    pub(crate) fn bind<V: Into<Value>>(mut self, value: V) -> Self {
        self.query = self.query.bind(value);
        self
    }
}

impl<T: TryGetable> QueryScalar<T> {
    pub(crate) async fn fetch_optional<C: ConnectionTrait>(
        self,
        connection: &C,
    ) -> Result<Option<T>, DbErr> {
        connection
            .query_one_raw(self.query.into_statement())
            .await?
            .map(|row| row.try_get_by_index(0))
            .transpose()
    }

    pub(crate) async fn fetch_one<C: ConnectionTrait>(self, connection: &C) -> Result<T, DbErr> {
        self.fetch_optional(connection)
            .await?
            .ok_or_else(|| DbErr::RecordNotFound("query returned no rows".to_owned()))
    }
}
