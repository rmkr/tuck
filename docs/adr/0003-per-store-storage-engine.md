# Select a local storage engine per store

The owner can select Turso or SurrealDB when initializing a new store. Turso is the default. Configuration records the backend. Existing configuration without that field identifies a legacy Turso store. A missing or conflicting storage layout fails before opening the selected engine. A new backend requires a separate store directory.

The SurrealDB adapter embeds pinned SurrealDB 4.0.0 with SurrealKV 1.2.0 from the lockfile. It implements the shared storage interface, including one transaction for a memory change, its terms, and its operation receipt. A format marker guards missing, uninitialized, or unsupported stores before the SDK can bootstrap them. In-place data conversion is separate work.

The adapter uses the SDK's explicit `SyncMode::Every` setting. The pinned local SDK builds its datastore options from explicit configuration. The SurrealKV engine publishes committed writes after its immediate disk sync succeeds. Automatic upgrades from older SurrealDB stores stay disabled. [Pinned SDK source](https://docs.rs/crate/surrealdb-engine-local/4.0.0/source/src/lib.rs) · [Embedded Rust guide](https://surrealdb.com/docs/reference/rust/embedding)

The embedded SurrealDB package and engine crates ship under the Business Source License 1.1. Their additional use grant and change terms are in the pinned package license. [SurrealDB 4.0.0 license](https://docs.rs/crate/surrealdb/4.0.0/source/LICENSE) · [Vendor license guide](https://surrealdb.com/license)

The pinned SDK splits local paths at a literal `?` and does not decode an escaped path. Tuck rejects that character for SurrealDB stores before opening the engine. This preserves the requested path and prevents a path from changing connection options.
