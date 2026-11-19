--!strict
-- Sinestesia su Genoma: il visualizzatore audio come finestra Genoma, gli
-- effetti disegnati come shader dalle bande di `@genoma/spectrum`.
--
--   genoma ~/Documents/Development/Sinestesia/genoma
--
-- F11: schermo intero (e niente controlli); H: nasconde o mostra i
-- controlli; Esc: torna in finestra. La cattura gira finché la finestra
-- c'è. La chrome segue il tema del desktop (il kit legge il file del tema
-- condiviso che Bioma scrive); l'area del visualizzatore resta nera.
local g = require("@genoma")
local kit = require("@genoma/kit")
local spectrum = require("@genoma/spectrum")
local config = require("config")

local theme = kit.theme

-- ---- Impostazioni -------------------------------------------------------------

local settings = g.signal(config.load())
local saving: any = nil
local function set(key: string, value: any)
    local s = table.clone(g.untracked(function() return settings.value end)) :: any
    s[key] = value
    settings.value = s
    -- Scritte quando la mano si ferma: uno slider trascinato cambia molte
    -- volte.
    if saving then saving:stop() end
    saving = g.after(0.4, function()
        saving = nil
        config.save(g.untracked(function() return settings.value end))
    end)
end
local function get(key: string): any return (settings.value :: any)[key] end

-- ---- La cattura -----------------------------------------------------------------

spectrum:listen_to(g.untracked(function() return get("source") end))
local hold = spectrum:hold()
-- Cambiare sorgente fa ripartire la cattura su quella nuova.
local function set_source(source: string)
    if source == g.untracked(function() return get("source") end) then return end
    set("source", source)
    hold:release()
    spectrum:listen_to(source)
    hold = spectrum:hold()
end

-- Qualcosa suona, o ha smesso da poco: la scia va ridisegnata finché sfuma,
-- poi tutto si ferma.
local alive = g.signal(false)
local quieting: any = nil
g.watch(function() return spectrum.level > 0.002 end, function(sounding: boolean)
    if quieting then quieting:stop() end
    quieting = nil
    if sounding then
        alive.value = true
    else
        quieting = g.after(2, function()
            quieting = nil
            alive.value = false
        end)
    end
end)

-- ---- Colori --------------------------------------------------------------------

-- Automatici: l'accento del tema e la stessa tinta schiarita (Sinestesia,
-- src/theme.rs). Manuali: i due scelti.
local function lighten(hex: string, t: number): string
    local c = g.colour.parse(hex) or { r = 1, g = 1, b = 1, a = 1 }
    return g.colour.format(c.r + (1 - c.r) * t, c.g + (1 - c.g) * t, c.b + (1 - c.b) * t)
end
local function colour_a(): string
    if get("color_mode") == "auto" then return theme.get("accent") end
    return get("color_a")
end
local function colour_b(): string
    if get("color_mode") == "auto" then return lighten(theme.get("accent"), 0.5) end
    return get("color_b")
end

-- Quello che ogni effetto legge: 128 bande, i due colori, il gain, il picco.
local function values(): { number }
    local l, r = spectrum.left, spectrum.right
    local gain = get("gain")
    local out = table.create(136, 0)
    local peak = 0
    for i = 1, 64 do
        local a, b = l[i] or 0, r[i] or 0
        out[i] = a
        out[64 + i] = b
        peak = math.max(peak, a, b)
    end
    local a = g.colour.parse(colour_a()) or { r = 1, g = 1, b = 1 }
    local b = g.colour.parse(colour_b()) or { r = 1, g = 1, b = 1 }
    out[129], out[130], out[131] = a.r, a.g, a.b
    out[132], out[133], out[134] = b.r, b.g, b.b
    out[135] = gain
    out[136] = math.clamp(peak * gain, 0, 1)
    return out
end

-- ---- Gli effetti ------------------------------------------------------------------

-- Nell'ordine del menu della versione GTK; `src` per quelli già portati.
local EFFECTS = {
    { key = "bars", label = "Barre", src = "effects/bars.wgsl" },
    { key = "line", label = "Linea", src = "effects/line.wgsl" },
    { key = "radial", label = "Radiale", src = "effects/radial.wgsl" },
    { key = "linespectrum", label = "Linea Spettro", src = "effects/line_spectrum.wgsl" },
    { key = "radialspectrum", label = "Radiale Spettro", src = "effects/radial_spectrum.wgsl" },
    { key = "tunnel", label = "Tunnel" },
    { key = "solid", label = "Poliedro" },
    { key = "imaging", label = "Imaging" },
    { key = "terrain", label = "Rilievo" },
    { key = "phase", label = "Fase" },
    { key = "nebula", label = "Nebulosa" },
}
local function effect_index(): number
    for i, e in EFFECTS do
        if e.key == get("effect") then return i end
    end
    return 1
