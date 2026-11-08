#!/usr/bin/env python3
"""drive.py — test driver: spawns sat-runtime with a given env,
plays a scenario file, and prints each tools/call payload as one
JSON line on stdout for assertions. Test harness only."""
import json, os, subprocess, sys

server = sys.argv[1]
scenario = sys.argv[2]
env = dict(os.environ)
proc = subprocess.Popen([server], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                        stderr=subprocess.DEVNULL, env=env, text=True)
reqs = []
for line in open(scenario):
    line = line.strip()
    if line and not line.startswith("#"):
        reqs.append(line)
proc.stdin.write("\n".join(reqs) + "\n")
proc.stdin.flush()
for line in reqs:
    if '"id"' not in line:
        continue
    resp = proc.stdout.readline()
    if not resp:
        break
    r = json.loads(resp)
    res = r.get("result", {})
    if "content" in res:
        print(res["content"][0]["text"])
    else:
        print(json.dumps(res))
proc.stdin.close()
proc.wait(timeout=120)
