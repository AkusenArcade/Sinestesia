--!strict
-- I controlli di Bioma (genoma/components/ in Bioma: segmented, slider,
-- choice), ridotti a quello che serve qui e legati a ui/look.
local g = require("@genoma")
local look = require("ui/look")

local M = {}

local CLEAR = "#00000000"

g.timing_default {
    ui_change = { duration = 0.2, curve = { 0.2, 0, 0, 1 } },
}

-- Un'etichetta: tecnica, maiuscola, spaziata.
function M.label(text: string | () -> string): any
    return g.text {
        text = function()
            local t = if type(text) == "function" then text() else text
            return string.upper(t)
        end,
        color = look.text_muted,
        font = look.technical,
        size = look.font_meta,
        weight = 500,
        tracking = look.label_tracking,
    }
end

-- Un valore, a cifre di larghezza fissa.
function M.figure(text: () -> string): any
    return g.text {
        text = text,
        color = look.text,
        font = look.technical,
        size = look.font_meta,
        weight = 500,
        tabular = true,
        tracking = 0.04,
    }
end

-- Uno tra pochi: una traccia con la scelta in una pillola del primario.
-- t: options() → { { key, label } }, current(), on_choose(key).
function M.segmented(t: { [string]: any }): any
    local SIZE, PAD, GAP, H = 11, 3, 2, 24
    local function width_of(o: any): number
        return g.text_width { text = string.upper(o.label), font = look.technical(), size = SIZE, weight = 700, tracking = 0.04 } + 24
    end
    local function selection(): (number, number)
        local x = 0
        for _, o in t.options() do
            local w = width_of(o)
            if o.key == t.current() then return x, w end
            x += w + GAP
        end
        return 0, 0
    end
    local function track_width(): number
        local w = 0
        for i, o in t.options() do w += width_of(o) + (if i > 1 then GAP else 0) end
        return w + PAD * 2
    end
    local items: { [any]: any } = { overlay = true, attach = "corner", offset_x = PAD, offset_y = PAD, direction = "row", gap = GAP }
    for _, o in t.options() do
        local function chosen(): boolean return o.key == t.current() end
        table.insert(items, g.box {
            role = "button",
            width = function() return width_of(o) end,
            height = H,
            align = "center",
            justify = "center",
            on_click = function() t.on_choose(o.key) end,
            g.text {
                text = string.upper(o.label),
                color = function() return if chosen() then look.background() else look.text_muted() end,
                font = look.technical,
                size = SIZE,
                weight = 700,
                tracking = 0.04,
            },
        })
    end
    return g.capsule {
        role = "group",
        width = track_width,
        height = H + PAD * 2,
        radius = look.round((H + PAD * 2) / 2),
        fill = function() return g.colour.alpha(look.background(), 0.8) end,
        rim_top = look.node,
        rim_bottom = look.node,
        g.capsule {
            overlay = true,
            attach = "corner",
            present = function() local _, w = selection() return w > 0 end,
            offset_x = function() local x = selection() return PAD + x end,
            offset_y = PAD,
            width = function() local _, w = selection() return w end,
            height = H,
            radius = look.round(H / 2),
            fill = function() return look.gradient_top(look.primary()) end,
            fill_bottom = function() return look.gradient_bottom(look.primary()) end,
            rim_top = CLEAR,
            rim_bottom = CLEAR,
            transition = { offset_x = "ui_change", width = "ui_change" },
        },
        g.box(items),
    }
end

-- Un livello: traccia sottile, il percorso nel primario, la maniglia in
-- cima. t: width, value() 0…1, on_move(v).
function M.slider(t: { [string]: any }): any
    local TRACK, HANDLE = 3, 14
    local held = false
    local function travel(): number return math.clamp(t.value(), 0, 1) end
    local function report(x: number) t.on_move(math.clamp(x / t.width, 0, 1)) end
    return g.box {
        role = "slider",
        name = t.name,
        width = t.width,
        height = HANDLE,
        isolate = true,
        on_press = function(button: number, x: number)
            if button ~= 272 then return end
            held = true
            report(x)
        end,
        on_drag = function(x: number) if held then report(x) end end,
        on_release = function() held = false end,
        on_click = function() end,
        on_wheel = function(_: number, dy: number): boolean
            local step = t.step or 0.05
            t.on_move(math.clamp(g.untracked(t.value) + (if dy < 0 then step else -step), 0, 1))
            return true
        end,
        g.capsule {
            overlay = true,
            attach = "corner",
            offset_y = (HANDLE - TRACK) / 2,
            width = t.width,
            height = TRACK,
            radius = look.round(TRACK / 2),
            fill = function() return g.colour.alpha(look.line(), 0.55) end,
            rim_top = CLEAR,
            rim_bottom = CLEAR,
        },
        g.capsule {
            overlay = true,
            attach = "corner",
            offset_y = (HANDLE - TRACK) / 2,
            width = function() return t.width * travel() end,
            height = TRACK,
            radius = look.round(TRACK / 2),
            fill = function() return look.gradient_top(look.primary()) end,
            fill_bottom = function() return look.gradient_bottom(look.primary()) end,
            rim_top = CLEAR,
            rim_bottom = CLEAR,
        },
        g.capsule {
            overlay = true,
            attach = "corner",
            offset_x = function() return t.width * travel() - HANDLE / 2 end,
            width = HANDLE,
            height = HANDLE,
            radius = look.round(HANDLE / 2),
            fill = function() return look.gradient_top(look.primary()) end,
            fill_bottom = function() return look.gradient_bottom(look.primary()) end,
            rim_top = CLEAR,
            rim_bottom = CLEAR,
        },
    }
end

-- Una scelta tra molte, come una chip di Bioma: piena nel primario quando
-- è quella, contornata altrimenti; `dimmed` la lascia vedere ma non
-- risponde. t: label, chosen(), dimmed(), on_click.
function M.chip(t: { [string]: any }): any
    local H = 26
    local over = g.signal(false)
    local function chosen(): boolean return t.chosen() end
    return g.capsule {
        role = "button",
        name = t.label,
        height = H,
        padding_x = 12,
        radius = look.round(H / 2),
        align = "center",
        justify = "center",
        opacity = function() return if t.dimmed and t.dimmed() then 0.45 else 1 end,
        fill = function()
            if chosen() then return look.gradient_top(look.primary()) end
            return if over.value then g.colour.alpha(look.text(), 0.06) else CLEAR
        end,
        fill_bottom = function()
            if chosen() then return look.gradient_bottom(look.primary()) end
            return if over.value then g.colour.alpha(look.text(), 0.06) else CLEAR
        end,
        rim_top = function() return if chosen() then CLEAR else look.node() end,
        rim_bottom = function() return if chosen() then CLEAR else look.node() end,
        transition = { fill = "ui_change", fill_bottom = "ui_change", rim_top = "ui_change", rim_bottom = "ui_change" },
        on_hover = function(on: boolean) over.value = on end,
        on_click = function()
            if t.dimmed and g.untracked(t.dimmed) then return end
            t.on_click()
        end,
        g.text {
            text = string.upper(t.label),
            color = function() return if chosen() then look.background() else look.text() end,
            font = look.technical,
            size = look.font_meta,
            weight = 500,
            tracking = look.label_tracking,
        },
    }
end

return M
