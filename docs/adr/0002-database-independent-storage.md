# Database-independent storage

The user requires a replaceable database layer. The memory module owns validation, client grants, revisions, retry checks, tokenization, and MCP response formats. A typed storage interface reads records and receipts, runs scoped keyword searches, and commits a memory change with its search terms and operation receipt atomically.

Each adapter must apply scope filtering before ranking and limiting. Search ranks the number of distinct matching terms from highest to lowest, then memory ID from lowest to highest. The caller serializes access to one store, including reads, retry checks, and writes. An adapter must preserve atomic commits, durability, and this ordering when it replaces Turso.

Turso is the current adapter. Its SQL, schema, connections, file checks, and crash-test hooks stay inside that adapter. Startup chooses the adapter. The existing database format is preserved. Adding another engine requires implementing the storage interface and passing the shared public MCP tests. The optional SurrealDB adapter and persistent backend selection are tracked in [#9](https://github.com/rmkr/tuck/issues/9).
