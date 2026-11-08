# STATUS

## What this is

The Sovereign Agent Tool Runtime: an MCP server (Rust, stdio
JSON-RPC) exposing eight stable tools over the sealed
922-record tool registry, with typed compute backends — Julia
for numerics, GDScript/Godot for graph scheduling, native
C/OpenSSL for cryptography — and Agda-checked invariants.
No Python, no JavaScript, no IPython anywhere in the product.

## Coverage of the SOVEREIGN AGENT TOOL RUNTIME brief

- Phase 1 (discovery): the supplied inventories were ingested
  by the Tool Discovery Engine into a sealed registry; this
  repo's data/ holds the brief's four products generated from
  it (922-row CSV + JSONL, 14 schema files for the 14
  schema-captured tools, 128 incomplete-documentation records).
  Fields the sources do not carry (server_version,
  output_schema, permissions, side_effects, transport) are
  present as columns and empty — never invented. Classes:
  source-verified T1/T2 (50), documented T3 (6), name-only /
  historical T4 (128), representative T5 (738). No live
  external tools/list was run against third-party endpoints;
  live discovery is demonstrated against this runtime itself.
- Phase 2 (MCP engine): initialize + capability negotiation,
  tools/list with cursor pagination, tools/call, over stdio.
  The brief's operation list maps: registry.search →
  registry_search; registry.list → registry_status (+ the
  MCP tools/list); registry.describe → registry_describe;
  registry.refresh → registry_status {refresh:true} (reloads
  on seal change, suite-tested against a live server);
  registry.validate → registry_verify; registry.export → the
  data/ products; tool.search/tool.describe → registry_search
  / registry_describe; tool.execute → tool_execute;
  tool.batch → successive calls (results are independent and
  trace-linked); tool.trace → registry_status {trace_id};
  compute.execute/inspect/verify → compute_execute,
  compute_inspect, and the per-call verification_status field.
  HTTP transport is not implemented (stdio only); remote tools
  are never executed for discovery.
- Phase 3 (router): eight tools maximum, exposed. Pipeline per
  call: candidates → schema validation → permission policy →
  backend → typed envelope + trace. Retrieval is deterministic
  keyword scoring (name 3 / category 2 / provider 1, tier
  tiebreak). No embedding backend (optional in the brief, not
  built); similarity never authorizes anything — the policy
  gate is separate and default-denies external_service,
  file_write, network, credentials, destructive.
- Phase 4 (compute): all 23 operations implemented and
  executed — array (5), matrix (5), tensor (3), crypto (6),
  graph (4). Sessions persist in the Julia worker and are
  isolated by name. Timeouts kill and respawn the worker.
  Results carry the brief's envelope fields. Julia arrays are
  column-major (native semantics; reshape follows Julia).
- Phase 5 (Agda): Runtime.agda and Topo.agda typecheck under
  `agda --safe` (postulates are rejected by --safe itself).
  Proven: 1 (topo-exists), 2 (node-registered), 3 (Before in
  topo-exists), 4 (Execution/policy-ok), 5 (schema-ok),
  7 (insert-fresh), 8 (failure-yields-nothing). Property 6
  (type contracts) is enforced by construction in the
  envelope — output_type is measured from the actual result
  by the backend, never asserted separately — and is not a
  separate Agda theorem; stated here plainly.
- Phase 6 (tests): test/run-tests.sh, 27/27 — discovery,
  schema parsing, dedupe (922), dynamic registry change,
  routing gates, Julia correctness vs known answers, GDScript
  scheduling, crypto vectors (FIPS, RFC 4231, GCM tamper
  refusal, Ed25519 cross-checked against libsodium), invalid
  inputs, unauthorized execution, parallel clients, timeout.
- Phase 7 (adapter): docs/ADAPTER.md + examples/mcp-config.json
  + sat-client. The restricted-deployment section states the
  enforcement boundary honestly: absence from the registry is
  not a host-level block; the runtime is the substitute that
  makes the block reasonable, and it refuses unregistered
  capabilities by construction.

## Routing misses (measured, kept honest)

On the 12-task labeled set: "run a sql query on postgres"
ranked query_sql first (the generic name `query` was the
expectation); "send a slack message" ranked send_message
variants first (slack_post_message not in top 3). Keyword
retrieval's known limits, recorded rather than tuned away.

## Limits

- stdio transport only; one request at a time per connection
  (parallelism is across client processes, suite-tested).
- External registered tools do not execute: no live MCP
  servers are configured, and the policy denies the class by
  default. Discovery is not execution.
- The Julia worker is one process; sessions are namespaces
  inside it. A timeout kills the worker and all sessions.
- tensor.contract handles label contractions via permute +
  matrix multiply; >2 operands contract pairwise by chaining.
