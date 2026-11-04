use crate::{
    memory::{Failure, Memory, Result, fail},
    operator,
    storage::{Mutation, Operation, Receipt, ReceiptState, SearchHit, SearchQuery, Storage},
};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
};
use surrealdb::{
    Surreal,
    engine::local::{Db, SurrealKv},
    opt::{Config, SyncMode},
    types::SurrealValue,
};

const SEARCH: &str = "SELECT memory_id, count() AS score FROM terms WITH INDEX terms_lookup WHERE scope IN $scopes AND term IN $terms GROUP BY memory_id ORDER BY score DESC, memory_id ASC LIMIT $limit";
const FORMAT: &str = "tuck-surrealdb-1\n";
const MANIFEST: &str = "manifest/00000000000000000000.manifest";

impl From<surrealdb::Error> for Failure {
    fn from(_: surrealdb::Error) -> Self {
        fail(
            "storage_error",
            "Storage failed. Check free space and permissions, then restart Tuck.",
        )
    }
}

pub struct SurrealStorage {
    database: Surreal<Db>,
}

fn storage_error(_: std::io::Error) -> Failure {
    fail(
        "storage_error",
        "Cannot read database. Restore the complete store before starting.",
    )
}

fn private_tree(path: &Path) -> Result<()> {
    operator::private(path).map_err(|_| {
        fail(
            "permissions",
            "Database files must be private and owned by you.",
        )
    })?;
    let metadata = fs::symlink_metadata(path).map_err(storage_error)?;
    if metadata.is_dir() {
        for entry in fs::read_dir(path).map_err(storage_error)? {
            private_tree(&entry.map_err(storage_error)?.path())?;
        }
    } else if !metadata.is_file() {
        return Err(fail(
            "storage_error",
            "Unexpected database file type. Restore the complete store before starting.",
        ));
    }
    Ok(())
}

impl SurrealStorage {
    async fn connect(home: &Path) -> Result<Self> {
        let path = fs::canonicalize(home)
            .map_err(storage_error)?
            .join("memory.surreal");
        // The SDK treats '?' as connection options even when given a PathBuf.
        if path.to_str().is_none_or(|path| path.contains('?')) {
            return Err(fail(
                "path",
                "SurrealDB store path must be UTF-8 without a '?' character.",
            ));
        }
        // SDK 4.0 ignores process environment. Explicit sync makes every commit durable.
        let config = Config::new().datastore_option("experimental_upgrade_from_3x", false);
        let database = Surreal::new::<SurrealKv>((path, config))
            .sync(SyncMode::Every)
            .await?;
        // SurrealKV resets this directory to 0750 on every open, even when it already exists.
        fs::set_permissions(
            home.join("memory.surreal/wal"),
            fs::Permissions::from_mode(0o700),
        )
        .map_err(storage_error)?;
        private_tree(&home.join("memory.surreal"))?;
        database.use_ns("tuck").use_db("tuck").await?;
        Ok(Self { database })
    }

    pub async fn create(home: &Path) -> Result<()> {
        let store = Self::connect(home).await?;
        store
            .database
            .query(
                "
            BEGIN TRANSACTION;
            DEFINE TABLE memories SCHEMALESS;
            DEFINE TABLE terms SCHEMALESS;
            DEFINE INDEX terms_lookup ON terms FIELDS scope, term;
            DEFINE INDEX terms_memory ON terms FIELDS memory_id;
            DEFINE TABLE receipts SCHEMALESS;
            CREATE metadata:format SET version = 1;
            COMMIT TRANSACTION;
        ",
            )
            .await?
            .check()?;
        store.database.shutdown().await?;
        // This marker lets startup reject unsupported stores before the engine can modify files.
        let mut marker = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(home.join("memory.surreal/tuck-format"))
            .map_err(storage_error)?;
        marker.write_all(FORMAT.as_bytes()).map_err(storage_error)?;
        marker.sync_all().map_err(storage_error)?;
        fs::File::open(home.join("memory.surreal"))
            .map_err(storage_error)?
            .sync_all()
            .map_err(storage_error)?;
        fs::File::open(home)
            .map_err(storage_error)?
            .sync_all()
            .map_err(storage_error)?;
        Ok(())
    }

    pub async fn open(home: &Path) -> Result<Self> {
        let path = home.join("memory.surreal");
        private_tree(&path)?;
        if !path.is_dir() {
            return Err(fail(
                "schema",
                "Database is invalid. Restore the complete store before starting.",
            ));
        }
        let format = fs::read(path.join("tuck-format")).map_err(storage_error)?;
        if format != FORMAT.as_bytes() {
            return Err(fail(
                "schema",
                "Unsupported newer schema or uninitialized database. Restore a compatible store.",
            ));
        }
        let mut manifest = [0; 2];
        fs::File::open(path.join(MANIFEST))
            .map_err(storage_error)?
            .read_exact(&mut manifest)
            .map_err(storage_error)?;
        if manifest != [0, 3] {
            return Err(fail(
                "schema",
                "Database format is invalid or unsupported. Restore the complete store before starting.",
            ));
        }
        let store = Self::connect(home).await?;
        let mut response = store
            .database
            .query("SELECT VALUE version FROM metadata:format")
            .await?
            .check()?;
        let versions: Vec<i64> = response.take(0)?;
        if versions != [1] {
            store.database.shutdown().await?;
            return Err(fail(
                "schema",
                "Unsupported newer schema. Use a compatible Tuck version.",
            ));
        }
        Ok(store)
    }
}

#[derive(SurrealValue)]
struct Record {
    record: String,
}
#[derive(SurrealValue)]
struct Ranked {
    memory_id: String,
    score: i64,
}
#[derive(SurrealValue)]
struct StoredReceipt {
    fingerprint: String,
    scope: String,
    memory_id: String,
    revision: i64,
    outcome: String,
}

