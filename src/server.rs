// server.rs — the Sovereign Agent Tool Runtime: an MCP server
// (stdio, JSON-RPC) exposing eight stable tools over a sealed
// tool registry, with typed compute backends (Julia, GDScript,
// native C/OpenSSL). No Python, no JavaScript, no IPython.
//
//   AGENT -> [8 tools] -> intent -> registry candidates ->
//   schema validation -> permission check -> backend ->
//   typed result + trace
//
// Configuration (environment):
//   SAT_REGISTRY   dir holding the discovery registry
//                  (tools.jsonl + SEAL.txt from tool-discovery-engine)
//   SAT_JULIA      julia binary        SAT_GODOT   godot binary
//   SAT_CRYPTO     crypto helper bin   SAT_POLICY  policy json path
//   SAT_TRACE      trace log path      SAT_TIMEOUT_SECS (default 30)

#[path = "json.rs"]
mod json;
use json::{obj, s, n, Value};

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Instant;

// ============================ SHA-256 ============================
const K256: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];
fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let bitlen = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 { msg.push(0); }
    msg.extend_from_slice(&bitlen.to_be_bytes());
    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 { w[i] = u32::from_be_bytes([chunk[i * 4], chunk[i * 4 + 1], chunk[i * 4 + 2], chunk[i * 4 + 3]]); }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) = (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K256[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let mj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(mj);
            hh = g; g = f; f = e; e = d.wrapping_add(t1); d = c; c = b; b = a; a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a); h[1] = h[1].wrapping_add(b); h[2] = h[2].wrapping_add(c); h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e); h[5] = h[5].wrapping_add(f); h[6] = h[6].wrapping_add(g); h[7] = h[7].wrapping_add(hh);
    }
    let mut out = [0u8; 32];
    for i in 0..8 { out[i * 4..i * 4 + 4].copy_from_slice(&h[i].to_be_bytes()); }
    out
}
fn sha256_hex(text: &str) -> String {
    sha256(text.as_bytes()).iter().map(|b| format!("{:02x}", b)).collect()
}

// ============================ config ============================
struct Config {
    registry_dir: String,
    julia: String,
    godot: String,
    crypto: String,
    policy_path: String,
    trace_path: String,
    timeout_secs: u64,
}
impl Config {
    fn from_env() -> Config {
        let g = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
        Config {
            registry_dir: g("SAT_REGISTRY", "../tool-discovery-engine/registry"),
            julia: g("SAT_JULIA", "julia"),
            godot: g("SAT_GODOT", "godot"),
            crypto: g("SAT_CRYPTO", "./cryptool"),
            policy_path: g("SAT_POLICY", "policy.json"),
            trace_path: g("SAT_TRACE", "trace.log"),
            timeout_secs: g("SAT_TIMEOUT_SECS", "30").parse().unwrap_or(30),
        }
    }
}

// ============================ registry ============================
struct Registry {
    records: Vec<Value>,
    raw_lines: Vec<String>,
    genesis: String,
    head: String,
    seal_records: usize,
}
impl Registry {
    fn load(dir: &str) -> Result<Registry, String> {
        let jsonl = std::fs::read_to_string(format!("{}/tools.jsonl", dir)).map_err(|e| e.to_string())?;
        let seal = std::fs::read_to_string(format!("{}/SEAL.txt", dir)).map_err(|e| e.to_string())?;
        let mut genesis = String::new();
        let mut head = String::new();
        let mut seal_records = 0usize;
        for line in seal.lines() {
            if let Some(v) = line.strip_prefix("genesis: ") { genesis = v.to_string(); }
            if let Some(v) = line.strip_prefix("head: ") { head = v.to_string(); }
            if let Some(v) = line.strip_prefix("records: ") { seal_records = v.parse().unwrap_or(0); }
        }
        let mut records = Vec::new();
        let mut raw_lines = Vec::new();
        for line in jsonl.lines() {
            if line.trim().is_empty() { continue; }
            records.push(json::parse(line).map_err(|e| format!("registry line parse: {}", e))?);
            raw_lines.push(line.to_string());
        }
        Ok(Registry { records, raw_lines, genesis, head, seal_records })
    }
    fn verify(&self) -> bool {
        let mut tip = self.genesis.clone();
        for line in &self.raw_lines {
            tip = sha256_hex(&format!("{}|{}", tip, line));
        }
        tip == self.head && self.raw_lines.len() == self.seal_records
    }
    fn search(&self, query: &str, limit: usize) -> Vec<(usize, i64, &Value)> {
        let toks: Vec<String> = query.to_lowercase().split(|c: char| !c.is_alphanumeric())
            .filter(|t| t.len() > 2).map(|t| t.to_string()).collect();
        let mut hits: Vec<(usize, i64, &Value)> = Vec::new();
        for (i, rec) in self.records.iter().enumerate() {
            let name = rec.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let cat = rec.get("category").and_then(|v| v.as_str()).unwrap_or("");
            let prov = rec.get("provider").and_then(|v| v.as_str()).unwrap_or("");
            let mut score = 0i64;
            for t in &toks {
                if name.to_lowercase().contains(t) { score += 3; }
                if cat.to_lowercase().contains(t) { score += 2; }
                if prov.to_lowercase().contains(t) { score += 1; }
            }
            if score > 0 { hits.push((i, score, rec)); }
        }
        hits.sort_by(|a, b| {
            let ta = a.2.get("tier").and_then(|v| v.as_str()).unwrap_or("T9");
            let tb = b.2.get("tier").and_then(|v| v.as_str()).unwrap_or("T9");
            b.1.cmp(&a.1).then(ta.cmp(tb))
        });
        hits.truncate(limit);
        hits
    }
    fn find(&self, provider: &str, name: &str) -> Option<(usize, &Value)> {
        let norm = |p: &str| p.trim().trim_start_matches("MCP ").to_lowercase();
        self.records.iter().enumerate().find(|(_, r)| {
            r.get("name").and_then(|v| v.as_str()) == Some(name)
                && (provider.is_empty() || norm(r.get("provider").and_then(|v| v.as_str()).unwrap_or("")) == norm(provider))
        })
    }
}

