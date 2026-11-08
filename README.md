# Sovereign Agent Tool Runtime

An agent execution environment that replaces IPython: tools
are discovered once, sealed into a registry, and reached
through eight stable MCP tools — while computation runs on
Julia (numerics), GDScript (graph scheduling), and native
C/OpenSSL (cryptography). No Python, no JavaScript.

    AGENT -> 8 tools -> registry candidates -> schema
    validation -> permission policy -> backend -> typed
    result + trace

## Run it

    cargo build --release
    gcc -O2 -o cryptool backends/crypto.c -lcrypto
    SAT_REGISTRY=../tool-discovery-engine/registry \
    SAT_JULIA=julia SAT_GODOT=godot SAT_CRYPTO=./cryptool \
      ./target/release/sat-runtime        # MCP over stdio

Drive it with any MCP client, or the bundled one:

    ./target/release/sat-client ./target/release/sat-runtime scenario.jsonl

## The eight tools

registry_search, registry_describe, registry_status,
registry_verify, compute_execute, compute_inspect,
graph_execute, tool_execute. Everything else — 922 registered
tools across 93 providers — stays behind them, out of the
agent's context (≈95K tokens not loaded, measured).

## Verify

    test/run-tests.sh     # 27 checks, all backends live

Formal invariants: agda/ (typechecks under --safe).
Coverage and limits: STATUS.md. Numbers: METRICS.md.
