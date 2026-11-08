# compute.jl — the Julia compute worker.
#
# A persistent process speaking one JSON object per line on
# stdin/stdout:
#   in : {"op": "...", "session": "s1", "args": {...}}
#   out: {"ok": true, "result": ..., "output_type": "...", 
#         "verification": "..."} | {"ok": false, "error": "..."}
#
# Sessions are isolated namespaces (a Dict per session name).
# Operands arrive as {"data": <numeric array>} or {"ref": "name"}
# and resolve inside the session. "store_as" in args saves the
# result under that name. Julia is responsible for the numerics;
# verification fields report residual checks where the operation
# admits one (inverse, solve) and say so plainly otherwise.

using LinearAlgebra

# ---------- minimal JSON reader (strict, stdlib-free) ----------
mutable struct P
    s::String
    i::Int
end
function ws(p::P)
    while p.i <= lastindex(p.s) && p.s[p.i] in (' ', '\t', '\n', '\r')
        p.i = nextind(p.s, p.i)
    end
end
peek(p::P) = (ws(p); p.s[p.i])
function expect(p::P, c::Char)
    ws(p)
    p.s[p.i] == c || error("expected $c at $(p.i)")
    p.i = nextind(p.s, p.i)
end
function parse_string(p::P)
    expect(p, '"')
    buf = IOBuffer()
    while true
        c = p.s[p.i]
        if c == '"'
            p.i = nextind(p.s, p.i)
            return String(take!(buf))
        elseif c == '\\'
            p.i = nextind(p.s, p.i)
            e = p.s[p.i]
            if e == 'u'
                hex = p.s[nextind(p.s, p.i):nextind(p.s, p.i, 4)]
                print(buf, Char(parse(UInt32, hex, base=16)))
                p.i = nextind(p.s, p.i, 4)
            else
                print(buf, Dict('n' => '\n', 't' => '\t', 'r' => '\r',
                                '"' => '"', '\\' => '\\', '/' => '/')[e])
            end
            p.i = nextind(p.s, p.i)
        else
            print(buf, c)
            p.i = nextind(p.s, p.i)
        end
    end
end
function parse_number(p::P)
    j = p.i
    while j <= lastindex(p.s) && p.s[j] in "0123456789+-.eE"
        j = nextind(p.s, j)
    end
    v = parse(Float64, p.s[p.i:prevind(p.s, j)])
    p.i = j
    return v
end
function parse_value(p::P)
    c = peek(p)
    if c == '{'
        expect(p, '{')
        d = Dict{String,Any}()
        peek(p) == '}' && (expect(p, '}'); return d)
        while true
            k = parse_string(p)
            expect(p, ':')
            d[k] = parse_value(p)
            peek(p) == '}' && (expect(p, '}'); return d)
            expect(p, ',')
        end
    elseif c == '['
        expect(p, '[')
        a = Any[]
        peek(p) == ']' && (expect(p, ']'); return a)
        while true
            push!(a, parse_value(p))
            peek(p) == ']' && (expect(p, ']'); return a)
            expect(p, ',')
        end
    elseif c == '"'
        return parse_string(p)
    elseif c == 't'
        p.i = nextind(p.s, p.i, 4); return true
    elseif c == 'f'
        p.i = nextind(p.s, p.i, 5); return false
    elseif c == 'n'
        p.i = nextind(p.s, p.i, 4); return nothing
    else
        return parse_number(p)
    end
end
parse_json(text) = parse_value(P(text, firstindex(text)))

# ---------- JSON writer ----------
function write_json(io::IO, v)
    if v === nothing
        print(io, "null")
    elseif v isa Bool
        print(io, v ? "true" : "false")
    elseif v isa Integer
        print(io, v)
    elseif v isa AbstractFloat
        isinteger(v) && abs(v) < 9e15 ? print(io, Int(v)) : print(io, v)
    elseif v isa AbstractString
        print(io, '"')
        for c in v
            if c == '"' print(io, "\\\"")
            elseif c == '\\' print(io, "\\\\")
            elseif c == '\n' print(io, "\\n")
            elseif c == '\r' print(io, "\\r")
            elseif c == '\t' print(io, "\\t")
            else print(io, c) end
        end
        print(io, '"')
    elseif v isa AbstractDict
        print(io, '{')
        for (i, (k, val)) in enumerate(v)
            i > 1 && print(io, ',')
            write_json(io, string(k)); print(io, ':'); write_json(io, val)
        end
        print(io, '}')
    elseif v isa AbstractVector
        print(io, '[')
        for (i, x) in enumerate(v)
            i > 1 && print(io, ',')
            write_json(io, x)
        end
        print(io, ']')
    else
        write_json(io, string(v))
    end
end

# ---------- values ----------
to_native(v::Vector) = length(v) > 0 && all(x -> x isa Vector, v) ?
    reduce(vcat, [reshape(Float64.(row), 1, :) for row in v]) :
    Float64.(v)