// ============================ backends ============================
struct JuliaWorker {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<String>,
}
struct Backends {
    julia: Option<JuliaWorker>,
    crypto_checked: bool,
}
fn julia_start(cfg: &Config) -> Result<JuliaWorker, String> {
    let mut child = Command::new(&cfg.julia)
        .args(["--startup-file=no", "--project=.", "backends/compute.jl"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
        .spawn().map_err(|e| format!("julia spawn: {}", e))?;
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => { if tx.send(line).is_err() { break; } }
            }
        }
    });
    Ok(JuliaWorker { child, stdin, rx })
}
fn julia_call(state: &mut State, req: &Value) -> Result<Value, String> {
    let timeout = std::time::Duration::from_secs(state.cfg.timeout_secs);
    if state.backends.julia.is_none() {
        state.backends.julia = Some(julia_start(&state.cfg)?);
    }
    let w = state.backends.julia.as_mut().unwrap();
    w.stdin.write_all(req.to_json().as_bytes()).map_err(|e| e.to_string())?;
    w.stdin.write_all(b"\n").map_err(|e| e.to_string())?;
    w.stdin.flush().map_err(|e| e.to_string())?;
    match w.rx.recv_timeout(timeout) {
        Ok(line) => json::parse(line.trim()).map_err(|e| format!("julia reply parse: {}", e)),
        Err(_) => {
            // timed out: kill and forget the worker; the next call
            // respawns it. The timeout IS the cancellation surface.
            if let Some(mut w) = state.backends.julia.take() { let _ = w.child.kill(); }
            Err(format!("julia backend timed out after {}s — worker killed, will respawn", state.cfg.timeout_secs))
        }
    }
}
fn crypto_call(state: &State, args: &[String]) -> Result<Value, String> {
    let out = Command::new(&state.cfg.crypto).args(args).output().map_err(|e| format!("crypto spawn: {}", e))?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let line = text.lines().next().unwrap_or("").to_string();
    json::parse(&line).map_err(|e| format!("crypto reply parse: {}", e))
}
fn godot_call(state: &State, job: &Value) -> Result<Value, String> {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let job_path = format!("/tmp/sat-graph-{}-job.json", stamp);
    let out_path = format!("/tmp/sat-graph-{}-out.json", stamp);
    std::fs::write(&job_path, job.to_json()).map_err(|e| e.to_string())?;
    let out = Command::new(&state.cfg.godot)
        .args(["--headless", "--script", "backends/graph_runner.gd", "--", &job_path, &out_path])
        .output().map_err(|e| format!("godot spawn: {}", e))?;
    if !out.status.success() {
        return Err(format!("godot exited {}: {}", out.status, String::from_utf8_lossy(&out.stderr)));
    }
    let text = std::fs::read_to_string(&out_path).map_err(|e| format!("graph output missing: {}", e))?;
    let _ = std::fs::remove_file(&job_path);
    let _ = std::fs::remove_file(&out_path);
    json::parse(text.trim()).map_err(|e| format!("graph reply parse: {}", e))
}

// ============================ state ============================
struct State {
    cfg: Config,
    registry: Registry,
    policy: Value,
    backends: Backends,
    traces: Vec<Value>,
    trace_seq: u64,
    request_seq: u64,
    started: Instant,
}
impl State {
    fn trace(&mut self, entry: Value) -> String {
        self.trace_seq += 1;
        let id = format!("tr-{}", self.trace_seq);
        let mut e = entry;
        if let Value::Obj(pairs) = &mut e { pairs.push(("trace_id".to_string(), s(&id))); }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&self.cfg.trace_path) {
            let _ = writeln!(f, "{}", e.to_json());
        }
        self.traces.push(e);
        id
    }
    fn policy_allows(&self, class: &str) -> bool {
        self.policy.get("classes").and_then(|c| c.get(class)).and_then(|v| v.as_str()) == Some("allow")
    }
}

