# export_data.jl — Phase 1 data products, generated FROM the
# sealed registry (tool-discovery-engine). Fields the sources do
# not carry are left empty — never invented.
#   julia backends/export_data.jl <registry_dir> <out_dir>

reg_dir, out_dir = ARGS[1], ARGS[2]
mkpath(joinpath(out_dir, "tool_schemas"))

# reuse the strict JSON reader/writer shapes from compute.jl by
# copy: the exporter is a build tool, kept dependency-free.
include(joinpath(@__DIR__, "json_mini.jl"))

records = []
for line in eachline(joinpath(reg_dir, "tools.jsonl"))
    isempty(strip(line)) && continue
    push!(records, parse_json(line))
end

csvesc(v) = begin
    t = v === nothing ? "" : string(v)
    (occursin(',', t) || occursin('"', t) || occursin('\n', t)) ?
        "\"" * replace(t, '"' => "\"\"") * "\"" : t
end

open(joinpath(out_dir, "tool_inventory.csv"), "w") do io
    println(io, "provider,server_name,server_version,tool_name,description,input_schema,output_schema,required_parameters,optional_parameters,permissions,side_effects,transport,source_url,verification_method,discovery_timestamp")
    for r in records
        params = get(r, "parameters", nothing)
        row = [
            get(r, "provider", ""), get(r, "provider", ""), "",
            get(r, "name", ""),
            strip(get(r, "category", "") * " — " * get(r, "notes", "")),
            params === nothing ? "" : sprint(write_json, params),
            "", join(get(r, "required", String[]), ";"), "", "", "", "",
            get(r, "source_url", ""),
            "evidence tier " * get(r, "tier", "") * " (" * join(get(r, "evidence", String[]), ";") * ")",
            "2026-11-07",
        ]
        println(io, join(csvesc.(row), ","))
    end
end

open(joinpath(out_dir, "tool_inventory.jsonl"), "w") do io
    for line in eachline(joinpath(reg_dir, "tools.jsonl"))
        isempty(strip(line)) || println(io, line)
    end
end

failures = 0
open(joinpath(out_dir, "discovery_failures.jsonl"), "w") do io
    for r in records
        if get(r, "tier", "") == "T4"
            global failures += 1
            write_json(io, Dict(
                "provider" => get(r, "provider", ""), "tool_name" => get(r, "name", ""),
                "failure_class" => "incomplete_documentation",
                "detail" => "published name; parameter schema not independently captured in the source",
                "source_url" => get(r, "source_url", "")))
            println(io)
        end
    end
end

schemas = 0
for r in records
    params = get(r, "parameters", nothing)
    params === nothing && continue
    global schemas += 1
    slug = replace(lowercase(get(r, "provider", "") * "__" * get(r, "name", "")), r"[^a-z0-9]+" => "-")
    open(joinpath(out_dir, "tool_schemas", slug * ".json"), "w") do io
        write_json(io, Dict("provider" => get(r, "provider", ""), "tool_name" => get(r, "name", ""),
                            "input_schema" => params, "required" => get(r, "required", String[]),
                            "source_url" => get(r, "source_url", "")))
    end
end
println("export: $(length(records)) inventory rows | $schemas schema files | $failures incomplete-documentation records")
