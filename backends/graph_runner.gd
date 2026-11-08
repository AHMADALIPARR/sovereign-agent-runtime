extends SceneTree
## graph_runner.gd — the GDScript graph scheduler.
##
## Invoked headless by the runtime:
##   godot --headless --script backends/graph_runner.gd -- <job.json> <out.json>
##
## Job kinds: graph.create, graph.traverse, graph.topological_sort,
## graph.detect_cycle. The scheduler owns graph state transitions:
## every node moves PENDING -> READY -> DONE as its dependencies
## complete (Kahn's algorithm). A cycle is a refusal with the
## stuck set named, never a partial order presented as success.
##
## Output: {"ok": true, "result": {...}, "states": {...}}
##      |  {"ok": false, "error": "..."}

func _init() -> void:
	var args := OS.get_cmdline_user_args()
	if args.size() < 2:
		_write_out(args, {"ok": false, "error": "usage: graph_runner.gd -- <job.json> <out.json>"})
		quit(2)
		return
	var job_text := FileAccess.get_file_as_string(args[0])
	var job: Variant = JSON.parse_string(job_text)
	if job == null:
		_write_out(args, {"ok": false, "error": "job file is not valid JSON"})
		quit(0)
		return
	_write_out(args, _run(job))
	quit(0)

func _write_out(args: PackedStringArray, value: Dictionary) -> void:
	if args.size() < 2:
		return
	var f := FileAccess.open(args[1], FileAccess.WRITE)
	if f:
		f.store_string(JSON.stringify(value))
		f.close()

func _ids(nodes: Array) -> Array:
	var out := []
	for nd in nodes:
		if nd is Dictionary:
			out.append(str(nd.get("id", "")))
		else:
			out.append(str(nd))
	return out

func _adjacency(ids: Array, edges: Array) -> Dictionary:
	var adj := {}
	var indeg := {}
	for id in ids:
		adj[id] = []
		indeg[id] = 0
	for e in edges:
		if e is Array and e.size() == 2:
			var a := str(e[0])
			var b := str(e[1])
			if adj.has(a) and indeg.has(b):
				adj[a].append(b)
				indeg[b] = int(indeg[b]) + 1
	return {"adj": adj, "indeg": indeg}

## Kahn's algorithm with explicit state transitions.
## Returns {"order": [...], "states": {id: state}, "stuck": [...]}
func _schedule(ids: Array, edges: Array) -> Dictionary:
	var built := _adjacency(ids, edges)
	var adj: Dictionary = built["adj"]
	var indeg: Dictionary = built["indeg"]
	var states := {}
	for id in ids:
		states[id] = "PENDING"
	var ready: Array = []
	for id in ids:
		if int(indeg[id]) == 0:
			states[id] = "READY"
			ready.append(id)
	var order: Array = []
	while not ready.is_empty():
		var cur: String = str(ready.pop_front())
		states[cur] = "DONE"
		order.append(cur)
		for nxt in adj[cur]:
			indeg[nxt] = int(indeg[nxt]) - 1
			if int(indeg[nxt]) == 0:
				states[nxt] = "READY"
				ready.append(nxt)
	var stuck: Array = []
	for id in ids:
		if states[id] != "DONE":
			stuck.append(id)
	return {"order": order, "states": states, "stuck": stuck}

func _run(job: Dictionary) -> Dictionary:
	var kind := str(job.get("kind", ""))
	var nodes: Array = job.get("nodes", [])
	var edges: Array = job.get("edges", [])
	var ids := _ids(nodes)
	if ids.is_empty():
		return {"ok": false, "error": "graph has no nodes"}
	var sched := _schedule(ids, edges)
	var order: Array = sched["order"]
	var stuck: Array = sched["stuck"]
	match kind:
		"graph.detect_cycle":
			return {"ok": true, "result": {
				"has_cycle": not stuck.is_empty(),
				"stuck_nodes": stuck,
			}, "states": sched["states"]}
		"graph.topological_sort":
			if not stuck.is_empty():
				return {"ok": false, "error": "cycle detected; no topological order exists (stuck: %s)" % [", ".join(stuck)]}
			return {"ok": true, "result": {"order": order}, "states": sched["states"]}
		"graph.create":
			if not stuck.is_empty():
				return {"ok": false, "error": "cycle detected at creation (stuck: %s)" % [", ".join(stuck)]}
			return {"ok": true, "result": {
				"nodes": ids, "edges": edges, "order": order,
				"node_count": ids.size(), "edge_count": edges.size(),
			}, "states": sched["states"]}
		"graph.traverse":
			var start := str(job.get("start", ""))
			if not ids.has(start):
				return {"ok": false, "error": "start node not in graph: %s" % start}
			var built := _adjacency(ids, edges)
			var adj: Dictionary = built["adj"]
			var method := str(job.get("method", "bfs"))
			var seen := {}
			var visited: Array = []
			var frontier: Array = [start]
			seen[start] = true
			while not frontier.is_empty():
				var cur: String = str(frontier.pop_front() if method == "bfs" else frontier.pop_back())
				visited.append(cur)
				for nxt in adj[cur]:
					if not seen.has(nxt):
						seen[nxt] = true
						frontier.append(nxt)
			return {"ok": true, "result": {"visited": visited, "method": method}, "states": sched["states"]}
		_:
			return {"ok": false, "error": "unsupported graph kind: %s" % kind}