// ============================ op table ============================
// (operation, backend, permission class, required args)
fn op_spec(op: &str) -> Option<(&'static str, &'static str, &'static [&'static str])> {
    Some(match op {
        "array.create" => ("julia", "compute", &["data"] as &[&str]),
        "array.reshape" => ("julia", "compute", &["data", "shape"]),
        "array.transpose" => ("julia", "compute", &["data"]),
        "array.slice" => ("julia", "compute", &["data", "ranges"]),
        "array.concatenate" => ("julia", "compute", &["a", "b"]),
        "matrix.multiply" => ("julia", "compute", &["a", "b"]),
        "matrix.inverse" => ("julia", "compute", &["a"]),
        "matrix.determinant" => ("julia", "compute", &["a"]),
        "matrix.solve" => ("julia", "compute", &["a", "b"]),
        "matrix.eigenvalues" => ("julia", "compute", &["a"]),
        "tensor.contract" => ("julia", "compute", &["a", "b", "dims_a", "dims_b"]),
        "tensor.reduce" => ("julia", "compute", &["data", "reduce_op"]),
        "tensor.broadcast" => ("julia", "compute", &["a", "b", "broadcast_op"]),
        "crypto.hash" => ("crypto", "crypto", &["algorithm"]),
        "crypto.hmac" => ("crypto", "crypto", &["key"]),
        "crypto.encrypt" => ("crypto", "crypto", &["key_hex"]),
        "crypto.decrypt" => ("crypto", "crypto", &["key_hex", "iv_hex", "ciphertext_hex", "tag_hex"]),
        "crypto.sign" => ("crypto", "crypto", &["seed_hex"]),
        "crypto.verify" => ("crypto", "crypto", &["public_key_hex", "signature_hex"]),
        "graph.create" => ("graph", "graph", &["nodes", "edges"]),
        "graph.traverse" => ("graph", "graph", &["nodes", "edges", "start"]),
        "graph.topological_sort" => ("graph", "graph", &["nodes", "edges"]),
        "graph.detect_cycle" => ("graph", "graph", &["nodes", "edges"]),
        _ => return None,
    })
}
fn is_number_tree(v: &Value) -> bool {
    match v {
        Value::Num(_) => true,
        Value::Arr(items) => items.iter().all(is_number_tree),
        _ => false,
    }
}
fn validate_args(op: &str, args: &Value) -> Result<(), String> {
    let (_, _, required) = op_spec(op).ok_or_else(|| format!("unsupported operation: {}", op))?;
    for r in required {
        if args.get(r).is_none() || args.get(r).unwrap().is_null() {
            return Err(format!("missing required argument: {}", r));
        }
    }
    // operand-shaped args must be literals of numbers or {ref|data}
    for key in ["data", "a", "b"] {
        if let Some(v) = args.get(key) {
            let ok = match v {
                Value::Arr(_) => is_number_tree(v),
                Value::Obj(_) => v.get("ref").is_some() || v.get("data").map(is_number_tree).unwrap_or(false),
                _ => false,
            };
            if !ok && !op.starts_with("crypto") && !op.starts_with("graph") {
                return Err(format!("argument {} must be a numeric array or an operand object", key));
            }
        }
    }
    for key in ["key_hex", "iv_hex", "ciphertext_hex", "tag_hex", "seed_hex", "public_key_hex", "signature_hex", "data_hex", "key", "plaintext_hex", "message_hex"] {
        if let Some(v) = args.get(key) {
            if op.starts_with("crypto") {
                if key == "key" || key.ends_with("_hex") {
                    let text = v.as_str().ok_or_else(|| format!("{} must be a string", key))?;
                    if key.ends_with("_hex") && (!text.chars().all(|c| c.is_ascii_hexdigit()) || text.len() % 2 != 0) {
                        return Err(format!("{} must be even-length hex", key));
                    }
                }
            }
        }
    }
    Ok(())
}
fn hex_of(args: &Value, plain_key: &str, hex_key: &str) -> Result<String, String> {
    if let Some(h) = args.get(hex_key).and_then(|v| v.as_str()) {
        return Ok(h.to_string());
    }
    if let Some(p) = args.get(plain_key).and_then(|v| v.as_str()) {
        return Ok(p.bytes().map(|b| format!("{:02x}", b)).collect());
    }
    Err(format!("missing {} or {}", plain_key, hex_key))
}
fn envelope(state: &mut State, op: &str, backend: &str, args: &Value, status: &str,
            result: Value, error: Value, verification: &str, started: Instant) -> Value {
    state.request_seq += 1;
    let input_types: Vec<Value> = args.as_obj().map(|pairs| {
        pairs.iter().map(|(k, v)| obj(vec![("name", s(k)), ("type", s(v.type_name()))])).collect()
    }).unwrap_or_default();
    let output_type = result.get("output_type").and_then(|v| v.as_str()).map(|t| t.to_string())
        .unwrap_or_else(|| result.type_name().to_string());
    let trace_id = state.trace(obj(vec![
        ("operation", s(op)), ("backend", s(backend)), ("status", s(status)),
    ]));
    obj(vec![
        ("request_id", s(&format!("req-{}", state.request_seq))),
        ("operation", s(op)),
        ("backend", s(backend)),
        ("input_types", Value::Arr(input_types)),
        ("output_type", s(&output_type)),
        ("result", result),
        ("execution_status", s(status)),
        ("error", error),
        ("elapsed_time_ms", n(started.elapsed().as_secs_f64() * 1000.0)),
        ("verification_status", s(verification)),
        ("trace_id", s(&trace_id)),
    ])
}

