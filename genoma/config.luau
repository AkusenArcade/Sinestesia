--!strict
-- Le impostazioni di Sinestesia: ~/.config/sinestesia/config.json.
--
-- Al primo avvio della versione Genoma, se il JSON non c'è ma c'è il
-- config.toml della versione GTK, se ne leggono le poche chiavi a mano
-- (righe `chiave = valore`, tabelle [color_a] e [color_b]) e si scrive il
-- JSON: le impostazioni passano da una versione all'altra.
local g = require("@genoma")

local M = {}

local dir = (g.env("XDG_CONFIG_HOME") or ((g.env("HOME") or "") .. "/.config")) .. "/sinestesia"
M.path = dir .. "/config.json"
local toml_path = dir .. "/config.toml"

export type Settings = {
    effect: string,
    source: string,
    color_mode: string,
    color_a: string,
    color_b: string,
    gain: number,
    blur: number,
}

-- Gli stessi della versione GTK (src/config.rs).
M.defaults = {
    effect = "bars",
    source = "output",
    color_mode = "auto",
    color_a = "#d6baff",
    color_b = "#f2b8c4",
    gain = 1.0,
    blur = 0.0,
}

local function hex(r: number, gr: number, b: number): string
    local function c(v: number): number return math.clamp(math.floor(v * 255 + 0.5), 0, 255) end
    return string.format("#%02x%02x%02x", c(r), c(gr), c(b))
end

-- Il TOML della versione GTK, quanto basta: nient'altro lo scrive.
local function from_toml(text: string): { [string]: any }
    local out: { [string]: any } = {}
    local section = ""
    local colours: { [string]: { [string]: number } } = {}
    for line in string.gmatch(text, "[^\n]+") do
        local s = string.match(line, "^%s*%[([%w_]+)%]%s*$")
        if s then
            section = s
            colours[s] = colours[s] or {}
        else
            local key, value = string.match(line, "^%s*([%w_]+)%s*=%s*(.-)%s*$")
            if key and value then
                local unquoted = string.match(value, '^"(.*)"$')
                if section == "" then
                    out[key] = if unquoted then unquoted else tonumber(value)
                elseif colours[section] then
                    colours[section][key] = tonumber(value) or 0
                end
            end
        end
    end
    for _, name in { "color_a", "color_b" } do
        local c = colours[name]
        if c and c.r and c.g and c.b then out[name] = hex(c.r, c.g, c.b) end
    end
    return out
end

local file = g.file { path = M.path, watch = false }

-- Le impostazioni lette ora: il JSON, o il TOML migrato, o i default.
function M.load(): Settings
    local data: { [string]: any } = {}
    if file.text then
        local ok, parsed = pcall(g.parse_json, file.text)
        if ok and type(parsed) == "table" then data = parsed end
    else
        local toml = g.file { path = toml_path, watch = false }
        if toml.text then
            data = from_toml(toml.text)
            file:write(g.to_json(data, true))
        end
    end
    local out = table.clone(M.defaults) :: any
    for k, v in data do
        if out[k] ~= nil and type(v) == type(out[k]) then out[k] = v end
    end
    out.gain = math.clamp(out.gain, 0.1, 10)
    out.blur = math.clamp(out.blur, 0, 0.95)
    return out
end

-- Scritte tutte insieme, intere.
function M.save(settings: Settings)
    file:write(g.to_json(settings, true))
end

M._from_toml = from_toml

return M
