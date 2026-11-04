use crate::operator::{self, Grant};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use turso::{Connection, Database, params};

#[derive(Debug)]
pub struct Failure {
    pub code: &'static str,
    pub message: &'static str,
}
type Result<T> = std::result::Result<T, Failure>;
impl From<turso::Error> for Failure {
    fn from(_: turso::Error) -> Self {
        Self {
            code: "storage_error",
            message: "Storage failed. Check free space and permissions, then restart Tuck.",
        }
    }
}
impl From<serde_json::Error> for Failure {
    fn from(_: serde_json::Error) -> Self {
        Self {
            code: "storage_error",
            message: "Stored record is invalid. Restore a verified backup.",
        }
    }
}
fn fail(code: &'static str, message: &'static str) -> Failure {
    Failure { code, message }
}
#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Preference,
    Decision,
    Fact,
    Lesson,
}
#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    UserStatement,
    ProjectArtifact,
    AgentInference,
}
#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub reference: String,
    pub origin: Origin,
}
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Save {
    pub scope: String,
    pub kind: Kind,
    pub content: String,
    pub source: Source,
    pub operation_id: String,
}
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Get {
    pub id: String,
}
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Search {
    pub query: String,
    pub scopes: Vec<String>,
    pub limit: Option<usize>,
}
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Update {
    pub id: String,
    pub expected_revision: i64,
    pub content: String,
    pub source: Source,
    pub operation_id: String,
}
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Delete {
    pub id: String,
    pub expected_revision: i64,
    pub operation_id: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct Memory {
    id: String,
    scope: String,
    kind: Kind,
    content: String,
    source: Source,
    created_at: u64,
    updated_at: u64,
    revision: i64,
    writer: String,
}

pub struct Store {
    connection: Connection,
    _database: Database,
}
impl Store {
    pub async fn open(home: &Path) -> Result<Self> {
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
        .build()
        .await?;
        let mut connection = db.connect()?;
        connection.execute("PRAGMA synchronous=FULL", ()).await?;
        let mut rows = connection.query("PRAGMA user_version", ()).await?;
        let version = rows
            .next()
            .await?
            .ok_or_else(|| fail("schema", "Cannot read schema version."))?
            .get::<i64>(0)?;
        drop(rows);
        if version != 0 && version != 1 {
            return Err(fail(
                "schema",
                "Unsupported newer schema. Use a compatible Tuck version.",
            ));
        }
        if version == 0 {
            let tx = connection.transaction().await?;
            tx.execute("CREATE TABLE memories (id TEXT PRIMARY KEY, scope TEXT NOT NULL, revision INTEGER NOT NULL, record TEXT NOT NULL)", ()).await?;
            tx.execute("CREATE TABLE terms (term TEXT NOT NULL, memory_id TEXT NOT NULL, PRIMARY KEY(term, memory_id))", ()).await?;
            tx.execute("CREATE INDEX terms_memory ON terms(memory_id)", ())
                .await?;
            tx.execute("CREATE TABLE receipts (grant_id TEXT NOT NULL, operation TEXT NOT NULL, operation_id TEXT NOT NULL, fingerprint TEXT NOT NULL, scope TEXT NOT NULL, memory_id TEXT NOT NULL, revision INTEGER NOT NULL, outcome TEXT NOT NULL, PRIMARY KEY(grant_id, operation, operation_id))", ()).await?;
            tx.execute("PRAGMA user_version=1", ()).await?;
            tx.commit().await?;
        }
        Ok(Self {
            connection,
            _database: db,
        })
    }
    pub async fn get(&self, grant: &Grant, id: &str) -> Result<Value> {
        let memory = self.read(grant, id, false).await?;
        Ok(serde_json::to_value(memory)?)
    }
    async fn read(&self, grant: &Grant, id: &str, write: bool) -> Result<Memory> {
        if !operator::identifier(id) {
            return Err(fail("validation", "Provide a valid memory ID."));
        }
        if (write && !grant.write) || (!write && !grant.read) {
            return Err(fail("forbidden", "Grant does not permit this operation."));
        }
        let mut rows = self
            .connection
            .query("SELECT record FROM memories WHERE id=?", [id])
            .await?;
        let row = rows
            .next()
            .await?
            .ok_or_else(|| fail("not_found", "Memory not found."))?;
        let memory: Memory = serde_json::from_str(&row.get::<String>(0)?)?;
        if !grant.scopes.contains(&memory.scope) {
            return Err(fail("not_found", "Memory not found."));
        }
        Ok(memory)
    }
    pub async fn save(&mut self, grant: &Grant, args: Save) -> Result<Value> {
        validate_text(&args.content, &args.source, &args.operation_id)?;
        if !grant.write || !grant.scopes.contains(&args.scope) {
            return Err(fail(
                "forbidden",
                "Grant does not permit writing this scope.",
            ));
        }
        let fingerprint = operator::hash(&serde_json::to_vec(&args)?);
        if let Some(receipt) = self
            .receipt(grant, "save", &args.operation_id, &fingerprint)
            .await?
        {
            return Ok(receipt);
        }
        let now = timestamp();
        let memory = Memory {
            id: uuid::Uuid::new_v4().to_string(),
            scope: args.scope,
            kind: args.kind,
            content: args.content,
            source: args.source,
            created_at: now,
            updated_at: now,
            revision: 1,
            writer: grant.id.clone(),
        };
        let tx = self.connection.transaction().await?;
        tx.execute(
            "INSERT INTO memories VALUES (?, ?, ?, ?)",
            params![
                memory.id.clone(),
                memory.scope.clone(),
                memory.revision,
                serde_json::to_string(&memory)?
            ],
        )
        .await?;
        pause("save:record").await;
        index(&tx, &memory).await?;
        pause("save:index").await;
        record_receipt(
            &tx,
            grant,
            "save",
            &args.operation_id,
            &fingerprint,
            &memory,
            "saved",
        )
        .await?;
        pause("save:before_commit").await;
        tx.commit().await?;
        pause("save:after_commit").await;
        Ok(json!({"id":memory.id,"revision":1,"outcome":"saved"}))
    }
    pub async fn search(&self, grant: &Grant, args: Search) -> Result<Value> {
        let limit = args.limit.unwrap_or(5);
        if args.query.trim().is_empty()
            || args.query.len() > 1024
            || args.scopes.is_empty()
            || args.scopes.len() > 128
            || !(1..=20).contains(&limit)
        {
            return Err(fail(
                "validation",
                "Query must be 1–1024 bytes, scopes nonempty, and limit 1–20.",
            ));
        }
        if !grant.read || args.scopes.iter().any(|s| !grant.scopes.contains(s)) {
            return Err(fail(
                "forbidden",
                "Grant does not permit reading these scopes.",
            ));
        }
        let terms = tokens(&args.query);
        if terms.is_empty() {
            return Ok(json!([]));
        }
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
            placeholders(terms.len()),
            placeholders(args.scopes.len())
        );
        let mut values = terms
            .into_iter()
            .chain(args.scopes)
            .map(turso::Value::Text)
            .collect::<Vec<_>>();
        values.push(turso::Value::Integer(limit as i64));
        let mut rows = self
            .connection
            .query(&sql, turso::params_from_iter(values))
            .await?;
        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            let memory: Memory = serde_json::from_str(&row.get::<String>(0)?)?;
            results.push(json!({"id":memory.id,"scope":memory.scope,"kind":memory.kind,"excerpt":memory.content.chars().take(512).collect::<String>(),"source":memory.source,"created_at":memory.created_at,"updated_at":memory.updated_at,"revision":memory.revision,"writer":memory.writer,"score":row.get::<i64>(1)?}));
        }
        Ok(Value::Array(results))
    }
    async fn receipt(
        &self,
        grant: &Grant,
        operation: &str,
        operation_id: &str,
        fingerprint: &str,
    ) -> Result<Option<Value>> {
        let mut rows = self.connection.query("SELECT fingerprint, scope, memory_id, revision, outcome FROM receipts WHERE grant_id=? AND operation=? AND operation_id=?", params![grant.id.clone(), operation, operation_id]).await?;
        let Some(row) = rows.next().await? else {
            return Ok(None);
        };
        if !grant.write || !grant.scopes.contains(&row.get::<String>(1)?) {
            return Err(fail("forbidden", "Grant does not permit this operation."));
        }
        if row.get::<String>(0)? != fingerprint {
            return Err(fail(
                "conflict",
                "Operation ID was already used with different input.",
            ));
        }
        let id = row.get::<String>(2)?;
        let mut present = self
            .connection
            .query("SELECT id FROM memories WHERE id=?", [id.clone()])
            .await?;
        let outcome = if present.next().await?.is_none() {
            "deleted".to_string()
        } else {
            row.get::<String>(4)?
        };
        Ok(Some(
            json!({"id":id,"revision":row.get::<i64>(3)?,"outcome":outcome}),
        ))
    }
    pub async fn update(&mut self, grant: &Grant, args: Update) -> Result<Value> {
        validate_text(&args.content, &args.source, &args.operation_id)?;
        validate_mutation(&args.id, args.expected_revision)?;
        let fingerprint = operator::hash(&serde_json::to_vec(&args)?);
        if let Some(receipt) = self
            .receipt(grant, "update", &args.operation_id, &fingerprint)
            .await?
        {
            return Ok(receipt);
        }
        let mut memory = self.read(grant, &args.id, true).await?;
        if memory.revision != args.expected_revision {
            return Err(fail(
                "conflict",
                "Revision changed. Read the current memory before retrying.",
            ));
        }
        memory.revision += 1;
        memory.content = args.content;
        memory.source = args.source;
        memory.updated_at = timestamp();
        memory.writer = grant.id.clone();
        let tx = self.connection.transaction().await?;
        tx.execute(
            "UPDATE memories SET revision=?, record=? WHERE id=?",
            params![
                memory.revision,
                serde_json::to_string(&memory)?,
                memory.id.clone()
            ],
        )
        .await?;
        pause("update:record").await;
        tx.execute("DELETE FROM terms WHERE memory_id=?", [memory.id.clone()])
            .await?;
        index(&tx, &memory).await?;
        pause("update:index").await;
        record_receipt(
            &tx,
            grant,
            "update",
            &args.operation_id,
            &fingerprint,
            &memory,
            "updated",
        )
        .await?;
        pause("update:before_commit").await;
        tx.commit().await?;
        pause("update:after_commit").await;
        Ok(json!({"id":memory.id,"revision":memory.revision,"outcome":"updated"}))
    }
    pub async fn delete(&mut self, grant: &Grant, args: Delete) -> Result<Value> {
        validate_mutation(&args.id, args.expected_revision)?;
        if !operator::identifier(&args.operation_id) {
            return Err(fail("validation", "Provide a valid operation_id."));
        }
        let fingerprint = operator::hash(&serde_json::to_vec(&args)?);
        if let Some(receipt) = self
            .receipt(grant, "delete", &args.operation_id, &fingerprint)
            .await?
        {
            return Ok(receipt);
        }
        let memory = self.read(grant, &args.id, true).await?;
        if memory.revision != args.expected_revision {
            return Err(fail(
                "conflict",
                "Revision changed. Read the current memory before retrying.",
            ));
        }
        let tx = self.connection.transaction().await?;
        tx.execute("DELETE FROM terms WHERE memory_id=?", [memory.id.clone()])
            .await?;
        pause("delete:index").await;
        tx.execute("DELETE FROM memories WHERE id=?", [memory.id.clone()])
            .await?;
        pause("delete:record").await;
        record_receipt(
            &tx,
            grant,
            "delete",
            &args.operation_id,
            &fingerprint,
            &memory,
            "deleted",
        )
        .await?;
        pause("delete:before_commit").await;
        tx.commit().await?;
        pause("delete:after_commit").await;
        Ok(json!({"id":memory.id,"revision":memory.revision,"outcome":"deleted"}))
    }
}
fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn validate_text(content: &str, source: &Source, operation: &str) -> Result<()> {
    if content.trim().is_empty()
        || content.len() > 8192
        || source.reference.trim().is_empty()
        || source.reference.len() > 2048
        || !operator::identifier(operation)
    {
        return Err(fail(
            "validation",
            "Content must be 1–8192 bytes, source reference 1–2048 bytes, and operation_id a valid 1–128 character ID.",
        ));
    }
    Ok(())
}
fn tokens(text: &str) -> BTreeSet<String> {
    let lower = text.to_lowercase();
    lower
        .split_whitespace()
        .map(|s| s.trim_matches(|c: char| !c.is_alphanumeric() && !"_:+.#/-".contains(c)))
        .chain(lower.split(|c: char| !c.is_alphanumeric()))
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}
async fn index(tx: &turso::transaction::Transaction<'_>, memory: &Memory) -> Result<()> {
    for term in tokens(&format!("{} {}", memory.content, memory.source.reference)) {
        tx.execute(
            "INSERT INTO terms VALUES (?, ?)",
            params![term, memory.id.clone()],
        )
        .await?;
    }
    Ok(())
}

fn validate_mutation(id: &str, revision: i64) -> Result<()> {
    if !operator::identifier(id) || !(1..i64::MAX).contains(&revision) {
        return Err(fail(
            "validation",
            "Provide a valid memory ID and positive expected_revision below i64::MAX.",
        ));
    }
    Ok(())
}
async fn record_receipt(
    tx: &turso::transaction::Transaction<'_>,
    grant: &Grant,
    operation: &str,
    operation_id: &str,
    fingerprint: &str,
    memory: &Memory,
    outcome: &str,
) -> Result<()> {
    tx.execute(
        "INSERT INTO receipts VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            grant.id.clone(),
            operation,
            operation_id,
            fingerprint,
            memory.scope.clone(),
            memory.id.clone(),
            memory.revision,
            outcome
        ],
    )
    .await?;
    Ok(())
}

async fn pause(_stage: &str) {
    #[cfg(feature = "test-hooks")]
    if std::env::var("TUCK_TEST_PAUSE").as_deref() == Ok(_stage) {
        eprintln!("TUCK_TEST_PAUSED {_stage}");
        std::future::pending::<()>().await;
    }
}