// ============================ executors ============================
fn exec_compute(state: &mut State, op: &str, args: &Value, session: &str) -> Value {
    let started = Instant::now();
    let (backend, class, _) = match op_spec(op) {
        Some(spec) => spec,
        None => return envelope(state, op, "none", args, "error", Value::Null,
            s(&format!("unsupported operation: {}", op)), "not-run", started),
    };
    if !state.policy_allows(class) {
        return envelope(state, op, backend, args, "denied", Value::Null,
            s(&format!("permission class '{}' is denied by policy", class)), "not-run", started);
    }
    if let Err(e) = validate_args(op, args) {
        return envelope(state, op, backend, args, "invalid", Value::Null, s(&e), "schema-validation-failed", started);
    }
    match backend {
        "julia" => {
            let req = obj(vec![
                ("op", s(op)), ("session", s(session)), ("args", args.clone()),
            ]);
            match julia_call(state, &req) {
                Ok(reply) if reply.get("ok").and_then(|v| match v { Value::Bool(b) => Some(*b), _ => None }) == Some(true) => {
                    let verification = reply.get("verification").and_then(|v| v.as_str()).unwrap_or("executed").to_string();
                    let result = reply.get("result").cloned().unwrap_or(Value::Null);
                    let mut result = result;
                    if let (Value::Obj(pairs), Some(ot)) = (&mut result, reply.get("output_type")) {
                        pairs.push(("output_type".to_string(), ot.clone()));
                    }
                    envelope(state, op, backend, args, "ok", result, Value::Null, &verification, started)
                }
                Ok(reply) => {
                    let e = reply.get("error").and_then(|v| v.as_str()).unwrap_or("julia error").to_string();
                    envelope(state, op, backend, args, "error", Value::Null, s(&e), "backend-reported-error", started)
                }
                Err(e) => envelope(state, op, backend, args, "error", Value::Null, s(&e), "backend-unavailable", started),
            }
        }
        "crypto" => {
            let argv: Result<Vec<String>, String> = (|| {
                match op {
                    "crypto.hash" => {
                        let algo = args.get("algorithm").and_then(|v| v.as_str()).unwrap_or("sha256").to_string();
                        Ok(vec!["hash".into(), algo, hex_of(args, "data", "data_hex")?])
                    }
                    "crypto.hmac" => {
                        let algo = args.get("algorithm").and_then(|v| v.as_str()).unwrap_or("sha256").to_string();
                        Ok(vec!["hmac".into(), algo, hex_of(args, "key", "key_hex")?, hex_of(args, "data", "data_hex")?])
                    }
                    "crypto.encrypt" => Ok(vec!["encrypt".into(),
                        args.get("key_hex").and_then(|v| v.as_str()).unwrap_or("").into(),
                        hex_of(args, "plaintext", "plaintext_hex")?]),
                    "crypto.decrypt" => Ok(vec!["decrypt".into(),
                        args.get("key_hex").and_then(|v| v.as_str()).unwrap_or("").into(),
                        args.get("iv_hex").and_then(|v| v.as_str()).unwrap_or("").into(),
                        args.get("ciphertext_hex").and_then(|v| v.as_str()).unwrap_or("").into(),
                        args.get("tag_hex").and_then(|v| v.as_str()).unwrap_or("").into()]),
                    "crypto.sign" => Ok(vec!["sign".into(),
                        args.get("seed_hex").and_then(|v| v.as_str()).unwrap_or("").into(),
                        hex_of(args, "message", "message_hex")?]),
                    "crypto.verify" => Ok(vec!["verify".into(),
                        args.get("public_key_hex").and_then(|v| v.as_str()).unwrap_or("").into(),
                        hex_of(args, "message", "message_hex")?,
                        args.get("signature_hex").and_then(|v| v.as_str()).unwrap_or("").into()]),
                    _ => Err("unreachable".to_string()),
                }
            })();
            match argv {
                Err(e) => envelope(state, op, backend, args, "invalid", Value::Null, s(&e), "schema-validation-failed", started),
                Ok(argv) => match crypto_call(state, &argv) {
                    Ok(reply) if reply.get("ok").and_then(|v| match v { Value::Bool(b) => Some(*b), _ => None }) == Some(true) => {
                        let verification = if state.backends.crypto_checked { "library-vector-checked" } else { "library-unchecked" };
                        envelope(state, op, backend, args, "ok", reply.get("result").cloned().unwrap_or(Value::Null), Value::Null, verification, started)
                    }
                    Ok(reply) => {
                        let e = reply.get("error").and_then(|v| v.as_str()).unwrap_or("crypto error").to_string();
                        envelope(state, op, backend, args, "error", Value::Null, s(&e), "backend-reported-error", started)
                    }
                    Err(e) => envelope(state, op, backend, args, "error", Value::Null, s(&e), "backend-unavailable", started),
                },
            }
        }
        "graph" => {
            let mut job = args.clone();
            if let Value::Obj(pairs) = &mut job { pairs.push(("kind".to_string(), s(op))); }
            match godot_call(state, &job) {
                Ok(reply) if reply.get("ok").and_then(|v| match v { Value::Bool(b) => Some(*b), _ => None }) == Some(true) => {
                    envelope(state, op, backend, args, "ok", reply.get("result").cloned().unwrap_or(Value::Null), Value::Null, "scheduler-constructed", started)
                }
                Ok(reply) => {
                    let e = reply.get("error").and_then(|v| v.as_str()).unwrap_or("graph error").to_string();
                    envelope(state, op, backend, args, "error", Value::Null, s(&e), "backend-reported-error", started)
                }
                Err(e) => envelope(state, op, backend, args, "error", Value::Null, s(&e), "backend-unavailable", started),
            }
        }
        _ => envelope(state, op, backend, args, "error", Value::Null, s("unknown backend"), "not-run", started),
    }
}

