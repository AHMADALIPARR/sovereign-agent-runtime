#!/bin/sh
# Sovereign Agent Tool Runtime — Phase 6 test suite.
# Every check runs the real server/backends. The driver and the
# assertion helpers are harness code; the product contains no
# Python and no JavaScript.
set -u
cd "$(dirname "$0")/.."
ROOT=$(pwd)
export RUSTUP_HOME=$HOME/workspace/.tools/rustup CARGO_HOME=$HOME/workspace/.tools/cargo PATH=$HOME/workspace/.tools/cargo/bin:$PATH
export SAT_REGISTRY=${SAT_REGISTRY:-$HOME/workspace/tool-discovery-engine/registry}
export SAT_JULIA=${SAT_JULIA:-$HOME/workspace/.tools/julia-1.11/bin/julia}
export SAT_GODOT=${SAT_GODOT:-$HOME/workspace/.tools/godot-4.3/Godot_v4.3-stable_linux.x86_64}
export SAT_CRYPTO=$ROOT/cryptool
BIN=$ROOT/target/release/sat-runtime
W=$(mktemp -d /tmp/sat-test.XXXXXX)
export SAT_TRACE=$W/trace.log
pass=0; fail=0
ok() { if [ "$1" = 0 ]; then echo "  PASS  $2"; pass=$((pass+1)); else echo "  FAIL  $2"; fail=$((fail+1)); fi }
cleanup() { rm -rf "$W"; }
trap cleanup EXIT

cargo build --release > /dev/null 2>&1
ok $? "runtime builds"
gcc -O2 -o cryptool backends/crypto.c -lcrypto 2> /dev/null
ok $? "crypto backend builds (OpenSSL)"

drive() { python3 test/drive.py "$BIN" "$1"; }
INIT='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}'

# ---------- MCP discovery + registry ----------
printf '%s\n%s\n%s\n%s\n' "$INIT" \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"registry_verify","arguments":{}}}' \
  '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"registry_status","arguments":{}}}' \
  > "$W/s1.jsonl"
drive "$W/s1.jsonl" > "$W/o1.json"
grep -q '"valid":true' "$W/o1.json"; ok $? "registry seal verifies through the server"
grep -q '"records":922' "$W/o1.json"; ok $? "registry holds 922 deduplicated records"

# ---------- schema validation + invalid inputs ----------
printf '%s\n%s\n%s\n%s\n' "$INIT" \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"compute_execute","arguments":{"operation":"matrix.multiply","args":{"a":[[1,2]]}}}}' \
  '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"compute_execute","arguments":{"operation":"crypto.decrypt","args":{"key_hex":"zz"}}}}' \
  '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"compute_execute","arguments":{"operation":"nope.nope","args":{}}}}' \
  > "$W/s2.jsonl"
drive "$W/s2.jsonl" > "$W/o2.json"
[ "$(grep -c '"execution_status":"invalid"' "$W/o2.json")" = 2 ]; ok $? "schema validation blocks bad calls before execution (2 invalid)"
grep -q '"execution_status":"error"' "$W/o2.json"; ok $? "unsupported operation returns an explicit error"

# ---------- Julia numerical correctness ----------
printf '%s\n%s\n%s\n%s\n%s\n%s\n%s\n' "$INIT" \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"compute_execute","arguments":{"operation":"matrix.multiply","args":{"a":[[1,2],[3,4]],"b":[[5,6],[7,8]]}}}}' \
  '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"compute_execute","arguments":{"operation":"matrix.determinant","args":{"a":[[1,2],[3,4]]}}}}' \
  '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"compute_execute","arguments":{"operation":"matrix.eigenvalues","args":{"a":[[2,1],[1,2]]}}}}' \
  '{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"compute_execute","arguments":{"operation":"tensor.contract","args":{"a":[[1,2],[3,4]],"b":[[5,6],[7,8]],"dims_a":"ij","dims_b":"jk"}}}}' \
  '{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"compute_execute","arguments":{"operation":"matrix.solve","args":{"a":[[3,1],[1,2]],"b":[9,8]}}}}' \
  '{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"compute_execute","arguments":{"operation":"array.reshape","args":{"data":[1,2,3,4,5,6],"shape":[2,3]}}}}' \
  > "$W/s3.jsonl"
