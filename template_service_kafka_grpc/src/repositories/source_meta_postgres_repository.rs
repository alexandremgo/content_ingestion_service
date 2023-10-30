use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use common::helper::error_chain_fmt;
use sqlx::{Executor, PgExecutor, PgPool, Postgres, Transaction};
use tracing::warn;

use crate::domain::entities::source_meta::{SourceMeta, SourceType};

// TODO: is it inheriting/implementing a "SqlRepository" ?
// #[async_trait]
// pub trait SourceMetaRepository {
//     async fn add_source_meta(
//         &self,
//         db_executor: impl PgExecutor<'_>,
//         source_meta: &SourceMeta,
//     ) -> Result<(), SourceMetaPostgresRepositoryError>;
// }

pub struct SourceMetaPostgresRepository {
    connectionPool: Arc<PgPool>,
}

impl SourceMetaPostgresRepository {
    pub fn new(connectionPool: Arc<PgPool>) -> Self {
        Self { connectionPool }
    }

    pub async fn beginWithTransaction(&mut self) -> Result<(), SourceMetaPostgresRepository> {
        // TODO
        let mut transaction = self.connectionPool.begin().await.unwrap();
        // .context("Failed to acquire a Postgres connection from the pool")?;

        Ok(())
    }
}

/// A type that contains or can provide a database
/// connection to use for executing queries against the database.
// enum SqlExecutor {
//     Transaction(impl Executor<'_>)

// }

// TODO: imagine the controllers know the repository implementation
#[async_trait]
pub trait Repository {
    // type UnitOfWork;
    type UnitOfWork<'unitOfWorkLifetime>;

    #[must_use]
    async fn begin_uow<'a>(&self) -> Self::UnitOfWork<'a>;
    // Take UoW by value, to finalize it.
    fn commit<'a>(&self, uow: Self::UnitOfWork<'a>) -> Result<(), RepositoryError>;

    // Take UoW by reference, to allow combining multiple actions into the unit
    // fn method_that_needs_transaction(&self, uow: &Self::UnitOfWork) -> /* */;
    // fn method_that_does_not_need_a_transaction(&self) -> /* */;
}

pub enum RepositoryError {}

#[async_trait]
pub trait SourceMetaRepository: Repository {
    // async fn add_source_meta<'a>(
    //     &self,
    //     uow: Self::UnitOfWork<'a>,
    //     source_meta: &SourceMeta,
    // ) -> Result<(), SourceMetaPostgresRepositoryError>;

    async fn add_source_meta(
        &self,
        db_executor: impl PgExecutor<'_>,
        source_meta: &SourceMeta,
    ) -> Result<(), SourceMetaPostgresRepositoryError>;
}

#[async_trait]
impl Repository for SourceMetaPostgresRepository {
    type UnitOfWork<'unitOfWorkLifetime> = Transaction<'unitOfWorkLifetime, Postgres>;

    // TODO: HERE
    // TODO: OK: great but how do we make this work with other repositories ?
    // They don't know about SourceMetaPostgresRepository unit of work ?
    // Could we have a parent SqlPostgresRepository handling this ?
    async fn begin_uow<'a>(&self) -> Self::UnitOfWork<'a> {
        self.connectionPool.begin().await.unwrap()
    }

    // Take UoW by value, to finalize it.
    fn commit<'a>(&self, uow: Self::UnitOfWork<'a>) -> Result<(), RepositoryError> {
        Ok(())
    }
}

// #[async_trait]
// impl SourceMetaRepository for SourceMetaPostgresRepository {
//     #[tracing::instrument(name = "Saving new source meta in database", skip(self, db_executor))]
//     async fn add_source_meta<'a>(
//         &self,
//         // uow: Self::UnitOfWork<'a>,
//         db_executor: impl PgExecutor<'_>,
//         source_meta: &SourceMeta,
//     ) -> Result<(), SourceMetaPostgresRepositoryError> {
//         Ok(())
//     }
// }

#[async_trait]
impl SourceMetaRepository for SourceMetaPostgresRepository {
    #[tracing::instrument(name = "Saving new source meta in database", skip(self, db_executor))]
    async fn add_source_meta(
        &self,
        db_executor: impl PgExecutor<'_>,
        source_meta: &SourceMeta,
    ) -> Result<(), SourceMetaPostgresRepositoryError> {
        sqlx::query!(
            r#"
    INSERT INTO source_metas (id, user_id, object_store_name, source_type, initial_name, added_at, extracted_at)
    VALUES ($1, $2, $3, $4, $5, $6, NULL)
            "#,
            source_meta.id,
            source_meta.user_id,
            source_meta.object_store_name,
            source_meta.source_type.to_owned() as SourceType,
            source_meta.initial_name.to_string(),
            Utc::now()
        )
        .execute(db_executor)
        .await?;

        Ok(())
    }
}

