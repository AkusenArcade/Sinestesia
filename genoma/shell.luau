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

-- I due canali. In ingresso il microfono è mono: arriva su un canale solo
-- (la Revelator ne dà sei), e lo si specchia sui due lati, come la
-- versione GTK.
local function channels(): ({ number }, { number })
    local l = spectrum.left
    if get("source") == "input" then return l, l end
    return l, spectrum.right
end

-- Quello che ogni effetto legge: 128 bande, i due colori, il gain, il picco.
local function values(): { number }
    local l, r = channels()
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

-- I due canali col gain, 0..1: quello che gli effetti con uno stato
-- leggono.
local function gained(): ({ number }, { number })
    local l, r = channels()
    local gain = get("gain")
    local gl, gr = table.create(64, 0), table.create(64, 0)
    for i = 1, 64 do
        gl[i] = math.clamp((l[i] or 0) * gain, 0, 1)
        gr[i] = math.clamp((r[i] or 0) * gain, 0, 1)
    end
    return gl, gr
end

-- I due colori, dove ogni shader li legge (128..133).
local function colours_into(out: { number })
    local a = g.colour.parse(colour_a()) or { r = 1, g = 1, b = 1 }
    local b = g.colour.parse(colour_b()) or { r = 1, g = 1, b = 1 }
    out[129], out[130], out[131] = a.r, a.g, a.b
    out[132], out[133], out[134] = b.r, b.g, b.b
end

