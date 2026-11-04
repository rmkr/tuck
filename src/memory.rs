use crate::{
    operator::{self, Grant},
    storage::{Mutation, Operation, Outcome, Receipt, SearchQuery, Storage},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug)]
pub struct Failure {
    pub code: &'static str,
    pub message: &'static str,
}
pub type Result<T> = std::result::Result<T, Failure>;
impl From<serde_json::Error> for Failure {
    fn from(_: serde_json::Error) -> Self {
        Self {
            code: "storage_error",
            message: "Stored record is invalid. Restore a verified backup.",
        }
    }
}
pub fn fail(code: &'static str, message: &'static str) -> Failure {
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
pub struct Memory {
    pub id: String,
    pub scope: String,
    pub kind: Kind,
    pub content: String,
    pub source: Source,
    pub created_at: u64,
    pub updated_at: u64,
    pub revision: i64,
    pub writer: String,
}

pub struct Store<S> {
    storage: S,
}
impl<S: Storage> Store<S> {
    pub fn new(storage: S) -> Self {
        Self { storage }
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
        let memory = self
            .storage
            .read(id)
            .await?
            .ok_or_else(|| fail("not_found", "Memory not found."))?;
        if !grant.scopes.contains(&memory.scope) {
            return Err(fail("not_found", "Memory not found."));
        }
        Ok(memory)
    }
    pub async fn save(&mut self, grant: &Grant, args: Save) -> Result<Value> {
        validate_text(&args.content, &args.source, &args.operation_id)?;
        if !operator::identifier(&args.scope) {
            return Err(fail("validation", "Provide a valid scope ID."));
        }
        if !grant.write || !grant.scopes.contains(&args.scope) {
            return Err(fail(
                "forbidden",
                "Grant does not permit writing this scope.",
            ));
        }
        let fingerprint = operator::hash(&serde_json::to_vec(&args)?);
        if let Some(receipt) = self
            .receipt(grant, Operation::Save, &args.operation_id, &fingerprint)
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
        self.commit(
            grant,
            Operation::Save,
            &args.operation_id,
            &fingerprint,
            &memory,
        )
        .await
    }
    pub async fn search(&self, grant: &Grant, args: Search) -> Result<Value> {
        let limit = args.limit.unwrap_or(5);
        if args.query.trim().is_empty()
            || args.query.len() > 1024
            || args.scopes.is_empty()
            || args.scopes.len() > 128
            || args.scopes.iter().any(|scope| !operator::identifier(scope))
            || !(1..=20).contains(&limit)
        {
            return Err(fail(
                "validation",
                "Query must be 1–1024 bytes, scope IDs valid and nonempty, and limit 1–20.",
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
        let hits = self
            .storage
            .search(SearchQuery {
                terms: &terms,
                scopes: &args.scopes,
                limit,
            })
            .await?;
        let results = hits.into_iter().map(|hit| {
            let memory = hit.memory;
            json!({"id":memory.id,"scope":memory.scope,"kind":memory.kind,"excerpt":memory.content.chars().take(512).collect::<String>(),"source":memory.source,"created_at":memory.created_at,"updated_at":memory.updated_at,"revision":memory.revision,"writer":memory.writer,"score":hit.score})
        }).collect();
        Ok(Value::Array(results))
    }
    async fn receipt(
        &self,
        grant: &Grant,
        operation: Operation,
        operation_id: &str,
        fingerprint: &str,
    ) -> Result<Option<Value>> {
        let Some(state) = self
            .storage
            .receipt(&grant.id, operation, operation_id)
            .await?
        else {
            return Ok(None);
        };
        let mut receipt = state.receipt;
        if !grant.write || !grant.scopes.contains(&receipt.scope) {
            return Err(fail("forbidden", "Grant does not permit this operation."));
        }
        if receipt.fingerprint != fingerprint {
            return Err(fail(
                "conflict",
                "Operation ID was already used with different input.",
            ));
        }
        if !state.memory_exists {
            receipt.outcome = Outcome::Deleted;
        }
        Ok(Some(receipt.response()))
    }
    pub async fn update(&mut self, grant: &Grant, args: Update) -> Result<Value> {
        validate_text(&args.content, &args.source, &args.operation_id)?;
        validate_mutation(&args.id, args.expected_revision)?;
        let fingerprint = operator::hash(&serde_json::to_vec(&args)?);
        if let Some(receipt) = self
            .receipt(grant, Operation::Update, &args.operation_id, &fingerprint)
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
        self.commit(
            grant,
            Operation::Update,
            &args.operation_id,
            &fingerprint,
            &memory,
        )
        .await
    }
    pub async fn delete(&mut self, grant: &Grant, args: Delete) -> Result<Value> {
        validate_mutation(&args.id, args.expected_revision)?;
        if !operator::identifier(&args.operation_id) {
            return Err(fail("validation", "Provide a valid operation_id."));
        }
        let fingerprint = operator::hash(&serde_json::to_vec(&args)?);
        if let Some(receipt) = self
            .receipt(grant, Operation::Delete, &args.operation_id, &fingerprint)
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
        self.commit(
            grant,
            Operation::Delete,
            &args.operation_id,
            &fingerprint,
            &memory,
        )
        .await
    }
    async fn commit(
        &mut self,
        grant: &Grant,
        operation: Operation,
        operation_id: &str,
        fingerprint: &str,
        memory: &Memory,
    ) -> Result<Value> {
        let receipt = Receipt {
            grant_id: grant.id.clone(),
            operation_id: operation_id.to_owned(),
            fingerprint: fingerprint.to_owned(),
            scope: memory.scope.clone(),
            memory_id: memory.id.clone(),
            revision: memory.revision,
            outcome: operation.outcome(),
        };
        let terms = match operation {
            Operation::Save | Operation::Update => {
                tokens(&format!("{} {}", memory.content, memory.source.reference))
            }
            Operation::Delete => BTreeSet::new(),
        };
        self.storage
            .commit(Mutation {
                operation,
                memory,
                terms,
                receipt: &receipt,
            })
            .await?;
        Ok(receipt.response())
    }
}
impl Receipt {
    fn response(&self) -> Value {
        json!({"id":self.memory_id,"revision":self.revision,"outcome":self.outcome})
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
fn validate_mutation(id: &str, revision: i64) -> Result<()> {
    if !operator::identifier(id) || !(1..i64::MAX).contains(&revision) {
        return Err(fail(
            "validation",
            "Provide a valid memory ID and positive expected_revision below i64::MAX.",
        ));
    }
    Ok(())
}
