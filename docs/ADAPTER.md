# Agent adapter

Any MCP-compatible agent connects over stdio with the
configuration in `examples/mcp-config.json` (paths adjusted).
The agent sees exactly eight tools — registry_search,
registry_describe, registry_status, registry_verify,
compute_execute, compute_inspect, graph_execute, tool_execute —
and never the 922 record definitions behind them. Measured on
this build: the eight tool definitions total ~2.9 KB of JSON;
preloading every registered record would cost ~377 KB.

A minimal client that speaks the same protocol ships in the
repo: `sat-client <server-bin> <scenario.jsonl>` plays a file of
JSON-RPC requests and prints the transcript. Anything that can
write JSON-RPC lines can drive the runtime the same way.

## Requesting computation without IPython

An agent that needs a determinant does not open a Python
session. It calls:

    compute_execute
      operation: "matrix.determinant"
      args:      {"a": [[1, 2], [3, 4]]}
      session:   "analysis"

and receives a typed envelope with the result, the backend that
ran it, a verification status, and a trace id it can look up
later through registry_status. Sessions persist across calls,
so multi-step work accumulates state in the Julia worker, not
in a notebook.

## Restricted deployment: keeping Python out

Honesty first, because the brief demands it: a tool's absence
from this registry does not by itself block Python on a host.
What this runtime provides is the *substitute* — every
computation an agent legitimately needs is reachable through
compute_execute, so a restricted deployment has no functional
reason to offer a Python executor at all.

A restricted deployment therefore:

1. Registers no Python/IPython/shell executor with the agent.
   The runtime's own surface has none, and its tool_execute
   refuses unregistered capabilities by construction.
2. Runs the agent under an OS account whose PATH contains no
   python, node, or general shell tools the agent could reach
   through any OTHER registered surface — the runtime cannot
   police tools it does not mediate.
3. Keeps policy.json's non-compute classes at "deny", so even
   registered external tools cannot execute through this
   runtime without a deliberate policy change.

The enforcement boundary is stated plainly: this runtime
controls what executes THROUGH it. Host-level lockdown (1–2)
is the operator's, and the combination is the policy.
