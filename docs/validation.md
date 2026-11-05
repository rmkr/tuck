# Tuck v1 validation

Checked on 2026-11-04 on an Apple M5 Pro running macOS 27.2. The build used Rust 1.98, Turso 0.8.3, and rmcp 3.8.0. `Cargo.lock` records the dependency graph.

The review-fix release binary has SHA-256 `69a42e4c17e697369851218f20226e3657958de8f7354cef04d7e912b3cf047c`. The earlier release `238ddb6a25595628ed744a784c9e84225d71eab479e32b2ab95f6c238bb6bbdf` supplied the repeated-detail trial below. The final benchmark and supplemental headless checks used the review-fix release.

## Process and storage checks

The public MCP suite uses the running executable and a separate on-disk database for each test. All fifteen tests passed with `test-hooks`. The normal release passed fourteen tests and skipped the one test that requires those hooks. Formatting and Clippy with warnings denied also passed.

The suite covers tool discovery, authenticated scope access, optional shared scope access, read-only grants, source metadata, ranked and bounded search, code punctuation, input limits, concurrent edits, retry receipts, deletion, and restart persistence. It also covers credential rotation and recreation, current grant checks on old receipts, delete/update races, lock contention, failed writes, malformed configuration, missing or corrupt databases, future schema rejection, and stopped-store restoration.

The crash test kills the process at four positions for each mutation. Those positions follow the record change, follow the index change, precede commit, and follow commit. After each restart, MCP reads and searches show the complete old or new state. A retry returns a consistent outcome. These twelve crash cases passed.

The file-size limit test forces a real operating-system write failure. The previously committed memory remains readable after restart. The backup test copies the whole stopped store after forced termination, restores it to another directory, and checks both the memory and its save receipt through MCP.

The two review regressions also passed. Save and search validate scope syntax before checking authorization. A valid ungranted scope still returns `forbidden`. Only `init` creates the schema. Normal startup rejects empty stores before opening the engine and checks initialized stores through a read-only connection. The empty-file and valid version-zero fixtures retain their exact original bytes and grant configuration after rejection.

Repeat these checks with the commands in the README. On this Mac, release debug-info stripping required the toolchain's library directory in `DYLD_LIBRARY_PATH`. The build used a per-command environment setting and left the installed toolchain unchanged.

## Headless agent workflow

Two independent, signed-in Codex CLI 0.166 agent sessions used separate credentials for the same project. Agent A saved and read a sourced decision. Agent B searched, read, corrected, and read the new revision. Agent A observed that revision, deleted the memory, retried its original save, and confirmed both old and new search terms returned no memory.

That live-agent run used binary SHA-256 `c42c1e33da29ebb1a9d72aefcfdab501c7f3f871e3c5200f913eedf49782ccf1`. The later `238ddb` build changed the HTTP request-body cap from 32 KiB to 128 KiB. The review-fix build adds scope validation and safe startup checks. A new public test verifies that maximum-size escaped content and source text round-trip through the final binary. It passed with the full review-fix process suite.

The local run records are `/private/tmp/tuck-live-v1/save-v2.jsonl`, `/private/tmp/tuck-live-v1/correct-v2.jsonl`, and `/private/tmp/tuck-live-v1/forget-v2.jsonl`. Editor integration is optional. This validation used direct agent connections.

## Retrieval speed

The benchmark used the `69a42e` review-fix binary, one process, a serialized connection, WAL, and `synchronous=FULL`. It seeded all records through MCP. The data contains short sourced decisions in one hundred groups. Four queries contain code identifiers and one uses the broad word `decision`. Each scenario measures one hundred warm requests after twenty warm-up requests.

Cold results measure the first wave of queries after each of five process restarts. The operating-system file cache stays in place. The one-client cold sample contains five requests. The four-client sample contains twenty.

| Memories | Clients | Warm median ms | Warm p95 ms | Cold median ms | Cold p95 ms |
| --- | --- | --- | --- | --- | --- |
| 1000 | 1 | 1.94 | 2.58 | 2.71 | 5.82 |
| 1000 | 4 | 5.64 | 6.49 | 5.25 | 7.34 |
| 10000 | 1 | 13.13 | 22.52 | 18.16 | 53.22 |
| 10000 | 4 | 58.74 | 63.91 | 42.17 | 67.28 |

