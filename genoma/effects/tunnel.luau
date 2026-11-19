--!strict
-- Lo stato del Tunnel (Sinestesia, update_tunnel e build_ring_shape): gli
-- anelli nascono con la sagoma dello spettro, crescono in modo esponenziale
-- (prospettiva) e ruotano ciascuno di quanto ha vissuto (vortice). Tutto a
-- passi da 1/60 s, come la versione GTK, e solo finché c'è suono o restano
-- anelli in volo: in silenzio non nasce nulla e il tunnel si svuota.
local g = require("@genoma")

local M = {}

local MAX_RINGS = 26
local RING_BIRTH = 0.05
local RING_DEATH = 1.85
local STEP = 1 / 60

-- t.bands(): i due canali, già col gain, 0..1 (64 ciascuno).
-- t.active(): se il Tunnel è l'effetto scelto.
function M.new(t: { [string]: any }): { [string]: any }
    local rings: { { [string]: any } } = {}
    local emit = 0
    local drive = g.signal({ bass = 0, punch = 0, gate = 0 })
    local version = g.signal(0)

    local audio_drive = require("effects/drive").new()

    -- La sagoma alla nascita: una banda ogni due, il destro dall'alto in
    -- basso poi il sinistro risalendo, 64 altezze 0..255.
    local function shape(l: { number }, r: { number }): { number }
        local out = table.create(64, 0)
        for n = 0, 31 do
            out[n + 1] = math.floor(math.clamp(r[2 * n + 1] or 0, 0, 1) * 255 + 0.5)
        end
        for n = 32, 63 do
            local i = 2 * (63 - n)
            out[n + 1] = math.floor(math.clamp(l[i + 1] or 0, 0, 1) * 255 + 0.5)
        end
        return out
    end

    local function step()
        local both = g.untracked(function() local a, b = t.bands() return { a, b } end)
        local l, r = both[1], both[2]
        local bass, punch, gate = audio_drive(l, r)
        local growth = 1 + 0.006 + (0.015 + bass * 0.030 + punch * 0.15) * gate
        local spin = (0.005 + bass * 0.016 + punch * 0.06) * gate
        local kept = {}
        for _, ring in rings do
            ring.scale *= growth
            ring.angle += spin
            if ring.scale < RING_DEATH then table.insert(kept, ring) end
        end
        rings = kept
        emit += (0.075 + bass * 0.10 + punch * 0.35) * gate
        if emit >= 1 then
            emit = 0
            if #rings < MAX_RINGS then
                table.insert(rings, {
                    shape = shape(l, r),
                    scale = RING_BIRTH,
                    angle = 0,
                    -- Il colore registra la botta alla nascita.
                    tint = math.clamp(bass + punch * 0.5, 0, 1),
                })
            end
        end
        drive.value = { bass = bass, punch = punch, gate = gate }
        version.value += 1
    end

    -- Il passo corre mentre il Tunnel è scelto e c'è qualcosa da muovere.
    local timer: any = nil
    local function wanted(): boolean
        return t.active() and (t.sounding() or #rings > 0)
    end
    g.watch(function() return wanted() or version.value < 0 end, function(on: boolean)
        if on and not timer then
            timer = g.timer { every = STEP, on_tick = function()
                step()
                if not g.untracked(wanted) then
                    timer:stop()
                    timer = nil
                end
            end }
        elseif not on and timer and #rings == 0 then
            timer:stop()
            timer = nil
        end
    end)

    -- I valori dello shader del Tunnel: 0 quanti anelli, da 140 i loro.
    local function fill(out: { number })
        local _ = version.value
        out[1] = #rings
        for k, ring in rings do
            local base = 140 + (k - 1) * 35 + 1
            out[base] = ring.scale
            out[base + 1] = ring.angle
            out[base + 2] = ring.tint
            for m = 0, 31 do
                out[base + 3 + m] = ring.shape[2 * m + 1] + ring.shape[2 * m + 2] * 256
            end
        end
    end

    return { fill = fill, drive = function() return drive.value end }
end

return M
