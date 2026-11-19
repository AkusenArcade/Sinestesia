--!strict
-- Lo stato della Nebulosa (Sinestesia, update_nebula): la camera gira e
-- oscilla, i bassi la avvicinano, e ogni colpo forte lancia un'onda d'urto
-- dal centro, una per colpo. A passi da 1/60 s, finché c'è suono: in
-- silenzio la nebulosa si ferma dov'è.
local g = require("@genoma")

local M = {}

local STEP = 1 / 60
local DIST = 3.3
local SHOCK_SPEED = 1.5
local SHOCK_WIDTH = 0.16
local SHOCK_TRIGGER = 0.35
local MOST_SHOCKS = 8

-- t.bands(): i due canali col gain; t.active(): se è l'effetto scelto;
-- t.sounding(): se c'è suono (o ce n'è stato da poco).
function M.new(t: { [string]: any }): { [string]: any }
    local audio_drive = require("effects/drive").new()
    local time = 0
    local shocks: { { front: number, amp: number } } = {}
    local camera = g.signal({ yaw = 0, pitch = 0.35, dist = DIST })

    local function step()
        local both = g.untracked(function() local a, b = t.bands() return { a, b } end)
        local bass, punch, gate = audio_drive(both[1], both[2])
        time += STEP
        local kept = {}
        for _, sh in shocks do
            sh.front += SHOCK_SPEED * STEP
            if sh.front < 1 + SHOCK_WIDTH * 3 then table.insert(kept, sh) end
        end
        shocks = kept
        -- Una sola onda per colpo: nasce quando il colpo supera la soglia e
        -- non di nuovo finché la precedente non si è allontanata.
        local last = shocks[#shocks]
        if punch * gate > SHOCK_TRIGGER and (last == nil or last.front > 0.12) and #shocks < MOST_SHOCKS then
            table.insert(shocks, { front = 0, amp = math.min(punch * gate, 1) })
        end
        camera.value = {
            yaw = time * 0.17,
            pitch = 0.35 + math.sin(time * 0.11) * 0.22,
            dist = DIST - (bass * 0.5 + punch * 0.6) * gate,
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

    -- I valori della simulazione: 140 yaw, 141 pitch, 142 distanza, 143
    -- quante onde, da 144 fronte e ampiezza.
    local function fill(out: { number })
        local c = camera.value
        out[141], out[142], out[143] = c.yaw, c.pitch, c.dist
        out[144] = #shocks
        for k, sh in shocks do
            out[145 + (k - 1) * 2] = sh.front
            out[146 + (k - 1) * 2] = sh.amp
        end
    end

    return { fill = fill }
end

return M
