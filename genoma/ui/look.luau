--!strict
-- L'aspetto di Bioma, per integrarsi con la shell: i colori derivati come
-- in Bioma (genoma/core/theme.luau), dai ruoli del tema del desktop che il
-- kit segue (il file del tema condiviso), e i suoi due caratteri.
local g = require("@genoma")
local kit = require("@genoma/kit")

local theme = kit.theme

local M = {}

function M.background(): string return theme.get("background") end
function M.text(): string return theme.get("text") end
function M.primary(): string return theme.get("accent") end

function M.is_dark(): boolean
    local d = theme.desktop()
    if d and type(d.dark) == "boolean" then return d.dark end
    return g.colour.luma(M.background()) < 0.5
end

-- I due caratteri di Bioma: tecnico (quello che il sistema misura) ed
-- espressivo (quello che viene da una persona).
function M.technical(): string
    local d = theme.desktop()
    return (d and d.fonts and d.fonts.technical) or "Orbitron"
end
function M.expressive(): string
    local d = theme.desktop()
    return (d and d.fonts and d.fonts.expressive) or "Spectral"
end

local function lift(hex: string, amount: number): string
    return g.colour.lift(hex, amount, M.is_dark())
end

-- La famiglia del contorno: tre luminosità della tinta dello sfondo, mai
-- quella del primario.
local function structural(lightness: number): string
    local bg, accent = M.background(), M.primary()
    local bh, bs = g.colour.hsl(bg)
    if bh < 0 or bs <= 0 then
        return g.colour.hsla(0, 0, lightness, 1)
    end
    local _, as = g.colour.hsl(accent)
    local s = math.max(math.min(bs + 0.04, 0.40), math.min(as * 0.33, 0.28))
    return g.colour.hsla(bh, s, lightness, 1)
end

function M.cell(): string return lift(M.background(), 0.075) end
function M.line(): string return structural(if M.is_dark() then 0.32 else 0.72) end
function M.node(): string return structural(if M.is_dark() then 0.43 else 0.62) end
function M.rim(): string return structural(if M.is_dark() then 0.55 else 0.50) end
function M.text_muted(): string return g.colour.alpha(M.text(), 0.70) end
function M.text_faint(): string return g.colour.alpha(M.text(), 0.54) end

-- Una luce sola, dall'alto: la cima più chiara, la base più scura.
function M.gradient_top(hex: string): string
    local h, s, l, a = g.colour.hsl(hex)
    return g.colour.hsla(math.max(0, h), math.min(1, s + 0.14), math.min(1, l + 0.16), a)
end
function M.gradient_bottom(hex: string): string
    local h, s, l, a = g.colour.hsl(hex)
    return g.colour.hsla(math.max(0, h), math.max(0, s - 0.15), math.max(0, l - 0.15), a)
end

-- Il vetro di una cella, come in Bioma (cell.opacity 0,72).
function M.glass(): string return g.colour.alpha(M.cell(), 0.72) end

-- Le misure di Bioma alla densità normale.
M.cell_height = 40
M.radius_panel = 20
M.font_meta = 11
M.font_secondary = 13
M.label_tracking = 0.06

return M