#[derive(thiserror::Error)]
pub enum SourceMetaPostgresRepositoryError {
    #[error(transparent)]
    DBError(#[from] sqlx::Error),
    // #[error(transparent)]
    // Other(#[from] anyhow::Error),
}

impl std::fmt::Debug for SourceMetaPostgresRepositoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

/// The controllers, which knows about the implementation of the current repository,
/// will be able to mutate the instance. Not the use-case.
/// TODO: well actually a commit needs to be updated, so it makes sense i guess that all the methods takes a &mut
#[async_trait]
pub trait RepositoryCore2 {
    // type UnitOfWork;
    // type UnitOfWork<'unitOfWorkLifetime>;
    type DBDriver;

    #[must_use]
    // async fn begin_uow<'a>(&self) -> Self::UnitOfWork<'a>;
    async fn begin_unit_of_work(&mut self) -> Result<(), RepositoryError>;

    // Take UoW by value, to finalize it.
    // fn commit<'a>(&self, uow: Self::UnitOfWork<'a>) -> Result<(), RepositoryError>; async fn commit_unit_of_work(&mut self) -> Result<(), RepositoryError>;

    // Take UoW by reference, to allow combining multiple actions into the unit
    // fn method_that_needs_transaction(&self, uow: &Self::UnitOfWork) -> /* */;
    // fn method_that_does_not_need_a_transaction(&self) -> /* */;

    async fn commit_unit_of_work(&mut self) -> Result<(), RepositoryError>;
}

#[async_trait]
pub trait SourceMetaRepository2: RepositoryCore2 {
    // Either take an executor ? Or
    async fn add_source_meta(
        &mut self,
        source_meta: &SourceMeta,
    ) -> Result<(), SourceMetaPostgresRepositoryError>;
}

/// Implemented as a state machine ?
/// # Lifetimes
/// - a: the unit of work (transaction) has to live until the current repository instance is dropped if necessary
pub struct PostgresRepository2<'a> {
    connection_pool: Arc<PgPool>,
    current_unit_of_work: Option<Transaction<'a, Postgres>>,
}

#[async_trait]
impl<'a> RepositoryCore2 for PostgresRepository2<'a> {
    type DBDriver = Postgres;

    async fn begin_unit_of_work(&mut self) -> Result<(), RepositoryError> {
        let transaction = self.connection_pool.begin().await.unwrap();
        self.current_unit_of_work = Some(transaction);
        Ok(())
    }

    async fn commit_unit_of_work(&mut self) -> Result<(), RepositoryError> {
        // The unit of work will be set to None
        if let Some(current_unit_of_work) = self.current_unit_of_work.take() {
            current_unit_of_work.commit().await.unwrap();
        } else {
            warn!("Attempt to commit a unit of work without having starting one");
        }

        Ok(())
    }
}

// pub type DB = Pool<Postgres>;

// pub trait Queryer {
//   // fn get()...
//   // fn select()...
//   // fn execute()...
// }

/// By having each Repository struct (here Postgres) implementing all of the module resources ports
/// it eliminates the need to share transaction/executor between different resources' repositories.
///
/// It does mean all resources's repositories sharing the same executor needs to be of the same kind (all Postgres, all SQLite).
/// But it makes sense: you can't share a transaction between different kind of databases.
/// 
/// Ok check
/// - question: https://users.rust-lang.org/t/from-slqx-0-7-this-clean-code-doesnt-work-anymore-and-i-cannot-understand-why/97836
/// - changelog: https://github.com/launchbadge/sqlx/blob/main/CHANGELOG.md#breaking
#[async_trait]
impl<'a> SourceMetaRepository2 for PostgresRepository2<'a> {
    #[tracing::instrument(name = "Saving new source meta in database", skip(self))]
    async fn add_source_meta(
        &mut self,
        source_meta: &SourceMeta,
    ) -> Result<(), SourceMetaPostgresRepositoryError> {
        if let Some(db_executor) = &mut self.current_unit_of_work {
            sqlx::query!(
                r#"
        INSERT INTO source_metas (id, user_id, object_store_name, source_type, initial_name, added_at, extracted_at)
        VALUES ($1, $2, $3, $4, $5, $6, NULL)
                "#,
                source_meta.id,
                source_meta.user_id,
                source_meta.object_store_name,
                source_meta.source_type.to_owned() as SourceType,
                source_meta.initial_name.to_string(),
                Utc::now()
            )
            .execute(&mut **db_executor)
            // .execute::<'_, 'a>(&mut *db_executor)
            .await?;
        } else {
            warn!("No db executor");
        }

        Ok(())
    }
}