Both 10000-memory scenarios met the proposed 100 ms warm p95 target on this Mac. An earlier query plan missed the target with four clients. It carried full JSON records through ranking. The final query ranks scoped IDs first, applies the limit, and then fetches the selected records. That run is recorded in `/private/tmp/tuck-forge/benchmark-fixed.json`. Run `python3 tests/benchmark.py target/release/tuck` to repeat it.

## Memory-enabled comparison

Four synthetic coding-context questions ran in separate headless Codex sessions with and without Tuck. The expected answers stayed outside the memory store. Two questions had a current, authorized answer. One concerned a deleted fact and one concerned an unauthorized project.

| Observation | Memory off | Memory on |
| --- | --- | --- |
| Correct sourced answers to answerable questions | 0 of 2 | 2 of 2 |
| Correct abstentions on deleted or unauthorized information | 2 of 2 | 2 of 2 |

The memory-enabled session recalled `Widget::decode` with its current fixture source and `Zed` with its shared-preference source. Both sessions abstained on the deleted formatting decision and the other project's release information. The run used the same `c42c` binary as the live workflow. Its local result is `/private/tmp/tuck-forge/benefit.json`.

This small trial demonstrates sourced retrieval and correct abstention for these fixtures. Day-to-day productivity and task completion time remain unmeasured.

## Supplemental headless acceptance

Eighteen checks passed on the `69a42e` review-fix release through two independently authenticated HTTP MCP clients and an opt-in read-only shared client. They covered initialization, explicit project scopes, separate-project access, valid unauthorized scopes, opt-in sharing, corrections across restart, deletion across forced restart, and retries that cannot restore deleted memories. The local record is `/private/tmp/tuck-forge/headless-acceptance.json`.

## Repeated-detail trial

A matched synthetic follow-up task ran in two independent headless Codex sessions on release `238ddb`. The previously agreed command lived only in Tuck. Both sessions received the same prompt and could ask the user for the missing detail.

| Observation | Memory off | Memory on |
| --- | --- | --- |
| Requests to repeat the known command | 1 | 0 |
| Exact stored command recovered | No | Yes |

The memory-enabled session recovered `cargo test --locked --features test-hooks` and supplied its source. The memory-disabled session asked the user for the command. The local record is `/private/tmp/tuck-forge/repetition.json`.

This measures one request to repeat one stored detail in a synthetic task. Day-to-day productivity and task completion time remain unmeasured.

## Database-independent storage follow-up

The storage-interface refactor uses the same Turso release, disk schema, and public MCP contract. Its release binary has SHA-256 `a594fce72b9eed054934e14b66f2ebc1f91129bb9b200e805e0539e7ac020b12`. Formatting, strict Clippy, and all fifteen process tests passed with `test-hooks`. The normal release passed fourteen tests and skipped the hook-only case. The eighteen supplemental headless checks passed with the new release.

Eleven compatibility checks passed across the earlier `69a42e` release and this release. The new release read old records and receipts, searched their terms, preserved deletion, and corrected an old record. The earlier release then read that correction and replayed its receipt. Both releases used the same disposable store without a format conversion.

The memory, storage, and operator modules also compiled in a temporary crate with a separate storage implementation and no Turso dependency. This was a compile-only check. Runtime validation used the real Turso adapter.

The same benchmark was repeated through MCP with the new release. These measurements use the dataset, warm-up, sample counts, and cold definition described above.

| Memories | Clients | Warm median ms | Warm p95 ms | Cold median ms | Cold p95 ms |
| --- | --- | --- | --- | --- | --- |
| 1000 | 1 | 1.04 | 1.51 | 2.55 | 3.13 |
| 1000 | 4 | 3.67 | 3.98 | 4.14 | 5.92 |
| 10000 | 1 | 12.74 | 21.42 | 16.31 | 24.11 |
| 10000 | 4 | 61.37 | 77.06 | 34.59 | 64.50 |