impl Storage for SurrealStorage {
    async fn read(&self, id: &str) -> Result<Option<Memory>> {
        let mut response = self
            .database
            .query("SELECT record FROM type::record('memories', $id)")
            .bind(("id", id.to_owned()))
            .await?
            .check()?;
        let records: Vec<Record> = response.take(0)?;
        records
            .into_iter()
            .next()
            .map(|r| serde_json::from_str(&r.record).map_err(Into::into))
            .transpose()
    }

    async fn search(&self, query: SearchQuery<'_>) -> Result<Vec<SearchHit>> {
        // Rank indexed scope/term rows first. Only bounded winners load full memory records.
        #[cfg(feature = "test-hooks")]
        if std::env::var_os("TUCK_TEST_ASSERT_INDEX").is_some() {
            let mut plan = self
                .database
                .query(format!("EXPLAIN {SEARCH}"))
                .bind(("scopes", query.scopes.to_vec()))
                .bind(("terms", query.terms.iter().cloned().collect::<Vec<_>>()))
                .bind(("limit", query.limit as i64))
                .await?
                .check()?;
            let plan: surrealdb::types::Value = plan.take(0)?;
            let plan = format!("{plan:?}");
            if !plan.contains("terms_lookup") || plan.contains("TableScan") {
                return Err(fail(
                    "storage_error",
                    "Search must use the scoped term index.",
                ));
            }
            eprintln!("TUCK_TEST_INDEX terms_lookup");
        }
        let mut response = self
            .database
            .query(SEARCH)
            .bind(("scopes", query.scopes.to_vec()))
            .bind(("terms", query.terms.iter().cloned().collect::<Vec<_>>()))
            .bind(("limit", query.limit as i64))
            .await?
            .check()?;
        let ranked: Vec<Ranked> = response.take(0)?;
        let mut hits = Vec::with_capacity(ranked.len());
        for hit in ranked {
            let memory = self.read(&hit.memory_id).await?.ok_or_else(|| {
                fail(
                    "storage_error",
                    "Search index is invalid. Restore a verified backup.",
                )
            })?;
            hits.push(SearchHit {
                memory,
                score: hit.score,
            });
        }
        Ok(hits)
    }

    async fn receipt(
        &self,
        grant_id: &str,
        operation: Operation,
        operation_id: &str,
    ) -> Result<Option<ReceiptState>> {
        let id = operator::hash(
            format!("{grant_id}\0{}\0{operation_id}", operation.as_str()).as_bytes(),
        );
        let mut response = self.database.query("SELECT fingerprint, scope, memory_id, revision, outcome FROM type::record('receipts', $id)")
            .bind(("id", id)).await?.check()?;
        let receipts: Vec<StoredReceipt> = response.take(0)?;
        let Some(stored) = receipts.into_iter().next() else {
            return Ok(None);
        };
        let memory_exists = self.read(&stored.memory_id).await?.is_some();
        Ok(Some(ReceiptState {
            receipt: Receipt {
                grant_id: grant_id.to_owned(),
                operation_id: operation_id.to_owned(),
                fingerprint: stored.fingerprint,
                scope: stored.scope,
                memory_id: stored.memory_id,
                revision: stored.revision,
                outcome: serde_json::from_value(serde_json::Value::String(stored.outcome))?,
            },
            memory_exists,
        }))
    }

    async fn commit(&mut self, mutation: Mutation<'_>) -> Result<()> {
        let memory = mutation.memory;
        let receipt = mutation.receipt;
        let operation = mutation.operation.as_str();
        let tx = self.database.clone().begin().await?;
        match mutation.operation {
            Operation::Save | Operation::Update => {
                let statement = if matches!(mutation.operation, Operation::Save) {
                    "CREATE type::record('memories', $id) SET record = $record"
                } else {
                    "UPDATE type::record('memories', $id) SET record = $record"
                };
                tx.query(statement)
                    .bind(("id", memory.id.clone()))
                    .bind(("record", serde_json::to_string(memory)?))
                    .await?
                    .check()?;
                pause(&format!("{operation}:record")).await?;
                tx.query("DELETE terms WHERE memory_id = $id")
                    .bind(("id", memory.id.clone()))
                    .await?
                    .check()?;
                for term in mutation.terms {
                    tx.query("CREATE terms SET memory_id = $id, scope = $scope, term = $term")
                        .bind(("id", memory.id.clone()))
                        .bind(("scope", memory.scope.clone()))
                        .bind(("term", term))
                        .await?
                        .check()?;
                }
                pause(&format!("{operation}:index")).await?;
            }
            Operation::Delete => {
                tx.query("DELETE terms WHERE memory_id = $id")
                    .bind(("id", memory.id.clone()))
                    .await?
                    .check()?;
                pause("delete:index").await?;
                tx.query("DELETE type::record('memories', $id)")
                    .bind(("id", memory.id.clone()))
                    .await?
                    .check()?;
                pause("delete:record").await?;
            }
        }
        let receipt_id = operator::hash(
            format!(
                "{}\0{operation}\0{}",
                receipt.grant_id, receipt.operation_id
            )
            .as_bytes(),
        );
        tx.query("CREATE type::record('receipts', $id) SET fingerprint = $fingerprint, scope = $scope, memory_id = $memory, revision = $revision, outcome = $outcome")
            .bind(("id", receipt_id)).bind(("fingerprint", receipt.fingerprint.clone()))
            .bind(("scope", receipt.scope.clone())).bind(("memory", receipt.memory_id.clone()))
            .bind(("revision", receipt.revision)).bind(("outcome", receipt.outcome.as_str().to_owned()))
            .await?.check()?;
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
