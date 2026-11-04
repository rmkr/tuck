use crate::memory::{Memory, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, future::Future};

#[derive(Clone, Copy)]
pub enum Operation {
    Save,
    Update,
    Delete,
}
impl Operation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Save => "save",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }
    pub fn outcome(self) -> Outcome {
        match self {
            Self::Save => Outcome::Saved,
            Self::Update => Outcome::Updated,
            Self::Delete => Outcome::Deleted,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Saved,
    Updated,
    Deleted,
}

impl Outcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Saved => "saved",
            Self::Updated => "updated",
            Self::Deleted => "deleted",
        }
    }
}

// Receipts retain mutation metadata only. They never retain memory content or sources.
pub struct Receipt {
    pub grant_id: String,
    pub operation_id: String,
    pub fingerprint: String,
    pub scope: String,
    pub memory_id: String,
    pub revision: i64,
    pub outcome: Outcome,
}
pub struct ReceiptState {
    pub receipt: Receipt,
    pub memory_exists: bool,
}

pub struct SearchQuery<'a> {
    pub terms: &'a BTreeSet<String>,
    pub scopes: &'a [String],
    pub limit: usize,
}
pub struct SearchHit {
    pub memory: Memory,
    pub score: i64,
}
pub struct Mutation<'a> {
    pub operation: Operation,
    pub memory: &'a Memory,
    pub terms: BTreeSet<String>,
    pub receipt: &'a Receipt,
}

/// The caller holds a shared lock for each complete get/search operation.
/// Mutations hold an exclusive lock through receipt checks, reads, and durable commit.
/// The caller owns validation, grants, revisions, and retry policy.
pub trait Storage: Send + Sync {
    fn read(&self, id: &str) -> impl Future<Output = Result<Option<Memory>>> + Send;

    /// Match exact terms in the requested scopes before applying the limit.
    /// Score by distinct matched terms, descending, then memory ID ascending.
    fn search(&self, query: SearchQuery<'_>)
    -> impl Future<Output = Result<Vec<SearchHit>>> + Send;

    /// Read the receipt and memory existence under the caller's exclusive lock.
    fn receipt(
        &self,
        grant_id: &str,
        operation: Operation,
        operation_id: &str,
    ) -> impl Future<Output = Result<Option<ReceiptState>>> + Send;

    /// Commit the memory change, replacement terms, and content-free receipt atomically.
    /// Delete removes content and terms while retaining all operation receipts.
    /// Partial changes must never become visible. Return success only after durable commit.
    fn commit(&mut self, mutation: Mutation<'_>) -> impl Future<Output = Result<()>> + Send;
}