end

local layers: { any } = {}
for _, e in EFFECTS do
    if e.src then
        table.insert(layers, g.shader {
            present = function() return get("effect") == e.key end,
            grow = 1,
            src = g.dir .. "/" .. e.src,
            values = values,
        })
    end
end
-- La scia: il quadro di prima, sbiadito sotto il nuovo. In Sinestesia un
-- velo nero di opacità 1 − scia (almeno 0,05) a ogni frame.
table.insert(layers, g.shader {
    pass = true,
    src = g.dir .. "/effects/trail.wgsl",
    present = function() return get("blur") > 0 end,
    clock = function() return alive.value end,
    fps = 60,
    values = function() return { 1 - math.clamp(1 - get("blur"), 0.05, 1) } end,
})

-- ---- Le particelle del radiale ---------------------------------------------------------
--
-- Ogni raggio che sale spara particelle dalla punta verso l'esterno, tante
-- e veloci quanto è salito (Sinestesia, update_particles: a ogni passo da
-- 1/60 s, fino a 3 se la banda è salita di più di 0,02, a 0,005 + 0,06 ×
-- salita NDC per passo, vive 1/0,018 passi). Qui un emettitore per raggio,
-- con ritmo e velocità legati alla salita.

local area = g.signal({ w = 1100, h = 560 })
local rise = g.signal(table.create(128, 0))
do
    local before = table.create(128, 0)
    g.watch(function() return spectrum.left end, function()
        local l, r = g.untracked(function() return spectrum.left end), g.untracked(function() return spectrum.right end)
        local gain = g.untracked(function() return get("gain") end)
        local out = table.create(128, 0)
        for i = 1, 64 do
            for side = 0, 1 do
                local k = side * 64 + i
                local h = math.clamp(((if side == 0 then l[i] else r[i]) or 0) * gain, 0, 1)
                out[k] = h - before[k]
                before[k] = h
            end
        end
        rise.value = out
    end)
end
local function radial_on(): boolean
    local e = get("effect")
    return e == "radial" or e == "radialspectrum"
end
local STEPS = 60
local particles: { any } = {}
for k = 1, 128 do
    local i = (k - 1) % 64
    local right = k > 64
    -- L'angolo del raggio, antiorario da destra: il destro scende dall'alto
    -- a destra, il sinistro a sinistra.
    local angle = if right then math.pi / 2 - i / 63 * math.pi else math.pi / 2 + i / 63 * math.pi
    local function tip(): (number, number)
        local a = area.value
        local raw = (if right then spectrum.right else spectrum.left)[i + 1] or 0
        local band = raw * get("gain")
        local r = (0.28 + math.clamp(band, 0, 1) * 0.62) * a.h / 2
        return a.w / 2 + math.cos(angle) * r, a.h / 2 - math.sin(angle) * r
    end
    local function climb(): number
        local d = rise.value[k] or 0
        return if d > 0.02 then d else 0
    end
    table.insert(particles, g.emitter {
        overlay = true,
        attach = "corner",
        present = radial_on,
        offset_x = function() local x = tip() return x end,
        offset_y = function() local _, y = tip() return y end,
        width = 1,
        height = 1,
        rate = function() return math.min(3, math.floor(climb() * 9)) * STEPS end,
        speed = function() return (0.005 + climb() * 0.06) * STEPS * area.value.h / 2 end,
        heading = 90 - math.deg(angle),
        spread = 4,
        lifetime = 1 / (0.018 * STEPS),
        particle_size = 3,
        size_end = 2,
        fill = colour_a,
        color_end = function() return string.sub(colour_b(), 1, 7) .. "00" end,
    })
end

local function unported(): string?
    local e = EFFECTS[effect_index()]
    if e.src then return nil end
    return e.label .. ": non ancora su Genoma"
end

local stage = g.capsule {
    grow = 1,
    radius = 0,
    fill = "#000000",
    on_size = function(w: number, h: number) area.value = { w = w, h = h } end,
    direction = "column",
    align = "stretch",
    g.box({ grow = 1, direction = "column", align = "stretch", table.unpack(layers) } :: any),
    g.box({
        overlay = true,
        attach = "corner",
        width = function() return area.value.w end,
        height = function() return area.value.h end,
        clip = true,
        table.unpack(particles),
    } :: any),
    g.box {
        overlay = true,
        present = function() return unported() ~= nil end,
        align = "center",
        justify = "center",
        g.text {
            text = function() return unported() or "" end,
            size = function() return theme.get("text_size") end,
            color = function() return theme.get("faint") end,
        },
    },
}