The 10000-memory warm measurements remain within the 100 ms target. Run evidence is in `/private/tmp/tuck-storage-review/benchmark.json`, `legacy-store.json`, and `headless-acceptance.json`. The optional SurrealDB adapter is tracked in [#9](https://github.com/rmkr/tuck/issues/9). It has no runtime validation yet.

## Optional SurrealDB backend

The optional backend uses SurrealDB 4.0.0 with SurrealKV 1.2.0 and explicit `SyncMode::Every`. The initial SurrealDB-option release has SHA-256 `9e0457d60082d1e92fecafb1200aa7d440d68e3d0be0508f4dafc5b39e176122`. Both adapters use the same memory module and public MCP contract.

All 50 feature-enabled process tests passed, with 25 cases for each backend and no skips. The normal release passed 21 cases per backend and skipped four cases per backend that require the compile-time test hooks. Formatting and strict Clippy passed. The tests cover initialization, recorded backend selection, legacy Turso configuration, ranked search, permissions, concurrent changes, retry receipts, deletion, restarts, forced termination, failed writes, and backup and restore.

The independent direct-client checks passed 18 assertions for each backend. Eleven additional checks opened a store from the earlier Turso release, read its records and receipts, corrected a record with the new release, and reopened it with the earlier release. The store needed no format conversion.

The first independent review found a startup recovery defect. SurrealKV sets its WAL directory to `0750` during opening. A process killed before Tuck restored `0700` could leave an intact store unable to restart. A separate verifier reproduced that exact process-kill window. The fix recognizes only that owned directory state under a private, valid store, checks its contents, and restores `0700`. Public files, symlinks, wrong owners, and other directory modes remain rejected. A deterministic startup-kill regression now passes.

The first benchmark also exposed a multi-term query plan that scanned the scope before filtering terms. A bounded query change now uses exact composite-index lookups for up to 16 scope/term pairs. Larger requests use the original scoped query. Public tests verify both paths, duplicate scopes, and a 1024-byte query with 128 scopes.

The evidence is in `/private/tmp/tuck-surrealdb-review/`. Process termination was tested. Physical power-loss recovery was not tested.

The benchmark ran after the build and correctness checks finished on binary `58d0d1b58b91ca954add3a42c15529f0c11469b968f41024409ba4f8d90043d6`. The later final binary adds a startup file-type check and its regression test. Its search and transaction code are identical to the measured binary. It used the same datasets, 100 warm requests after 20 warm-up requests, and five cold restart trials per scenario. Each backend used the same measured normal release binary.

| Backend | Memories | Clients | Warm median ms | Warm p95 ms | Cold median ms | Cold p95 ms |
| --- | --- | --- | --- | --- | --- | --- |
| turso | 1000 | 1 | 1.08 | 1.49 | 2.22 | 2.75 |
| turso | 1000 | 4 | 3.57 | 3.74 | 3.74 | 5.19 |
| turso | 10000 | 1 | 8.28 | 13.82 | 11.53 | 19.37 |
| turso | 10000 | 4 | 37.55 | 40.45 | 27.60 | 42.45 |
| surrealdb | 1000 | 1 | 4.13 | 6.16 | 5.91 | 7.38 |
| surrealdb | 1000 | 4 | 16.63 | 17.75 | 13.91 | 20.35 |
| surrealdb | 10000 | 1 | 26.89 | 46.37 | 30.16 | 49.26 |
| surrealdb | 10000 | 4 | 136.84 | 165.15 | 77.49 | 125.86 |

Turso met the proposed 100 ms warm p95 target with one and four clients at 10000 memories. SurrealDB met it with one client. Its four-client result was 165.15 ms, above that target. SurrealDB is an optional backend, and Turso remains the default. These measurements describe this Mac and workload. Large scope/term requests use the fallback query and may take longer.

The final results are in `/private/tmp/tuck-surrealdb-review/benchmark-final-turso.json` and `benchmark-final-surrealdb.json`. The first timing run overlapped compiler work and was used only to find the query-plan issue.

A later review found that a private named pipe at a format-marker path could block startup before the recursive file checks. An independent verifier reproduced both affected paths. The final fix checks file types before reading them. The named-pipe regressions pass on both engines, including the normal release.

## Concurrent SurrealDB reads

The performance follow-up release has SHA-256 `e66d07a842e1ba071eabc2215332264243233b428dba7fa0e21edad7afc571e9`. The user requested SurrealDB warm-search p95 below 100 ms for 10000 memories and four clients.

A persistent disposable store used the same five queries, 20 warm-up requests, and 100 measured requests as the standard benchmark. The unchanged baseline `144d153` measured 127.41 and 133.96 ms p95 in two runs. A timing probe measured lock-wait p95 at 108.20 ms and search-service p95 at 50.23 ms. The main delay came from serializing independent reads through one store mutex.

Read and search operations now share a read lock for the complete operation. Saves, corrections, and deletions hold an exclusive lock through their reads, receipt checks, and durable commit. The Turso adapter serializes its connection reads internally. The new concurrent-reader regression exposed storage errors without that adapter guard. It passes with the guard and checks that scores, excerpts, and revisions agree during updates and deletion.

All 52 optimized, hook-enabled process tests passed, with 26 cases per backend. The normal release passed 22 cases and skipped four hook-only cases per backend. Formatting, strict Clippy, and exact result comparisons for all five benchmark queries passed.

Three independent four-client repeat runs on the freshly rebuilt release measured 58.40, 58.46, and 59.30 ms warm p95. The standard benchmark then seeded a fresh 10000-memory store through MCP and produced these results.

| Memories | Clients | Warm median ms | Warm p95 ms | Cold median ms | Cold p95 ms |
| --- | --- | --- | --- | --- | --- |
| 10000 | 1 | 28.75 | 50.12 | 32.68 | 51.35 |
| 10000 | 4 | 33.45 | 57.96 | 41.27 | 60.51 |

The four-client warm result meets the 100 ms target on this Mac with the stated query mix. Durability remains `SyncMode::Every`. The reports are `/private/tmp/tuck-surreal-performance/root-four-1.json`, `root-four-2.json`, `root-four-3.json`, and `canonical-surrealdb.json`.

At this stage, the debug-profile disk-limit fixture raised `ConnectionResetError`. The unchanged baseline reproduced the same failure in three fresh runs. Optimized hook-enabled tests and normal-release tests passed this case. The later debug recovery follow-up below resolves this failure.

## Debug recovery follow-up

The rebuilt `f4034179` baseline reproduced the disk-limit failure. The fixture also limits writes to its log file. Sending diagnostics to `/dev/null` under the same file-size limit returned the expected `storage_error`. A new public MCP regression reproduced the connection reset with a closed stderr pipe and unrestricted database writes.

Server diagnostics now tolerate failed stderr writes. The regression verifies a successful save, a read, a conflict response, and a normal restart followed by a read and replay of the original save receipt. The existing disk-limit test also verifies that the prior committed memory survives restart.

The full debug run exposed a second baseline failure. A SurrealDB update exhausted a Tokio worker's stack and aborted the process. The same baseline update passed with a 10 MiB worker stack. Tuck now builds its Tokio runtime with that stack size, following the pinned SurrealDB 4.0.0 embedded-runtime example. Final checks ran with `RUST_MIN_STACK` unset.

Both debug and release profiles passed all 54 hook-enabled process tests, with 27 tests per backend. Each normal profile passed 46 tests and skipped the eight hook-only cases. Formatting and strict Clippy also passed. The normal release binary has SHA-256 `1d821ae22f977a2e0330de1b171e036893cd664dc903321d94947764e7089087`.

The checks used Rust 1.98.0 on the same Mac. The build used `CARGO_HOME=/private/tmp/tuck-cargo`, `CARGO_TARGET_DIR=/private/tmp/tuck-surreal-target`, and the toolchain's library directory in `DYLD_LIBRARY_PATH`. Run the same checks with these commands.

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked
cargo test --locked --features test-hooks
cargo test --release --locked
cargo test --release --locked --features test-hooks
```

The logs are in `/private/tmp/tuck-debug-recovery/`. Debug linking reported the existing large unwind-section warning. The debug builds and process tests completed. This follow-up did not repeat the live-agent trials, benchmarks, or physical power-loss checks.
