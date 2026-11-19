--!strict
-- Il Poliedro (Sinestesia, build_icosphere e update_solid): la geometria,
-- fatta una volta, e lo stato che la musica muove a passi da 1/60 s.
local g = require("@genoma")

local M = {}

local STEP = 1 / 60
local DIST = 3.4

local function normalized(v: { number }): { number }
    local l = math.max(math.sqrt(v[1] * v[1] + v[2] * v[2] + v[3] * v[3]), 1e-6)
    return { v[1] / l, v[2] / l, v[3] / l }
end

-- L'icosaedro geodetico a due suddivisioni: 162 vertici sulla sfera
-- unitaria e 320 facce (indici da 0), nell'ordine della versione GTK.
local function icosphere(): ({ { number } }, { { number } })
    local phi = (1 + math.sqrt(5)) / 2
    local verts: { { number } } = {}
    for _, v in {
        { -1, phi, 0 }, { 1, phi, 0 }, { -1, -phi, 0 }, { 1, -phi, 0 },
        { 0, -1, phi }, { 0, 1, phi }, { 0, -1, -phi }, { 0, 1, -phi },
        { phi, 0, -1 }, { phi, 0, 1 }, { -phi, 0, -1 }, { -phi, 0, 1 },
    } do
        table.insert(verts, normalized(v))
    end
    local faces = {
        { 0, 11, 5 }, { 0, 5, 1 }, { 0, 1, 7 }, { 0, 7, 10 }, { 0, 10, 11 },
        { 1, 5, 9 }, { 5, 11, 4 }, { 11, 10, 2 }, { 10, 7, 6 }, { 7, 1, 8 },
        { 3, 9, 4 }, { 3, 4, 2 }, { 3, 2, 6 }, { 3, 6, 8 }, { 3, 8, 9 },
        { 4, 9, 5 }, { 2, 4, 11 }, { 6, 2, 10 }, { 8, 6, 7 }, { 9, 8, 1 },
    }
    for _ = 1, 2 do
        local cache: { [string]: number } = {}
        local function midpoint(a: number, b: number): number
            local key = math.min(a, b) .. ":" .. math.max(a, b)
            local found = cache[key]
            if found then return found end
            local pa, pb = verts[a + 1], verts[b + 1]
            table.insert(verts, normalized({ (pa[1] + pb[1]) / 2, (pa[2] + pb[2]) / 2, (pa[3] + pb[3]) / 2 }))
            cache[key] = #verts - 1
            return #verts - 1
        end
        local next = {}
        for _, f in faces do
            local a = midpoint(f[1], f[2])
            local b = midpoint(f[2], f[3])
            local c = midpoint(f[3], f[1])
            table.insert(next, { f[1], a, c })
            table.insert(next, { f[2], b, a })
            table.insert(next, { f[3], c, b })
            table.insert(next, { a, b, c })
        end
        faces = next
    end
    return verts, faces
end

-- t.bands(), t.active(), t.sounding(): come per il Tunnel.
function M.new(t: { [string]: any }): { [string]: any }
    local verts, faces = icosphere()
    -- La parte fissa dei valori: le direzioni (da 140) e le facce (da 626).
    local fixed: { [number]: number } = {}
    for i, v in verts do
        local at = 140 + (i - 1) * 3 + 1
        fixed[at], fixed[at + 1], fixed[at + 2] = v[1], v[2], v[3]
    end
    for f, face in faces do
        fixed[626 + f] = face[1] + face[2] * 256 + face[3] * 65536
    end

    local audio_drive = require("effects/drive").new()
    local time, spin = 0, 0
    local state = g.signal({ spin = 0, pitch = 0, dist = DIST, punch = 0, gate = 0 })

    local function step()
        local both = g.untracked(function() local a, b = t.bands() return { a, b } end)
        local bass, punch, gate = audio_drive(both[1], both[2])
        time += STEP
        spin += 0.002 + (bass * 0.012 + punch * 0.05) * gate
        state.value = {
            spin = spin,
            pitch = math.sin(time * 0.31) * 0.35,
            dist = DIST - (bass * 0.5 + punch * 0.7) * gate,
            punch = punch,
            gate = gate,
        }
    end

    local timer: any = nil
    g.watch(function() return t.active() and t.sounding() end, function(on: boolean)
        if on and not timer then
            timer = g.timer { every = STEP, on_tick = step }
        elseif not on and timer then
            timer:stop()
            timer = nil
        end
    end)

    local function fill(out: { number })
        local s = state.value
        out[136], out[137], out[138], out[139], out[140] = s.spin, s.pitch, s.dist, s.punch, s.gate
        for k, v in fixed do out[k] = v end
    end

    return { fill = fill, agents = #verts + #faces }
end

return M