drive "$W/s3.jsonl" > "$W/o3.json"
grep -q '"result":\[\[19,22\],\[43,50\]\]' "$W/o3.json"; ok $? "matrix.multiply [[19,22],[43,50]]"
grep -q '"result":-2' "$W/o3.json"; ok $? "determinant -2"
grep -q '"result":\[1,3\]' "$W/o3.json"; ok $? "eigenvalues [1,3]"
[ "$(grep -c 'self-checked' "$W/o3.json")" -ge 1 ]; ok $? "solve carries a residual self-check"
grep -q '"result":\[\[1,3,5\],\[2,4,6\]\]' "$W/o3.json"; ok $? "reshape [2,3] (Julia column-major)"

# ---------- session isolation ----------
printf '%s\n%s\n%s\n%s\n' "$INIT" \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"compute_execute","arguments":{"operation":"array.create","args":{"data":[1,1],"store_as":"v"},"session":"alpha"}}}' \
  '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"compute_inspect","arguments":{"session":"beta"}}}' \
  '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"compute_inspect","arguments":{"session":"alpha"}}}' \
  > "$W/s4.jsonl"
drive "$W/s4.jsonl" > "$W/o4.json"
python3 - "$W/o4.json" << 'PY'
import json, sys
rows = [json.loads(l) for l in open(sys.argv[1])]
beta, alpha = rows[2], rows[3]
assert beta["variables"] == [], "beta session leaked"
assert any(v["name"] == "v" for v in alpha["variables"]), "alpha lost its variable"
print("isolation ok")
PY
ok $? "sessions are isolated (alpha holds v, beta is empty)"

# ---------- GDScript scheduler through the server ----------
printf '%s\n%s\n%s\n' "$INIT" \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"graph_execute","arguments":{"nodes":[{"id":"m1","op":"matrix.multiply","args":{"a":[[1,2],[3,4]],"b":[[5,6],[7,8]]}},{"id":"det","op":"matrix.determinant","args":{"a":{"$node":"m1"}}}],"edges":[["m1","det"]]}}}' \
  '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"compute_execute","arguments":{"operation":"graph.detect_cycle","args":{"nodes":["a","b"],"edges":[["a","b"],["b","a"]]}}}}' \
  > "$W/s5.jsonl"
drive "$W/s5.jsonl" > "$W/o5.json"
grep -q '"order":\["m1","det"\]' "$W/o5.json"; ok $? "graph executes in scheduler order"
python3 - "$W/o5.json" << 'PY'
import json, sys
rows = [json.loads(l) for l in open(sys.argv[1])]
g = rows[1]
det = g["result"]["nodes"]["det"]["result"]
assert abs(det - 4) < 1e-9, det
print("det ok")
PY
ok $? "dependent node consumed the upstream result (det = 4 within 1e-9)"
grep -q '"has_cycle":true' "$W/o5.json"; ok $? "cycle detected and reported"

# ---------- crypto through the server ----------
KEY=abababababababababababababababababababababababababababababababab
printf '%s\n%s\n%s\n%s\n' "$INIT" \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"compute_execute","arguments":{"operation":"crypto.hash","args":{"algorithm":"sha256","data":"abc"}}}}' \
  '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"compute_execute","arguments":{"operation":"crypto.hmac","args":{"algorithm":"sha256","key_hex":"0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b","data":"Hi There"}}}}' \
  "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"tools/call\",\"params\":{\"name\":\"compute_execute\",\"arguments\":{\"operation\":\"crypto.encrypt\",\"args\":{\"key_hex\":\"$KEY\",\"plaintext\":\"sovereign\"}}}}" \
  > "$W/s6.jsonl"
drive "$W/s6.jsonl" > "$W/o6.json"
grep -q 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad' "$W/o6.json"; ok $? "SHA-256 FIPS vector through the server"
grep -q 'b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7' "$W/o6.json"; ok $? "HMAC RFC 4231 vector through the server"
grep -q '"ciphertext_hex"' "$W/o6.json"; ok $? "AES-256-GCM encrypts through the server"
grep -q 'library-vector-checked' "$W/o6.json"; ok $? "crypto verification status reports the startup vector check"

# ---------- routing: tool_execute gates ----------
printf '%s\n%s\n%s\n%s\n' "$INIT" \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"tool_execute","arguments":{"provider":"Playwright MCP","name":"browser_navigate","arguments":{}}}}' \
  '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"tool_execute","arguments":{"provider":"Ghost","name":"not_a_tool","arguments":{}}}}' \
  '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"tool_execute","arguments":{"provider":"runtime","name":"matrix.determinant","arguments":{"a":[[1,2],[3,4]]}}}}' \
  > "$W/s7.jsonl"