fn exec_tool(state: &mut State, provider: &str, name: &str, args: &Value) -> Value {
    let started = Instant::now();
    let norm = |p: &str| p.trim().trim_start_matches("MCP ").to_lowercase();
    if norm(provider) == "runtime" {
        // the runtime's own capabilities are registered tools too,
        // routed through the same validation + permission gates
        return exec_compute(state, name, args, "default");
    }
    match state.registry.find(provider, name) {
        None => envelope(state, &format!("tool:{}", name), "registry", args, "error", Value::Null,
            s("tool not found in the registry — unregistered capabilities do not execute"), "not-run", started),
        Some((_, rec)) => {
            let rec = rec.clone();
            if !state.policy_allows("external_service") {
                return envelope(state, &format!("tool:{}", name), "registry", args, "denied",
                    obj(vec![("record", rec)]), s("permission class 'external_service' is denied by policy"), "policy-checked", started);
            }
            envelope(state, &format!("tool:{}", name), "registry", args, "not_executable",
                obj(vec![("record", rec)]), s("no live MCP server is configured for this provider; discovery is not execution"), "policy-checked", started)
        }
    }
}

fn exec_graph(state: &mut State, args: &Value, session: &str) -> Value {
    let started = Instant::now();
    if !state.policy_allows("graph") {
        return envelope(state, "graph.execute", "graph", args, "denied", Value::Null,
            s("permission class 'graph' is denied by policy"), "not-run", started);
    }
    // the GDScript scheduler owns ordering and state transitions
    let mut job = args.clone();
    if let Value::Obj(pairs) = &mut job { pairs.push(("kind".to_string(), s("graph.topological_sort"))); }
    let order_reply = match godot_call(state, &job) {
        Ok(r) => r,
        Err(e) => return envelope(state, "graph.execute", "graph", args, "error", Value::Null, s(&e), "backend-unavailable", started),
    };
    if order_reply.get("ok").and_then(|v| match v { Value::Bool(b) => Some(*b), _ => None }) != Some(true) {
        let e = order_reply.get("error").and_then(|v| v.as_str()).unwrap_or("scheduling failed").to_string();
        return envelope(state, "graph.execute", "graph", args, "error", Value::Null, s(&e), "scheduler-refused", started);
    }
    let order: Vec<String> = order_reply.get("result").and_then(|r| r.get("order")).and_then(|o| o.as_arr())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|t| t.to_string())).collect()).unwrap_or_default();
    let nodes = args.get("nodes").and_then(|v| v.as_arr()).cloned().unwrap_or_default();
    let mut results: Vec<(String, Value)> = Vec::new();
    let mut by_id: HashMap<String, Value> = HashMap::new();
    for id in &order {
        let node = nodes.iter().find(|nd| nd.get("id").and_then(|v| v.as_str()) == Some(id.as_str()));
        let (nop, nargs) = match node {
            Some(nd) => (
                nd.get("op").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                nd.get("args").cloned().unwrap_or(Value::Null),
            ),
            None => (String::new(), Value::Null),
        };
        if nop.is_empty() {
            // a structural node: its result is its position in the order
            let r = obj(vec![("node", s(id)), ("executed", Value::Bool(true))]);
            by_id.insert(id.clone(), r.clone());
            results.push((id.clone(), r));
            continue;
        }
        // substitute {"$node": "id"} references with that node's result
        let nargs = substitute(&nargs, &by_id);
        let env = exec_compute(state, &nop, &nargs, session);
        let status = env.get("execution_status").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let res = env.get("result").cloned().unwrap_or(Value::Null);
        by_id.insert(id.clone(), res.clone());
        results.push((id.clone(), obj(vec![("op", s(&nop)), ("status", s(&status)), ("result", res)])));
        if status != "ok" {
            return envelope(state, "graph.execute", "graph", args, "error",
                obj(vec![("order", Value::Arr(order.iter().map(|o| s(o)).collect())), ("completed", Value::Obj(results))]),
                s(&format!("node {} failed with status {}", id, status)), "scheduler-constructed", started);
        }
    }
    envelope(state, "graph.execute", "graph", args, "ok",
        obj(vec![
            ("order", Value::Arr(order.iter().map(|o| s(o)).collect())),
            ("nodes", Value::Obj(results)),
            ("states", order_reply.get("result").and_then(|r| r.get("states")).cloned().unwrap_or(Value::Null)),
        ]),
        Value::Null, "scheduler-constructed", started)
}
fn substitute(v: &Value, by_id: &HashMap<String, Value>) -> Value {
    match v {
        Value::Obj(pairs) => {
            if pairs.len() == 1 && pairs[0].0 == "$node" {
                if let Some(id) = pairs[0].1.as_str() {
                    // a node's stored result envelope part: hand back
                    // the raw result value the node produced
                    return by_id.get(id).cloned().unwrap_or(Value::Null);
                }
            }
            Value::Obj(pairs.iter().map(|(k, val)| (k.clone(), substitute(val, by_id))).collect())
        }
        Value::Arr(items) => Value::Arr(items.iter().map(|x| substitute(x, by_id)).collect()),
        other => other.clone(),
    }
}

