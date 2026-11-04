use crate::{
    memory::{Failure, Memory, Result, fail},
    operator,
    storage::{Mutation, Operation, Receipt, ReceiptState, SearchHit, SearchQuery, Storage},
};
use std::path::Path;
use turso::{Connection, Database, params};

impl From<turso::Error> for Failure {
    fn from(_: turso::Error) -> Self {
        fail(
            "storage_error",
            "Storage failed. Check free space and permissions, then restart Tuck.",
        )
    }
}

pub struct TursoStorage {
    connection: Connection,
    _database: Database,
}
impl TursoStorage {
    async fn connect(home: &Path, read_only: bool) -> Result<Self> {
        for name in ["memory.db", "memory.db-wal", "memory.db-shm"] {
            let path = home.join(name);
            if path.symlink_metadata().is_ok() {
                operator::private(&path).map_err(|_| {
                    fail(
                        "permissions",
                        "Database files must be private and owned by you.",
                    )
                })?;
            }
        }
        let path = home.join("memory.db");
        let db = turso::Builder::new_local(
            path.to_str()
                .ok_or_else(|| fail("path", "Store path must be UTF-8."))?,
        )
        .read_only(read_only)
        .build()
        .await?;
        let connection = db.connect()?;
        if !read_only {
            connection.execute("PRAGMA synchronous=FULL", ()).await?;
        }
        Ok(Self {
            connection,
            _database: db,
        })
    }
    pub async fn open(home: &Path) -> Result<Self> {
        if !home.join("memory.db").is_file() {
            return Err(fail(
                "storage_error",
                "Database is missing. Restore the complete store before starting.",
            ));
        }
        let metadata = std::fs::symlink_metadata(home.join("memory.db")).map_err(|_| {
            fail(
                "storage_error",
                "Database is missing. Restore the complete store before starting.",
            )
        })?;
        if metadata.len() == 0 {
            return Err(fail(
                "schema",
                "Database is empty. Restore the complete store before starting.",
            ));
        }
        let store = Self::connect(home, true).await?;
        let mut rows = store.connection.query("PRAGMA user_version", ()).await?;
        let version = rows
            .next()
            .await?
            .ok_or_else(|| fail("schema", "Cannot read schema version."))?
            .get::<i64>(0)?;
        drop(rows);
        if version == 0 {
            return Err(fail(
                "schema",
                "Database is uninitialized. Restore the complete store before starting.",
            ));
        }
        if version != 1 {
            return Err(fail(
                "schema",
                "Unsupported newer schema. Use a compatible Tuck version.",
            ));
        }
        drop(store);
        Self::connect(home, false).await
    }
    pub async fn create(home: &Path) -> Result<Self> {
        if home.join("memory.db").exists() {
            return Err(fail(
                "storage_error",
                "Store already exists. Choose another --home or use serve.",
            ));
        }
        let mut store = Self::connect(home, false).await?;
        let tx = store.connection.transaction().await?;
        tx.execute("CREATE TABLE memories (id TEXT PRIMARY KEY, scope TEXT NOT NULL, revision INTEGER NOT NULL, record TEXT NOT NULL)", ()).await?;
        tx.execute("CREATE TABLE terms (term TEXT NOT NULL, memory_id TEXT NOT NULL, PRIMARY KEY(term, memory_id))", ()).await?;
        tx.execute("CREATE INDEX terms_memory ON terms(memory_id)", ())
            .await?;
        tx.execute("CREATE TABLE receipts (grant_id TEXT NOT NULL, operation TEXT NOT NULL, operation_id TEXT NOT NULL, fingerprint TEXT NOT NULL, scope TEXT NOT NULL, memory_id TEXT NOT NULL, revision INTEGER NOT NULL, outcome TEXT NOT NULL, PRIMARY KEY(grant_id, operation, operation_id))", ()).await?;
        tx.execute("PRAGMA user_version=1", ()).await?;
        tx.commit().await?;
        Ok(store)
    }
}
impl Storage for TursoStorage {
    async fn read(&self, id: &str) -> Result<Option<Memory>> {
        let mut rows = self
            .connection
            .query("SELECT record FROM memories WHERE id=?", [id])
            .await?;
        let Some(row) = rows.next().await? else {
            return Ok(None);
        };
        Ok(Some(serde_json::from_str(&row.get::<String>(0)?)?))
    }
    async fn search(&self, query: SearchQuery<'_>) -> Result<Vec<SearchHit>> {
        let placeholders = |n| std::iter::repeat_n("?", n).collect::<Vec<_>>().join(",");
        // Rank small ID rows before loading full records for the bounded result.
        let sql = format!(
            "SELECT m.record, ranked.score FROM (
                SELECT t.memory_id, COUNT(*) AS score
                FROM terms t JOIN memories allowed ON allowed.id=t.memory_id
                WHERE t.term IN ({}) AND allowed.scope IN ({})
                GROUP BY t.memory_id ORDER BY score DESC, t.memory_id ASC LIMIT ?
            ) ranked JOIN memories m ON m.id=ranked.memory_id
            ORDER BY ranked.score DESC, ranked.memory_id ASC",
            placeholders(query.terms.len()),
            placeholders(query.scopes.len())
        );
        let mut values = query
            .terms
            .iter()
            .chain(query.scopes)
            .cloned()
            .map(turso::Value::Text)
            .collect::<Vec<_>>();
        values.push(turso::Value::Integer(query.limit as i64));
        let mut rows = self
            .connection
            .query(&sql, turso::params_from_iter(values))
            .await?;
        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            results.push(SearchHit {
                memory: serde_json::from_str(&row.get::<String>(0)?)?,
                score: row.get::<i64>(1)?,
            });
        }
        Ok(results)
    }
    async fn receipt(
        &self,
        grant_id: &str,
        operation: Operation,
        operation_id: &str,
    ) -> Result<Option<ReceiptState>> {
        let mut rows = self.connection.query("SELECT fingerprint, scope, memory_id, revision, outcome FROM receipts WHERE grant_id=? AND operation=? AND operation_id=?", params![grant_id, operation.as_str(), operation_id]).await?;
        let Some(row) = rows.next().await? else {
            return Ok(None);
        };
        let receipt = Receipt {
            grant_id: grant_id.to_owned(),
            operation_id: operation_id.to_owned(),
            fingerprint: row.get(0)?,
            scope: row.get(1)?,
            memory_id: row.get(2)?,
            revision: row.get(3)?,
            outcome: serde_json::from_value(serde_json::Value::String(row.get(4)?))?,
        };
        let mut present = self
            .connection
            .query(
                "SELECT id FROM memories WHERE id=?",
                [receipt.memory_id.clone()],
            )
            .await?;
        Ok(Some(ReceiptState {
            receipt,
            memory_exists: present.next().await?.is_some(),
        }))
    }
    async fn commit(&mut self, mutation: Mutation<'_>) -> Result<()> {
        let memory = mutation.memory;
        let receipt = mutation.receipt;
        let operation = mutation.operation.as_str();
        let tx = self.connection.transaction().await?;
        match mutation.operation {
            Operation::Save => {
                tx.execute(
                    "INSERT INTO memories VALUES (?, ?, ?, ?)",
                    params![
                        memory.id.clone(),
                        memory.scope.clone(),
                        memory.revision,
                        serde_json::to_string(memory)?
                    ],
                )
                .await?;
                pause("save:record").await?;
            }
            Operation::Update => {
                tx.execute(
                    "UPDATE memories SET revision=?, record=? WHERE id=?",
                    params![
                        memory.revision,
                        serde_json::to_string(memory)?,
                        memory.id.clone()
                    ],
                )
                .await?;
                pause("update:record").await?;
                tx.execute("DELETE FROM terms WHERE memory_id=?", [memory.id.clone()])
                    .await?;
            }
            Operation::Delete => {
                tx.execute("DELETE FROM terms WHERE memory_id=?", [memory.id.clone()])
                    .await?;
                pause("delete:index").await?;
                tx.execute("DELETE FROM memories WHERE id=?", [memory.id.clone()])
                    .await?;
                pause("delete:record").await?;
            }
        }
        if !matches!(mutation.operation, Operation::Delete) {
            for term in mutation.terms {
                tx.execute(
                    "INSERT INTO terms VALUES (?, ?)",
                    params![term, memory.id.clone()],
                )
                .await?;
            }
            pause(&format!("{operation}:index")).await?;
        }
        tx.execute(
            "INSERT INTO receipts VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                receipt.grant_id.clone(),
                operation,
                receipt.operation_id.clone(),
                receipt.fingerprint.clone(),
                receipt.scope.clone(),
                receipt.memory_id.clone(),
                receipt.revision,
                receipt.outcome.as_str(),
            ],
        )
        .await?;
        pause(&format!("{operation}:receipt")).await?;
        pause(&format!("{operation}:before_commit")).await?;
        tx.commit().await?;
        pause(&format!("{operation}:after_commit")).await?;
        Ok(())
    }
}

async fn pause(_stage: &str) -> Result<()> {
    #[cfg(feature = "test-hooks")]
    if std::env::var("TUCK_TEST_FAIL").as_deref() == Ok(_stage) {
        return Err(fail("storage_error", "Injected storage write failure."));
    }
    #[cfg(feature = "test-hooks")]
    if std::env::var("TUCK_TEST_PAUSE").as_deref() == Ok(_stage) {
        eprintln!("TUCK_TEST_PAUSED {_stage}");
        std::future::pending::<()>().await;
    }
    Ok(())
}
