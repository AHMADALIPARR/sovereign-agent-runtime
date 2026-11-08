// client.rs — sat-client: a minimal MCP client that spawns the
// runtime server over stdio, plays a scenario file of JSON-RPC
// requests (one per line), and prints the transcript. This is the
// agent adapter's reference driver: anything that can write
// JSON-RPC lines can drive the runtime the same way.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: sat-client <server-bin> <scenario.jsonl>");
        std::process::exit(2);
    }
    let scenario = std::fs::read_to_string(&args[2]).expect("scenario file");
    let mut child = Command::new(&args[1])
        .stdin(Stdio::piped()).stdout(Stdio::piped())
        .spawn().expect("spawn server");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    for line in scenario.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        println!(">> {}", line);
        stdin.write_all(line.as_bytes()).unwrap();
        stdin.write_all(b"\n").unwrap();
        stdin.flush().unwrap();
        if line.contains("\"id\"") {
            let mut resp = String::new();
            stdout.read_line(&mut resp).expect("server reply");
            println!("<< {}", resp.trim());
        }
    }
    drop(stdin);
    let _ = child.wait();
}