// ============================ the eight tools ============================
fn tool_defs() -> Vec<Value> {
    let schema = |props: Vec<(&str, Value)>, required: &[&str]| -> Value {
        obj(vec![
            ("type", s("object")),
            ("properties", Value::Obj(props.into_iter().map(|(k, v)| (k.to_string(), v)).collect())),
            ("required", Value::Arr(required.iter().map(|r| s(r)).collect())),
        ])
    };
    let str_prop = |desc: &str| obj(vec![("type", s("string")), ("description", s(desc))]);
    let obj_prop = |desc: &str| obj(vec![("type", s("object")), ("description", s(desc))]);
    vec![
        obj(vec![("name", s("registry_search")),
            ("description", s("Search the sealed tool registry by keyword. Returns ranked candidate records — never the whole catalogue.")),
            ("inputSchema", schema(vec![("query", str_prop("keywords")), ("limit", obj(vec![("type", s("number"))]))], &["query"]))]),
        obj(vec![("name", s("registry_describe")),
            ("description", s("One full registry record by tool name (and provider if ambiguous).")),
            ("inputSchema", schema(vec![("name", str_prop("tool name")), ("provider", str_prop("provider (optional)"))], &["name"]))]),
        obj(vec![("name", s("registry_status")),
            ("description", s("Registry coverage (counts by evidence tier and provider), or one execution trace by trace_id. refresh:true reloads the registry from disk when its seal changed.")),
            ("inputSchema", schema(vec![("trace_id", str_prop("a trace id from an earlier call")), ("refresh", obj(vec![("type", s("boolean"))]))], &[]))]),
        obj(vec![("name", s("registry_verify")),
            ("description", s("Recompute the registry hash chain and compare it with the seal.")),
            ("inputSchema", schema(vec![], &[]))]),
        obj(vec![("name", s("compute_execute")),
            ("description", s("Typed computation without Python: array/matrix/tensor operations (Julia), graph algorithms (GDScript), cryptography (native OpenSSL). Operations: array.create/reshape/transpose/slice/concatenate, matrix.multiply/inverse/determinant/solve/eigenvalues, tensor.contract/reduce/broadcast, crypto.hash/hmac/encrypt/decrypt/sign/verify, graph.create/traverse/topological_sort/detect_cycle. Operands are numeric arrays or {ref}/{data} objects in a persistent session; store_as saves a result.")),
            ("inputSchema", schema(vec![
                ("operation", str_prop("e.g. matrix.multiply")),
                ("args", obj_prop("operation arguments")),
                ("session", str_prop("persistent session name (default: 'default')")),
            ], &["operation"]))]),
        obj(vec![("name", s("compute_inspect")),
            ("description", s("List the variables held in a compute session, with types and shapes.")),
            ("inputSchema", schema(vec![("session", str_prop("session name"))], &["session"]))]),
        obj(vec![("name", s("graph_execute")),
            ("description", s("Execute a DAG: the GDScript scheduler orders the nodes (cycle = refusal), then each node's compute op runs in that order. Node args may reference {\"$node\": \"id\"}.")),
            ("inputSchema", schema(vec![
                ("nodes", obj(vec![("type", s("array")), ("description", s("nodes: [{id, op?, args?}]"))])),
                ("edges", obj(vec![("type", s("array")), ("description", s("edges: [[from, to], ...]"))])),
                ("session", str_prop("compute session for node ops")),
            ], &["nodes", "edges"]))]),
        obj(vec![("name", s("tool_execute")),
            ("description", s("Route a registered tool call through schema validation and the permission policy. provider 'runtime' reaches the compute operations. External tools execute only when policy allows and a live server is configured — otherwise the refusal is the result.")),
            ("inputSchema", schema(vec![
                ("provider", str_prop("registry provider, or 'runtime'")),
                ("name", str_prop("tool name, or a compute operation for provider 'runtime'")),
                ("arguments", obj_prop("tool arguments")),
            ], &["provider", "name"]))]),
    ]
}

