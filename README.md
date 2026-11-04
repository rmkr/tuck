# Tuck

Tuck shares selected memories between authenticated coding agents on one Mac. It runs locally and currently stores sourced memories through an embedded Turso adapter.

The memory module uses a database-independent storage interface. It owns access checks, validation, revisions, retry rules, and response formats. Storage adapters own persistence and atomic commits. An optional SurrealDB adapter is tracked in [#9](https://github.com/rmkr/tuck/issues/9).

Agents connect directly over MCP, the Model Context Protocol. Each agent receives its own credential and explicit scopes. An editor is optional.

## Build and test

Use Rust 1.98 and Python 3.9 or newer. On macOS, install the Xcode Command Line Tools if the native build tools are missing.

```sh
cargo build --locked --release
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked
cargo test --locked --features test-hooks
```

The last command runs deterministic crash tests. Ship a normal release build without `test-hooks`.

## Start a private store

The default store is `$HOME/.local/share/tuck`. Use an absolute `--home` path to keep separate stores.

```sh
target/release/tuck init
target/release/tuck scope add project:example
target/release/tuck grant add agent-a --scope project:example --read --write
target/release/tuck grant add agent-b --scope project:example --read --write
target/release/tuck serve
```

Each `grant add` command prints a new secret once. Keep it in the client's secret settings or an environment variable. Tuck stores only its hash. Keep secrets out of source control and shell history.

The default endpoint is `http://127.0.0.1:47831/mcp`. Each HTTP request must carry `Authorization: Bearer YOUR_TOKEN`. Use `serve --port 47832` to select another port. Tuck accepts loopback hostnames and rejects requests with a browser `Origin` header.

Use `Ctrl-C` or `SIGTERM` to stop Tuck. The operating system releases its store lock after a crash. Start the same command again. One process owns each store.

## Connect agents

Configure each agent with the endpoint and its own bearer token. The process test client in `tests/process.py` shows direct JSON-RPC requests. The server also supports normal MCP initialization and discovery through `rmcp`.

For Codex, keep the token in the environment that starts the client. Add this to its MCP configuration after substituting your chosen environment variable name.

```toml
[mcp_servers.tuck]
url = "http://127.0.0.1:47831/mcp"
bearer_token_env_var = "TUCK_AGENT_A_TOKEN"
```

For an optional Zed connection, add a remote context server. Zed's `headers` value contains the secret, so protect the settings file and keep it out of shared project settings.

```json
{
  "context_servers": {
    "tuck": {
      "url": "http://127.0.0.1:47831/mcp",
      "headers": {"Authorization": "Bearer YOUR_TOKEN"}
    }
  }
}
```

Client configuration details come from the official guides. Live acceptance uses independent authenticated agent sessions. [Codex MCP configuration](https://developers.openai.com/codex/mcp) · [Zed MCP configuration](https://zed.dev/docs/ai/mcp)

## Use the five tools

Save only when the user explicitly asks. Recall relevant memories before repeating work. Treat recalled content as sourced evidence under the host's instructions. The source origin describes the caller's attribution. Permissions come from the client grant.

Agent A can call `memory_save` with these arguments.

```json
{
  "scope": "project:example",
  "kind": "decision",
  "content": "Use Widget::parse for this project's parser.",
  "source": {
    "reference": "docs/parser.md#decision",
    "origin": "user_statement"
  },
  "operation_id": "a-save-001"
}
```

Keep the returned `id` and `revision`. Agent B can find the record with `memory_search`.

```json
{"query":"Widget::parse","scopes":["project:example"],"limit":5}
```

Read the full record with `memory_get`.

```json
{"id":"RETURNED_MEMORY_ID"}
```

Correct it with `memory_update`. Use the revision from the latest read.

```json
{
  "id": "RETURNED_MEMORY_ID",
  "expected_revision": 1,
  "content": "Use Widget::decode after the parser change.",
  "source": {"reference":"docs/parser.md#revision","origin":"project_artifact"},
  "operation_id": "b-update-001"
}
```

Forget the corrected record with `memory_delete`.

```json
{"id":"RETURNED_MEMORY_ID","expected_revision":2,"operation_id":"b-delete-001"}
```

Agent A can then confirm `memory_get` returns `not_found` and search omits the record. Repeat the flow after restarting Tuck to verify persistence in your clients.

A mutation retry must reuse the same operation ID and arguments. Tuck returns the previous ID, revision, and outcome. Different arguments with that ID return `conflict`. Operation IDs belong to one grant and one tool. After deletion, retries of the original save or update return `deleted` and cannot recreate the memory. A deliberate new save needs a new operation ID.

Corrections preserve the ID, scope, kind, and creation time. They advance the revision and record the authenticated writer. Timestamps are Unix milliseconds. Tuck keeps the current content. A stale revision returns `conflict` and requires a new read.

## Scopes and search

A scope is an explicit stable ID. A directory name does not grant access. List scopes and grants with `tuck scope list` and `tuck grant list`.

Shared user preferences require a separate scope and an explicit grant.

```sh
tuck scope add shared:user
tuck grant add agent-shared --scope project:example --scope shared:user --read --write
```

To change a grant's scopes or rights, revoke it and create a new grant. To keep its identity and change only the credential, rotate it.

```sh
tuck grant rotate agent-a
tuck grant revoke agent-b
```

Restart Tuck after each grant change. The running process keeps its startup grant snapshot. Rotation and revocation take effect on every request after restart. A recreated grant gets a new identity and separate retry receipts.

Search lowercases content and source references. It indexes whitespace-separated terms and alphanumeric parts, so code identifiers and their parts can match. Queries are literal data. Search ranks records by the number of distinct matching indexed terms, then by memory ID in ascending order. It filters allowed scopes before ranking and limiting.

Search returns five results by default and permits one to twenty. Each excerpt has at most 512 Unicode characters. Content permits 8192 UTF-8 bytes, source references 2048 bytes, and queries 1024 bytes. Kinds are `preference`, `decision`, `fact`, and `lesson`. Origins are `user_statement`, `project_artifact`, and `agent_inference`. IDs and operation IDs permit 1–128 ASCII letters, digits, or `-_:./` characters. Empty required text is invalid.

Tool failures use `isError` with a JSON error containing a stable `code` and a short `message`. Codes include `validation`, `forbidden`, `not_found`, `conflict`, and `storage_error`. Inaccessible, absent, and deleted IDs all return `not_found`. HTTP authentication failures return 401. Host or Origin rejection returns 403. Oversized HTTP bodies return 413.

## Backup and restore

Stop every process that owns or changes the store before copying it. Keep the whole directory together. The copy includes `memory.db`, any `memory.db-wal` or `memory.db-shm` sidecars, and `config.json`.

```sh
umask 077
cp -Rp "$HOME/.local/share/tuck" "$HOME/tuck-backup"
cp -Rp "$HOME/tuck-backup" "$HOME/tuck-restored"
tuck --home "$HOME/tuck-restored" serve --port 47832
```

Connect to port 47832 with a backed-up credential. Read a known memory and retry its saved operation ID. Confirm both before using the restored store. The process suite verifies this procedure after forced termination, including receipt and record recovery.

Keep backups private. They contain memories and credential hashes. A backup predating deletion still contains the old memory. Clients and operating-system snapshots have their own retention. Tuck deletion removes active records, their source, and search terms. It retains content-free retry receipts and request fingerprints. Old bytes can remain in unused disk pages.

## Troubleshooting and removal

A store or config file must belong to the current user. Its directory needs mode `700` and its files need mode `600`. Tuck rejects symlinks at the store or database paths. Fix the path or permissions before starting it.

If the port is busy, stop its other listener or select another port. If the store lock is busy, stop the other Tuck process. Do not remove a lock file while Tuck runs.

If startup rejects the schema, use a Tuck version that supports it. Restore a verified stopped-store backup into a separate directory if data is damaged. Keep the original for investigation. Tuck has schema version 1 and creates its tables transactionally. It rejects newer schemas.

If a write fails, check free disk space and file permissions, then restart. Retry the same operation ID and arguments to find whether the mutation committed. Ordinary logs contain operation, status, and timing. They omit credentials, memory content, source references, and queries.

To remove Tuck, stop the process, remove its MCP entries from clients, and delete the binary. Delete the chosen store directory and its backups only when you intend to erase those copies.

## Performance and acceptance

Run the benchmark against a normal optimized release binary.

```sh
python3 tests/benchmark.py target/release/tuck > benchmark.json
```

It seeds 1000 and 10000 memories through MCP. It uses one and four concurrent clients, reports median and p95 latency, and includes identifier and broad-keyword queries. Cold means the first request after process restart. It keeps the operating-system file cache. The proposed warm-search target is p95 at or below 100 ms over 10000 memories. Timing is evidence for the measured Mac, separate from correctness assertions.

The [Turso decision](docs/adr/0001-turso-and-transactional-keyword-index.md) records the original correctness gate. The [storage interface decision](docs/adr/0002-database-independent-storage.md) records how another engine can replace it. The [validation record](docs/validation.md) contains the process tests, headless two-agent workflow, benchmark, and synthetic memory-enabled comparison. The storage-interface release measured 10000-memory warm p95 at 21.42 ms with one client and 77.06 ms with four clients on this Mac. Day-to-day productivity remains unmeasured. The [approved specification](https://github.com/rmkr/tuck/issues/1) defines acceptance.
