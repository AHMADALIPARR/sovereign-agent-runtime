# Metrics — measured on this machine, 2026-11-07

| Metric | Value | How measured |
|---|---|---|
| discovered_servers | 93 providers | registry records (tool-discovery-engine) |
| verified_tools | 50 (14 T1 + 36 T2) | evidence tiers in the sealed registry |
| live_callable_tools | 23 compute/graph/crypto operations + 8 runtime tools | op table in src/server.rs; every op executed in the test suite or scenarios |
| duplicate_tools | 137 | 1,059 source rows merged into 922 records at ingest |
| invalid_schemas | 0 | every registry line parses; invalid CALLS are refused at the gate (suite-verified) |
| failed_discoveries | 128 | records whose schemas were not independently captured (data/discovery_failures.jsonl) |
| routing_accuracy | top-1 8/12, top-3 10/12 | labeled 12-task set against registry_search (see STATUS for the misses) |
| execution_success_rate | 27/27 checks | test/run-tests.sh |
| tool_selection_latency | 1.35 ms | mean registry_search round trip over stdio, 5 calls |
| registry_memory_usage | 385,519 bytes on disk; loaded once at server start | tools.jsonl size |
| context_tokens_saved | ≈ 95,500 tokens | registry definitions 385,519 B vs the 8-tool surface 3,450 B, at ~4 B/token |

No figure here is estimated except the token conversion, which
is labelled approximate and derived from the two byte counts.