fn call_tool(state: &mut State, name: &str, args: &Value) -> (Value, bool) {
    match name {
        "registry_search" => {
            let q = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
            let limit = args.get("limit").and_then(|v| v.as_f64()).unwrap_or(10.0) as usize;
            let hits = state.registry.search(q, limit);
            let matches: Vec<Value> = hits.iter().map(|(i, score, rec)| {
                obj(vec![("index", n((*i + 1) as f64)), ("score", n(*score as f64)), ("record", (*rec).clone())])
            }).collect();
            (obj(vec![("count", n(matches.len() as f64)), ("matches", Value::Arr(matches))]), false)
        }
        "registry_describe" => {
            let tname = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let prov = args.get("provider").and_then(|v| v.as_str()).unwrap_or("");
            match state.registry.find(prov, tname) {
                Some((i, rec)) => (obj(vec![("index", n((i + 1) as f64)), ("record", rec.clone())]), false),
                None => (obj(vec![("error", s("tool not found in the registry"))]), true),
            }
        }
        "registry_status" => {
            if let Some(tid) = args.get("trace_id").and_then(|v| v.as_str()) {
                return match state.traces.iter().find(|t| t.get("trace_id").and_then(|v| v.as_str()) == Some(tid)) {
                    Some(t) => (t.clone(), false),
                    None => (obj(vec![("error", s("trace not found"))]), true),
                };
            }
            let mut note = s("registry as loaded at startup");
            if args.get("refresh").and_then(|v| match v { Value::Bool(b) => Some(*b), _ => None }) == Some(true) {
                match Registry::load(&state.cfg.registry_dir) {
                    Ok(reg) if reg.head != state.registry.head => {
                        note = s(&format!("registry reloaded after seal change: {} records", reg.records.len()));
                        state.registry = reg;
                    }
                    Ok(_) => note = s("registry unchanged (seal head identical)"),
                    Err(e) => return (obj(vec![("error", s(&format!("registry reload failed: {}", e)))]), true),
                }
            }
            let mut tiers: HashMap<String, u64> = HashMap::new();
            let mut providers: HashMap<String, u64> = HashMap::new();
            for r in &state.registry.records {
                *tiers.entry(r.get("tier").and_then(|v| v.as_str()).unwrap_or("?").to_string()).or_default() += 1;
                *providers.entry(r.get("provider").and_then(|v| v.as_str()).unwrap_or("?").to_string()).or_default() += 1;
            }
            let tiers_v = Value::Obj(tiers.into_iter().map(|(k, v)| (k, n(v as f64))).collect());
            (obj(vec![
                ("records", n(state.registry.records.len() as f64)),
                ("tiers", tiers_v),
                ("providers", n(providers.len() as f64)),
                ("seal_head", s(&state.registry.head)),
                ("seal_valid", Value::Bool(state.registry.verify())),
                ("uptime_secs", n(state.started.elapsed().as_secs_f64())),
                ("note", note),
            ]), false)
        }
        "registry_verify" => {
            let valid = state.registry.verify();
            (obj(vec![
                ("valid", Value::Bool(valid)),
                ("records", n(state.registry.records.len() as f64)),
                ("seal_head", s(&state.registry.head)),
            ]), !valid)
        }
        "compute_execute" => {
            let op = args.get("operation").and_then(|v| v.as_str()).unwrap_or("");
            let cargs = args.get("args").cloned().unwrap_or_else(|| obj(vec![]));
            let session = args.get("session").and_then(|v| v.as_str()).unwrap_or("default");
            let env = exec_compute(state, op, &cargs, session);
            let ok = env.get("execution_status").and_then(|v| v.as_str()) == Some("ok");
            (env, !ok)
        }
        "compute_inspect" => {
            let session = args.get("session").and_then(|v| v.as_str()).unwrap_or("default");
            if state.backends.julia.is_none() {
                return (obj(vec![("session", s(session)), ("variables", Value::Arr(vec![])),
                    ("note", s("compute worker not started; the session is empty"))]), false);
            }
            let req = obj(vec![("op", s("__inspect")), ("session", s(session)), ("args", obj(vec![]))]);
            match julia_call(state, &req) {
                Ok(reply) => (reply.get("result").cloned().unwrap_or(Value::Null), false),
                Err(e) => (obj(vec![("error", s(&e))]), true),
            }
        }
        "graph_execute" => {
            let session = args.get("session").and_then(|v| v.as_str()).unwrap_or("default");
            let env = exec_graph(state, args, session);
            let ok = env.get("execution_status").and_then(|v| v.as_str()) == Some("ok");
            (env, !ok)
        }
        "tool_execute" => {
            let prov = args.get("provider").and_then(|v| v.as_str()).unwrap_or("");
            let tname = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let targs = args.get("arguments").cloned().unwrap_or_else(|| obj(vec![]));
            let env = exec_tool(state, prov, tname, &targs);
            let ok = env.get("execution_status").and_then(|v| v.as_str()) == Some("ok");
            (env, !ok)
        }
        _ => (obj(vec![("error", s(&format!("unknown tool: {}", name)))]), true),
    }
}

