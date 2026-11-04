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