local tunnel = require("effects/tunnel").new {
    bands = gained,
    active = function() return get("effect") == "tunnel" end,
    sounding = function() return alive.value end,
}
local function tunnel_values(): { number }
    local out = table.create(140 + 26 * 35, 0)
    colours_into(out)
    tunnel.fill(out)
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
    { key = "tunnel", label = "Tunnel", src = "effects/tunnel.wgsl", values = tunnel_values },
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
            values = e.values or values,
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
    local settle: any = nil
    g.watch(function() return spectrum.left end, function()
        -- g.untracked dà un valore solo: i due canali in una tabella.
        local both = g.untracked(function() local a, b = channels() return { a, b } end)
        local l, r = both[1], both[2]
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
        -- Una salita vale per il passo in cui avviene: se entro un passo non
        -- arriva uno spettro nuovo (silenzio, lo stesso spettro di prima),
        -- le punte smettono di sparare.
        if settle then settle:stop() end
        settle = g.after(1.5 / 60, function()
            settle = nil
            rise.value = table.create(128, 0)
        end)
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
        local l, r = channels()
        local raw = (if right then r else l)[i + 1] or 0
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

-- Le stelle del Tunnel: nascono vicino al punto di fuga e corrono verso
-- i bordi, tante e veloci quanto spinge la musica (update_tunnel: 2 + 6 ×
-- bassi + 4 × colpo a passo, la posizione scalata di 1 + 0,004 + (0,026 +
-- 0,055 × bassi + 0,10 × colpo) a passo, fuori a 1,6).
local function star_growth(): number
    local d = tunnel.drive()
    return 0.004 + (0.026 + d.bass * 0.055 + d.punch * 0.10) * d.gate
end
local function star_life(): number
    -- Passi per andare da ~0,065 a 1,6 crescendo di star_growth a passo.
    return math.clamp(math.log(1.6 / 0.065) / math.log(1 + star_growth()) / 60, 0.4, 4)
end
table.insert(particles, g.emitter {
    overlay = true,
    attach = "corner",
    present = function() return get("effect") == "tunnel" end,
    offset_x = function() return area.value.w / 2 - 1 end,
    offset_y = function() return area.value.h / 2 - 1 end,
    width = 2,
    height = 2,
    rate = function()
        local d = tunnel.drive()
        return math.floor((2 + d.bass * 6 + d.punch * 4) * d.gate) * 60
    end,
    lifetime = star_life,
    speed = function() return 1.55 * area.value.h / 2 / star_life() end,
    spread = 360,
    heading = 0,
    particle_size = 2.5,
    trail = 0.05,
    fill = function() return string.sub(colour_a(), 1, 7) .. "30" end,
    color_end = function() return string.sub(colour_b(), 1, 7) end,
})

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

-- ---- I controlli: una cella fluttuante -----------------------------------------------
--
-- Di solito non si vedono: c'è solo una piccola capsula col glifo di
-- Sinestesia in basso. Premuta, la cella cresce da lei, come le
-- espansioni di Bioma; si chiude ripremendola, con Esc o premendo fuori.

local look = require("ui/look")
local ui = require("ui/controls")

g.timing_default {
    cell_open = { duration = 0.32, curve = { 0.2, 0.9, 0.3, 1.05 } },
    cell_close = { duration = 0.2, curve = { 0.4, 0, 0.6, 1 } },
    content_in = { duration = 0.16, delay = 0.18, curve = { 0.2, 0, 0, 1 } },
    content_out = { duration = 0.08, curve = { 0.4, 0, 1, 1 } },
    panel_fade = { duration = 0.1, curve = { 0.2, 0, 0, 1 } },
}

-- Volutamente non salvati: si riapre sempre in finestra, con il bottone.
local whole = g.signal(false)
local chrome = g.signal(true)
local open = g.signal(false)
local PANEL_W, LABEL_W, GAP = 560, 92, 16
local SLIDER_W = PANEL_W - 2 * 22 - LABEL_W - 56

local function row(name: string, ...: any): any
    return g.box {
        direction = "row",
        align = "center",
        gap = 12,
        g.box { width = LABEL_W, ui.label(name) },
        ...,
    }
end

local chips: { [any]: any } = { direction = "row", flow = true, gap = 6, width = PANEL_W - 2 * 22 - LABEL_W - 12 }
for _, e in EFFECTS do
    table.insert(chips, ui.chip {
        label = e.label,
        chosen = function() return get("effect") == e.key end,
        dimmed = function() return e.src == nil end,
        on_click = function() set("effect", e.key) end,
    })
end

local function swatch(which: string, colour: () -> string): any
    return kit.color_picker.button {
        name = "Colore " .. which,
        color = colour,
        on_change = function(hex: string)
            set("color_mode", "manual")
            set(if which == "A" then "color_a" else "color_b", hex)
        end,
    }
end

local GAIN_MIN, GAIN_MAX = 0.1, 10
local function decimal(v: number, digits: number): string
    local s = string.format("%." .. digits .. "f", v)
    return (string.gsub(s, "%.", ","))
end

local content = g.box {
    direction = "column",
    align = "stretch",
    gap = GAP,
    padding = 22,
    opacity = function() return if open.value then 1 else 0 end,
    transition = { opacity = function() return if open.value then "content_in" else "content_out" end },
    g.box {
        direction = "row",
        align = "start",
        gap = 12,
        g.box { width = LABEL_W, height = 26, direction = "row", align = "center", ui.label("Effetto") },
        g.box(chips),
    },
    row("Sorgente", ui.segmented {
        options = function() return { { key = "output", label = "Uscita" }, { key = "input", label = "Ingresso" } } end,
        current = function() return get("source") end,
        on_choose = set_source,
    }),
    row("Colori", ui.segmented {
        options = function() return { { key = "auto", label = "Tema" }, { key = "manual", label = "Scelti" } } end,
        current = function() return get("color_mode") end,
        on_choose = function(key: string) set("color_mode", key) end,
    }, swatch("A", colour_a), swatch("B", colour_b)),
    row("Gain", ui.slider {
        name = "Gain",
        width = SLIDER_W,
        step = 0.1 / (GAIN_MAX - GAIN_MIN),
        value = function() return (get("gain") - GAIN_MIN) / (GAIN_MAX - GAIN_MIN) end,
        on_move = function(v: number)
            set("gain", math.floor((GAIN_MIN + v * (GAIN_MAX - GAIN_MIN)) * 10 + 0.5) / 10)
        end,
    }, ui.figure(function() return decimal(get("gain"), 1) end)),
    row("Scia", ui.slider {
        name = "Scia",
        width = SLIDER_W,
        step = 0.05 / 0.95,
        value = function() return get("blur") / 0.95 end,
        on_move = function(v: number) set("blur", math.floor(v * 0.95 * 100 + 0.5) / 100) end,
    }, ui.figure(function() return decimal(get("blur"), 2) end)),
}

-- Quanto è alta la cella aperta: quello che contiene.
local panel_h = g.signal(300)

local BUTTON = look.cell_height
local MARGIN = 18
local GAP_UP = 10

-- Il pannello c'è mentre è aperto e finché si chiude: tolto dopo, perché
-- anche invisibile prenderebbe le pressioni (il hit-test di Genoma non
-- guarda né il clip né l'opacità).
local panel_out = g.signal(false)
local putting_away: any = nil
g.watch(function() return open.value end, function(on: boolean)
    if putting_away then putting_away:stop() end
    putting_away = nil
    if on then
        panel_out.value = true
    else
        putting_away = g.after(0.3, function()
            putting_away = nil
            panel_out.value = false
        end)
    end
end)

local function button_x(): number return (area.value.w - BUTTON) / 2 end
local function button_y(): number return area.value.h - MARGIN - BUTTON end

-- La cella aperta: nasce dal bottone e sale sopra di lui.
local panel = g.capsule {
    name = "Controlli",
    overlay = true,
    attach = "corner",
    present = function() return panel_out.value and chrome.value end,
    offset_x = function()
        return if open.value then (area.value.w - PANEL_W) / 2 else button_x()
    end,
    offset_y = function()
        return if open.value then button_y() - GAP_UP - panel_h.value else button_y()
    end,
    width = function() return if open.value then PANEL_W else BUTTON end,
    height = function() return if open.value then panel_h.value else BUTTON end,
    radius = function() return if open.value then look.radius_panel else BUTTON / 2 end,
    opacity = function() return if open.value then 1 else 0 end,
    clip = true,
    fill = look.glass,
    rim_top = look.rim,
    rim_bottom = look.line,
    backdrop_blur = 14,
    transition = {
        offset_x = function() return if open.value then "cell_open" else "cell_close" end,
        offset_y = function() return if open.value then "cell_open" else "cell_close" end,
        width = function() return if open.value then "cell_open" else "cell_close" end,
        height = function() return if open.value then "cell_open" else "cell_close" end,
        radius = function() return if open.value then "cell_open" else "cell_close" end,
        opacity = function() return if open.value then "panel_fade" else "cell_close" end,
    },
    -- Le pressioni sul vetro restano al vetro.
    on_press = function() end,
    on_click = function() end,
    g.box {
        overlay = true,
        attach = "corner",
        width = PANEL_W,
        on_size = function(_: number, h: number) panel_h.value = h end,
        content,
    },
}

-- Il bottone: c'è sempre (salvo F11 e H), apre e chiude.
local button = g.capsule {
    id = "controls",
    role = "button",
    name = "Controlli",
    overlay = true,
    attach = "corner",
    present = function() return chrome.value end,
    offset_x = button_x,
    offset_y = button_y,
    width = BUTTON,
    height = BUTTON,
    radius = BUTTON / 2,
    fill = function() return if open.value then g.colour.alpha(look.primary(), 0.16) else look.glass() end,
    rim_top = function() return if open.value then look.primary() else look.rim() end,
    rim_bottom = function() return if open.value then look.primary() else look.line() end,
    backdrop_blur = 14,
    transition = { fill = "ui_change", rim_top = "ui_change", rim_bottom = "ui_change" },
    on_press = function() end,
    on_click = function() open.value = not g.untracked(function() return open.value end) end,
    g.svg {
        overlay = true,
        attach = "corner",
        offset_x = (BUTTON - 18) / 2,
        offset_y = (BUTTON - 18) / 2,
        width = 18,
        height = 18,
        src = g.dir .. "/icons/sinestesia.svg",
        -- Tinto come i glifi di Bioma: il primario, chiaro in alto e scuro in
        -- basso.
        gradient_top = function() return look.gradient_top(look.primary()) end,
        gradient_bottom = function() return look.gradient_bottom(look.primary()) end,
    },
}

-- Una pressione fuori dalla cella aperta la chiude.
local outside = g.box {
    overlay = true,
    attach = "corner",
    present = function() return open.value end,
    width = function() return area.value.w end,
    height = function() return area.value.h end,
    on_click = function() open.value = false end,
}

-- ---- La finestra ----------------------------------------------------------------------

local function key(name: string, text: string?)
    if name == "F11" then
        local on = not g.untracked(function() return whole.value end)
        whole.value = on
        chrome.value = not on
        if on then open.value = false end
    elseif text == "h" or text == "H" then
        chrome.value = not g.untracked(function() return chrome.value end)
        if not chrome.value then open.value = false end
    elseif name == "Escape" then
        if g.untracked(function() return open.value end) then
            open.value = false
        else
            whole.value = false
            chrome.value = true
        end
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
        stage,
        outside,
        panel,
        button,
    },
}