// ============================ JSON-RPC / MCP ============================
fn handle(state: &mut State, req: &Value) -> Option<Value> {
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    if req.get("id").is_none() {
        return None; // notification: nothing to answer
    }
    let result = match method {
        "initialize" => {
            let version = params.get("protocolVersion").and_then(|v| v.as_str()).unwrap_or("2024-11-05");
            obj(vec![
                ("protocolVersion", s(version)),
                ("capabilities", obj(vec![("tools", obj(vec![]))])),
                ("serverInfo", obj(vec![("name", s("sovereign-agent-runtime")), ("version", s("0.1.0"))])),
            ])
        }
        "ping" => obj(vec![]),
        "tools/list" => {
            let defs = tool_defs();
            let page = 3usize;
            let offset: usize = params.get("cursor").and_then(|v| v.as_str()).and_then(|c| c.parse().ok()).unwrap_or(0);
            let slice: Vec<Value> = defs.iter().skip(offset).take(page).cloned().collect();
            let mut out = vec![("tools", Value::Arr(slice))];
            if offset + page < defs.len() {
                out.push(("nextCursor", s(&(offset + page).to_string())));
            }
            obj(out)
        }
        "tools/call" => {
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or_else(|| obj(vec![]));
            let (payload, is_error) = call_tool(state, name, &args);
            obj(vec![
                ("content", Value::Arr(vec![obj(vec![("type", s("text")), ("text", s(&payload.to_json()))])])),
                ("isError", Value::Bool(is_error)),
            ])
        }
        _ => {
            return Some(obj(vec![
                ("jsonrpc", s("2.0")), ("id", id),
                ("error", obj(vec![("code", n(-32601.0)), ("message", s(&format!("method not found: {}", method)))])),
            ]));
        }
    };
    Some(obj(vec![("jsonrpc", s("2.0")), ("id", id), ("result", result)]))
}

fn main() {
    let cfg = Config::from_env();
    let registry = match Registry::load(&cfg.registry_dir) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("sovereign-agent-runtime: cannot load registry at {}: {}", cfg.registry_dir, e);
            std::process::exit(1);
        }
    };
    let policy = std::fs::read_to_string(&cfg.policy_path).ok()
        .and_then(|t| json::parse(&t).ok())
        .unwrap_or_else(|| obj(vec![("classes", obj(vec![
            ("compute", s("allow")), ("crypto", s("allow")), ("graph", s("allow")),
            ("external_service", s("deny")), ("file_write", s("deny")), ("network", s("deny")),
            ("credentials", s("deny")), ("destructive", s("deny")),
        ]))]));
    let mut state = State {
        cfg, registry, policy,
        backends: Backends { julia: None, crypto_checked: false },
        traces: Vec::new(), trace_seq: 0, request_seq: 0,
        started: Instant::now(),
    };
    // crypto library self-check against the FIPS SHA-256 vector
    state.backends.crypto_checked = crypto_call(&state, &["hash".into(), "sha256".into(),
        "616263".into()])
        .map(|r| r.get("result").and_then(|v| v.get("digest_hex")).and_then(|v| v.as_str())
            == Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"))
        .unwrap_or(false);
    eprintln!("sovereign-agent-runtime ready: {} registry records, seal valid: {}, crypto vector check: {}",
        state.registry.records.len(), state.registry.verify(), state.backends.crypto_checked);

    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }
        let req = match json::parse(trimmed) {
            Ok(v) => v,
            Err(e) => {
                let resp = obj(vec![
                    ("jsonrpc", s("2.0")), ("id", Value::Null),
                    ("error", obj(vec![("code", n(-32700.0)), ("message", s(&format!("parse error: {}", e)))])),
                ]);
                let mut out = stdout.lock();
                let _ = writeln!(out, "{}", resp.to_json());
                let _ = out.flush();
                continue;
            }
        };
        if let Some(resp) = handle(&mut state, &req) {
            let mut out = stdout.lock();
            let _ = writeln!(out, "{}", resp.to_json());
            let _ = out.flush();
        }
    }
}
