# Tuck research

Research checked on 2026-11-04.

## Agreed scope

Tuck will share memories between coding tools on one computer. Rust is the implementation language. The user selected the name Tuck. The database choice remains open, with Turso as the leading candidate.

## What the app would do

Build one local Rust service. Each coding tool connects through MCP, the Model Context Protocol. MCP gives apps a common way to call tools and request context. Each client still needs configuration and instructions about when to save or recall a memory. The client controls tool calls. [MCP architecture](https://modelcontextprotocol.io/docs/learn/architecture) · [MCP tools](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)

Use the official `rmcp` Rust SDK. For a shared local service, Streamable HTTP can accept multiple clients. Keep access on the local computer and check client access at the service boundary. One service owns the database connection lifecycle. This is a proposed design. [Rust SDK](https://github.com/modelcontextprotocol/rust-sdk) · [MCP transports](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)

The first operations would save, search, read, correct, and delete memories. A command-line interface could inspect and export them using the same service.

Each memory should contain its text, project or user scope, source, date, and current status. Store user-confirmed facts separately from agent guesses. Return a small set of relevant memories with their sources. Apply access limits before search. A correction should replace the active fact. Deletion should remove the text and its derived search entries. These are design recommendations.

For example, one coding tool saves a project decision and its reason. A second tool recalls that decision in a later session. When the user changes the decision, future searches return its replacement.

## Is shared memory useful?

There is evidence that memory design can improve later answers. LongMemEval tests 500 questions across saved facts, multiple sessions, time, updates, and abstention. The authors report improvements from better indexing and retrieval. Their tested assistants also showed a roughly 30% accuracy drop across sustained interactions. Applying these results to this app requires a coding-workflow trial. [LongMemEval, ICLR 2025](https://arxiv.org/abs/2410.10813)

Reflexion improved coding and other tasks by saving feedback for later attempts. This supports retaining useful lessons. The study concerns repeated task attempts with feedback. Its results provide a reason to test shared coding memory. [Reflexion](https://arxiv.org/abs/2303.11366)

Old memories can cause errors. STALE tests 400 scenarios where later evidence changes an earlier fact. Even its best tested model reached 55.2% overall accuracy. This supports explicit corrections and checks against current project sources. [STALE](https://arxiv.org/abs/2605.06527)

Stored text can also carry harmful instructions. MINJA demonstrates memory injection through agent interactions. Treat memory as evidence, retain its source, and keep permissions separate from retrieved text. [MINJA](https://arxiv.org/abs/2503.03704)

My proposed benefit test uses the same small set of coding tasks with memory enabled and disabled. Measure task success, repeated corrections, response time, and irrelevant recall. Include a changed fact, a deletion, a wrong-project query, and two clients working at once. Keep the answer key outside the memory store.

## Database shortlist

### Turso

Turso Database is the Rust engine. libSQL is a separate SQLite fork. The official Rust guide recommends the `turso` crate for embedded local storage and optional cloud sync. Local reads and writes support offline use. [Turso Rust guide](https://docs.turso.tech/sdk/rust/quickstart) · [libSQL](https://github.com/tursodatabase/libsql)

The current Turso README reports production use and a pre-1.0 status. Full-text search and multi-process file coordination remain experimental. Exact vector search is available. Approximate vector indexing remains on the roadmap. A single database-owning service avoids relying on cross-process file access. Pin the release and check its own compatibility notes before implementation. [Turso status and features](https://github.com/tursodatabase/turso)

Turso's 0.8 benchmark reports about 9,500 transactions per second at 64 connections, compared with about 1,370 for SQLite. It inserts 100 rows per transaction on separate keys. The test ran on a Linux Ryzen desktop with full disk-sync durability. SQLite led at one connection in that throughput test. This supports Turso for that concurrent-write workload. Memory retrieval speed on this Mac remains unmeasured. [Turso benchmark and linked code](https://turso.tech/blog/turso-0.8.0)

Turso is my leading option for a compact first version with structured memories and local storage. That recommendation depends on the text-search trial.

### SurrealDB

SurrealDB is the strongest alternative for richer search. It runs inside Rust and can use the Rust SurrealKV storage engine. It supports structured records, relationships, keyword search, vector indexes, and transactions. Vector search finds similar meaning through numeric representations of text. [Rust embedding](https://surrealdb.com/docs/reference/rust/embedding) · [Hybrid search](https://surrealdb.com/docs/learn/data-models/vector-search/hybrid-search)

Its own SQL-like language and broader feature set add learning and integration work. That work may be worthwhile if text search, meaning-based search, and linked memories are first-version requirements. This is a fit judgement. Comparative speed still requires a workload test.

The core uses Business Source License 1.1. The vendor permits use in applications and reserves some third-party database services for commercial licensing. SDKs have separate permissive licenses. Review the chosen engine release's terms if distributing the app. [License explanation](https://surrealdb.com/license) · [Core license](https://github.com/surrealdb/surrealdb/blob/main/LICENSE)

The published crate lists 4.0.0, released November 3, 2026. Some documentation still describes beta status. Pinning a release and checking its APIs matters here too. [Published crate](https://docs.rs/crate/surrealdb/4.0.0)

## Other engines examined

`redb` offers a compact Rust key-value store with crash-safe transactions and a stable file format. It allows one write transaction at a time. The app would need to supply its search layer. [redb](https://github.com/cberner/redb)

Fjall offers Rust key-value storage, atomic batches, and optional transactions. Its default persistence flush reaches operating-system buffers. Stronger power-loss durability requires an explicit sync mode. Search remains application work. [Fjall API](https://docs.rs/fjall/latest/fjall/) · [Persistence modes](https://docs.rs/fjall/latest/fjall/enum.PersistMode.html)

LanceDB offers embedded Rust access, vector indexes, keyword search, filtering, and automatic data versions. It is worth revisiting if large-scale semantic retrieval becomes the main requirement. Its Rust interface uses Arrow data batches, which adds integration work for a small record-oriented app. [Rust API](https://docs.rs/lancedb/latest/lancedb/) · [Full-text query API](https://docs.rs/lancedb/latest/lancedb/query/trait.QueryBase.html)

SQLite through `rusqlite` remains a fallback with mature full-text search. The database engine itself is C. [rusqlite](https://github.com/rusqlite/rusqlite) · [SQLite FTS5](https://sqlite.org/fts5.html)

## Next decision

Compare Turso and SurrealDB using realistic saves, searches, corrections, deletions, and concurrent clients. Judge result quality and latency together. Choose the engine after that small test.

This work produced research only. Implementation, client integration, recovery tests, and performance tests remain pending.
