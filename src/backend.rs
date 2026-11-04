use crate::{
    memory::{Memory, Result, fail},
    operator::Backend,
    storage::{Mutation, Operation, ReceiptState, SearchHit, SearchQuery, Storage},
    surreal::SurrealStorage,
    turso::TursoStorage,
};
use std::path::Path;

pub enum BackendStorage {
    Turso(TursoStorage),
    Surrealdb(SurrealStorage),
}

impl BackendStorage {
    pub async fn create(home: &Path, backend: Backend) -> Result<()> {
        match backend {
            Backend::Turso => TursoStorage::create(home).await.map(|_| ()),
            Backend::Surrealdb => SurrealStorage::create(home).await,
        }
    }

    pub async fn open(home: &Path, backend: Backend) -> Result<Self> {
        let turso = home.join("memory.db").symlink_metadata().is_ok();
        let surreal = home.join("memory.surreal").symlink_metadata().is_ok();
        if !matches!(
            (backend, turso, surreal),
            (Backend::Turso, true, false) | (Backend::Surrealdb, false, true)
        ) {
            return Err(fail(
                "storage_error",
                "Database is missing or backend does not match config.json. Restore the complete store before starting.",
            ));
        }
        match backend {
            Backend::Turso => Ok(Self::Turso(TursoStorage::open(home).await?)),
            Backend::Surrealdb => Ok(Self::Surrealdb(SurrealStorage::open(home).await?)),
        }
    }
}

impl Storage for BackendStorage {
    async fn read(&self, id: &str) -> Result<Option<Memory>> {
        match self {
            Self::Turso(store) => store.read(id).await,
            Self::Surrealdb(store) => store.read(id).await,
        }
    }
    async fn search(&self, query: SearchQuery<'_>) -> Result<Vec<SearchHit>> {
        match self {
            Self::Turso(store) => store.search(query).await,
            Self::Surrealdb(store) => store.search(query).await,
        }
    }
    async fn receipt(
        &self,
        grant_id: &str,
        operation: Operation,
        operation_id: &str,
    ) -> Result<Option<ReceiptState>> {
        match self {
            Self::Turso(store) => store.receipt(grant_id, operation, operation_id).await,
            Self::Surrealdb(store) => store.receipt(grant_id, operation, operation_id).await,
        }
    }
    async fn commit(&mut self, mutation: Mutation<'_>) -> Result<()> {
        match self {
            Self::Turso(store) => store.commit(mutation).await,
            Self::Surrealdb(store) => store.commit(mutation).await,
        }
    }
}