-- ---- I controlli -------------------------------------------------------------------

local function label(text: string): any
    return g.text {
        text = text,
        size = function() return theme.get("text_size") end,
        color = function() return theme.get("text") end,
    }
end
local function figure(text: () -> string): any
    return g.text {
        text = text,
        size = function() return theme.get("text_size") end,
        color = function() return theme.get("faint") end,
        tabular = true,
    }
end
local function separator(): any
    return g.capsule { width = 1, height = 22, radius = 0, fill = function() return theme.get("line") end }
end
-- Con la virgola, come la versione GTK in italiano.
local function decimal(v: number, digits: number): string
    local s = string.format("%." .. digits .. "f", v)
    return (string.gsub(s, "%.", ","))
end

local GAIN_MIN, GAIN_MAX = 0.1, 10

local controls = g.capsule {
    radius = 0,
    fill = function() return theme.get("background") end,
    direction = "row",
    align = "center",
    gap = 10,
    padding_x = 12,
    height = 52,
    label("Effetto"),
    kit.dropdown {
        width = 150,
        options = function()
            local out = {}
            for _, e in EFFECTS do table.insert(out, e.label) end
            return out
        end,
        selected = effect_index,
        on_select = function(i: number) set("effect", EFFECTS[i].key) end,
    },
    separator(),
    label("Uscita"),
    kit.toggle {
        name = "Ingresso",
        on = function() return get("source") == "input" end,
        on_toggle = function(on: boolean) set_source(if on then "input" else "output") end,
    },
    label("Ingresso"),
    separator(),
    label("Tema auto"),
    kit.toggle {
        name = "Tema auto",
        on = function() return get("color_mode") == "auto" end,
        on_toggle = function(on: boolean) set("color_mode", if on then "auto" else "manual") end,
    },
    g.box {
        direction = "row",
        gap = 8,
        opacity = function() return if get("color_mode") == "auto" then theme.get("disabled_opacity") else 1 end,
        kit.color_picker.button { name = "Colore A", color = colour_a, on_change = function(hex: string)
            set("color_mode", "manual")
            set("color_a", hex)
        end },
        kit.color_picker.button { name = "Colore B", color = colour_b, on_change = function(hex: string)
            set("color_mode", "manual")
            set("color_b", hex)
        end },
    },
    separator(),
    label("Gain"),
    kit.slider {
        name = "Gain",
        width = 120,
        step = 0.1 / (GAIN_MAX - GAIN_MIN),
        value = function() return (get("gain") - GAIN_MIN) / (GAIN_MAX - GAIN_MIN) end,
        on_change = function(v: number)
            set("gain", math.floor((GAIN_MIN + v * (GAIN_MAX - GAIN_MIN)) * 10 + 0.5) / 10)
        end,
    },
    figure(function() return decimal(get("gain"), 1) end),
    label("Scia"),
    kit.slider {
        name = "Scia",
        width = 120,
        step = 0.05 / 0.95,
        value = function() return get("blur") / 0.95 end,
        on_change = function(v: number) set("blur", math.floor(v * 0.95 * 100 + 0.5) / 100) end,
    },
    figure(function() return decimal(get("blur"), 2) end),
}

-- ---- La finestra ----------------------------------------------------------------------

-- Volutamente non salvati: si riapre sempre in finestra, con i controlli.
local whole = g.signal(false)
local chrome = g.signal(true)

local function key(name: string, text: string?)
    if name == "F11" then
        local on = not g.untracked(function() return whole.value end)
        whole.value = on
        chrome.value = not on
    elseif text == "h" or text == "H" then
        chrome.value = not g.untracked(function() return chrome.value end)
    elseif name == "Escape" then
        whole.value = false
        chrome.value = true
    end
end

return g.window {
    title = "Sinestesia",
    app_id = "dev.akusen.sinestesia",
    width = 1100,
    height = 640,
    min = { 480, 320 },
    fullscreen = function() return whole.value end,
    on_fullscreen = function(on: boolean)
        whole.value = on
        if not on then chrome.value = true end
    end,
    on_close = function() g.quit() end,
    g.capsule {
        grow = 1,
        radius = 0,
        direction = "column",
        align = "stretch",
        fill = "#000000",
        catch = true,
        on_key = key,
        g.box {
            present = function() return chrome.value end,
            direction = "column",
            align = "stretch",
            kit.titlebar { title = "Sinestesia", on_close = function() g.quit() end },
        },
        stage,
        g.box {
            present = function() return chrome.value end,
            direction = "column",
            align = "stretch",
            controls,
        },
    },
}
