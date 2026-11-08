# json_mini.jl — the JSON reader/writer extracted from compute.jl
# (single implementation, reused by the data exporter).

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

