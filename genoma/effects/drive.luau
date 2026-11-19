--!strict
-- La spinta della musica per gli effetti che hanno uno stato (Sinestesia,
-- audio_drive): i bassi (le prime 12 bande dei due canali), il colpo
-- (quanto i bassi superano la propria media lenta: il livello assoluto con
-- la musica resta quasi costante, il colpo di cassa emerge sempre
-- sull'inviluppo) e il cancello del silenzio. Un'istanza per effetto: la
-- media lenta è sua.
local M = {}

function M.new(): (l: { number }, r: { number }) -> (number, number, number)
    local env = 0
    return function(l: { number }, r: { number }): (number, number, number)
        local sum, peak = 0, 0
        for i = 1, 12 do sum += (l[i] or 0) + (r[i] or 0) end
        for i = 1, 64 do peak = math.max(peak, l[i] or 0, r[i] or 0) end
        local bass = math.clamp(sum / 24, 0, 1)
        local gate = math.clamp((peak - 0.05) / 0.12, 0, 1)
        env += (bass - env) * 0.05
        local punch = math.clamp((bass - env) * 2.5, 0, 1)
        return bass, punch, gate
    end
end

return M