to_native(v::Number) = Float64(v)
to_native(v) = v
to_jsonable(v::AbstractMatrix) = [v[i, :] for i in 1:size(v, 1)]
to_jsonable(v::AbstractVector) = collect(v)
to_jsonable(v::Number) = v
to_jsonable(v::AbstractArray) = [to_jsonable(x) for x in eachslice(v, dims=1)]
to_jsonable(v) = v
type_name(v::AbstractMatrix) = "Matrix{Float64} $(size(v,1))x$(size(v,2))"
type_name(v::AbstractVector) = "Vector{Float64} length $(length(v))"
type_name(v::Number) = "Float64"
type_name(v::AbstractArray) = "Array{Float64,$(ndims(v))} size $(size(v))"
type_name(v) = string(typeof(v))

const SESSIONS = Dict{String,Dict{String,Any}}()
session(name) = get!(SESSIONS, name, Dict{String,Any}())

function operand(sess, v)
    if v isa Dict
        haskey(v, "ref") && return sess[v["ref"]]
        haskey(v, "data") && return to_native(v["data"])
        error("operand object needs ref or data")
    end
    return to_native(v)
end

function handle(req)
    op = req["op"]
    sess = session(get(req, "session", "default"))
    args = get(req, "args", Dict{String,Any}())
    if op == "__inspect"
        vars = [Dict("name" => k, "type" => type_name(v)) for (k, v) in sort(collect(sess))]
        return Dict("ok" => true, "result" => Dict(
            "session" => get(req, "session", "default"), "variables" => vars))
    end
    A() = operand(sess, haskey(args, "a") ? args["a"] : args["data"])
    B() = operand(sess, args["b"])
    verification = "executed"
    result = nothing
    if op == "array.create"
        result = operand(sess, args["data"])
    elseif op == "array.reshape"
        shape = Tuple(Int.(args["shape"]))
        result = reshape(A(), shape)
    elseif op == "array.transpose"
        result = permutedims(A())
    elseif op == "array.slice"
        x = A()
        idx = Tuple((Int(r[1]):Int(r[2]) for r in args["ranges"]))
        result = x[idx...]
    elseif op == "array.concatenate"
        dims = Int(get(args, "dims", 1))
        result = cat(A(), B(), dims=dims)
    elseif op == "matrix.multiply"
        result = A() * B()
    elseif op == "matrix.inverse"
        x = A()
        result = inv(x)
        resid = maximum(abs.(x * result - I))
        verification = resid < 1e-8 ? "self-checked: max |A*A^-1 - I| = $resid" :
                       "self-check FAILED: residual $resid"
    elseif op == "matrix.determinant"
        result = det(A())
    elseif op == "matrix.solve"
        x, b = A(), B()
        result = x \ b
        resid = maximum(abs.(x * result - b))
        verification = resid < 1e-8 ? "self-checked: max |Ax - b| = $resid" :
                       "self-check FAILED: residual $resid"
    elseif op == "matrix.eigenvalues"
        result = eigvals(A())
    elseif op == "tensor.contract"
        # label contraction: dims_a/dims_b are strings like "ij","jk";
        # shared labels are summed, the rest kept in order.
        x, y = A(), B()
        la, lb = args["dims_a"], args["dims_b"]
        keep = [c for c in la if !(c in lb)]
        keep = vcat(keep, [c for c in lb if !(c in la)])
        result = contract(x, y, la, lb, keep)
        verification = "label-checked contraction"
    elseif op == "tensor.reduce"
        x = A()
        f = Dict("sum" => sum, "prod" => prod, "mean" => x -> sum(x) / length(x),
                 "max" => maximum, "min" => minimum)[args["reduce_op"]]
        dims = get(args, "dims", nothing)
        result = dims === nothing ? f(x) : dropdims(f(x, dims=Int(dims)), dims=Int(dims))
    elseif op == "tensor.broadcast"
        f = Dict("add" => (+), "sub" => (-), "mul" => (*), "div" => (/))[args["broadcast_op"]]
        result = f.(A(), B())
    else
        return Dict("ok" => false, "error" => "unsupported operation: $op")
    end
    if haskey(args, "store_as")
        sess[args["store_as"]] = result
    end
    return Dict("ok" => true, "result" => to_jsonable(result),
                "output_type" => type_name(result), "verification" => verification)
end

function contract(x, y, la, lb, keep)
    # permute both operands so contracted labels align, then a
    # reshape+multiply over the shared dimensions.
    shared = [c for c in la if c in lb]
    pa = [findfirst(==(c), la) for c in vcat(shared, [c for c in la if !(c in lb)])]
    pb = [findfirst(==(c), lb) for c in vcat(shared, [c for c in lb if !(c in la)])]
    xp = permutedims(x, pa)
    yp = permutedims(y, pb)
    ks = prod(size(xp)[1:length(shared)])
    xr = reshape(xp, ks, :)
    yr = reshape(yp, ks, :)
    out = xr' * yr
    oshape = (size(xp)[(length(shared) + 1):end]..., size(yp)[(length(shared) + 1):end]...)
    # `keep` order: free labels of a then of b, which is exactly
    # the reshape order produced above.
    length(oshape) == 0 && return out[1]
    length(oshape) == 1 && return vec(out)
    return reshape(out, oshape)
end

for line in eachline(stdin)
    isempty(strip(line)) && continue
    reply = try
        handle(parse_json(line))
    catch e
        Dict("ok" => false, "error" => sprint(showerror, e))
    end
    io = IOBuffer()
    write_json(io, reply)
    println(stdout, String(take!(io)))
    flush(stdout)
end
