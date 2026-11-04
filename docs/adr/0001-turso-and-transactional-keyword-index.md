# Turso with a transactional keyword index

Turso remains the current storage adapter. [ADR 0002](0002-database-independent-storage.md) records the later decision to keep memory rules independent of the engine.

Tuck uses pinned Turso 0.8.3 with one database-owning process and one serialized connection. The public MCP tests passed persistence, source and identifier search, concurrent corrections, retry receipts, deletion, and twelve forced-stop positions across save, update, and delete.

Search uses a regular indexed table of unique terms. Content, terms, revisions, and operation receipts commit in one transaction with `synchronous=FULL`. This meets the initial keyword contract without experimental full-text search or multi-process coordination. SurrealDB remains unevaluated because the required Turso correctness and search checks passed. Performance evidence is separate from this correctness decision.

The test entry point is `cargo test --locked --features test-hooks`. The compile-time feature enables deterministic pauses inside mutations. Release builds omit it. Tests use the executable, HTTP MCP requests, and isolated on-disk databases.

The selected APIs were checked against the published crate source. The official references describe the engine and transport. [Turso 0.8.3](https://docs.rs/turso/0.8.3/turso/) · [Turso transaction API](https://docs.rs/turso/0.8.3/turso/transaction/struct.Transaction.html) · [rmcp 3.8.0](https://docs.rs/rmcp/3.8.0/rmcp/) · [MCP transport requirements](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)