drive "$W/s7.jsonl" > "$W/o7.json"
grep -q '"execution_status":"denied"' "$W/o7.json"; ok $? "external tool denied by policy"
grep -q 'unregistered capabilities do not execute' "$W/o7.json"; ok $? "unregistered tool refused"
grep -q '"result":-2' "$W/o7.json"; ok $? "runtime tools route through tool_execute"

# ---------- unauthorized: policy denies compute ----------
printf '{"classes":{"compute":"deny","crypto":"allow","graph":"allow","external_service":"deny"}}' > "$W/deny.json"
printf '%s\n%s\n' "$INIT" \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"compute_execute","arguments":{"operation":"matrix.determinant","args":{"a":[[1,2],[3,4]]}}}}' \
  > "$W/s8.jsonl"
SAT_POLICY="$W/deny.json" python3 test/drive.py "$BIN" "$W/s8.jsonl" > "$W/o8.json"
grep -q '"execution_status":"denied"' "$W/o8.json"; ok $? "policy can deny the whole compute class"

# ---------- timeout: a 2s budget against a cold Julia ----------
printf '%s\n%s\n' "$INIT" \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"compute_execute","arguments":{"operation":"matrix.determinant","args":{"a":[[1,2],[3,4]]}}}}' \
  > "$W/s9.jsonl"
SAT_TIMEOUT_SECS=2 python3 test/drive.py "$BIN" "$W/s9.jsonl" > "$W/o9.json"
grep -q 'timed out' "$W/o9.json"; ok $? "backend timeout surfaces as a structured error"

# ---------- dynamic registry change + refresh ----------
mkdir -p "$W/regcopy"
cp "$SAT_REGISTRY/tools.jsonl" "$SAT_REGISTRY/SEAL.txt" "$W/regcopy/"
python3 - "$BIN" "$W/regcopy" "$W/s10.jsonl" << 'PY'
import hashlib, json, os, subprocess, sys
server, regdir, scenario = sys.argv[1], sys.argv[2], sys.argv[3]
env = dict(os.environ, SAT_REGISTRY=regdir)
proc = subprocess.Popen([server], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                        stderr=subprocess.DEVNULL, env=env, text=True)
def call(line):
    proc.stdin.write(line + "\n"); proc.stdin.flush()
    return json.loads(proc.stdout.readline())
call('{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}')
refresh = '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"registry_status","arguments":{"refresh":true}}}'
first = json.loads(call(refresh)["result"]["content"][0]["text"])
assert first["records"] == 922, first
# change the registry on disk and reseal (test-harness operation)
lines = [l for l in open(regdir + "/tools.jsonl").read().rstrip("\n").split("\n")
         if '"browser_navigate"' not in l]
genesis = [l for l in open(regdir + "/SEAL.txt") if l.startswith("genesis: ")][0].split(" ", 1)[1].strip()
tip = genesis
for l in lines:
    tip = hashlib.sha256((tip + "|" + l).encode()).hexdigest()
open(regdir + "/tools.jsonl", "w").write("\n".join(lines) + "\n")
open(regdir + "/SEAL.txt", "w").write(f"genesis: {genesis}\nrecords: {len(lines)}\nhead: {tip}\n")
second = json.loads(call(refresh)["result"]["content"][0]["text"])
assert "reloaded" in second["note"] and second["records"] == 921, second
proc.stdin.close(); proc.wait(timeout=30)
print("refresh ok")
PY
ok $? "registry refresh picks up a resealed change (live server)"

# ---------- concurrency: two clients at once ----------
printf '%s\n%s\n' "$INIT" \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"compute_execute","arguments":{"operation":"matrix.determinant","args":{"a":[[1,2],[3,4]]}}}}' \
  > "$W/s11.jsonl"
drive "$W/s11.jsonl" > "$W/o11a.json" &
drive "$W/s11.jsonl" > "$W/o11b.json" &
wait
grep -q '"result":-2' "$W/o11a.json" && grep -q '"result":-2' "$W/o11b.json"; ok $? "parallel clients both compute correctly"

# ---------- Agda ----------
(cd agda && LANG=C.UTF-8 agda --safe Runtime.agda > /dev/null 2>&1 && LANG=C.UTF-8 agda --safe Topo.agda > /dev/null 2>&1)
ok $? "Agda invariants typecheck (--safe, no postulates)"

echo "tests: $pass passed, $fail failed"
[ "$fail" = 0 ]
