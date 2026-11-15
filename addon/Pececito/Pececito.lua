-- A strip of squares in the top-left corner of the screen, which the Pececito desktop app reads.
-- Each square is a cell carrying three bits as a color: red is 4, green is 2, blue is 1, each on
-- or off. The strip opens with a magenta and a cyan cell, then holds numbers written across one
-- or more cells, most significant first: the addon version, the fields below, and a checksum.
-- The app reads the same layout, so change both together and give the addon a new version.

-- The "## Version" in Pececito.toc.
local VERSION = 2
local MAGENTA = 5
local CYAN = 3
local VERSION_CELLS = 2
local STATE_CELLS = 1
local CHECKSUM_CELLS = 2
local STATE_IDLE = 0
local STATE_NO_POLE = 1
local STATE_BUSY = 2
local STATE_FISHING = 3

local FISHING_SPELLS = { [7620] = true, [131474] = true, [131476] = true }
local MAIN_HAND_SLOT = 16
local WEAPON_CLASS = 2
local FISHING_POLE_SUBCLASS = 20
-- Each square's side in screen pixels.
local CELL_PIXELS = 3
local UPDATE_INTERVAL = 0.05

-- On WorldFrame, not UIParent: hiding the UI (Alt+Z, or addons that hide it while fishing)
-- hides UIParent and everything on it.
local strip = CreateFrame("Frame", "PececitoSignal", WorldFrame)
strip:SetFrameStrata("TOOLTIP")
strip:SetIgnoreParentScale(true)
strip:SetPoint("TOPLEFT", WorldFrame, "TOPLEFT", 0, 0)

local cells = {}
for index = 1, 2 + VERSION_CELLS + STATE_CELLS + CHECKSUM_CELLS do
    cells[index] = strip:CreateTexture(nil, "OVERLAY")
end

local function paint(index, value)
    local red = value >= 4 and 1 or 0
    local green = value % 4 >= 2 and 1 or 0
    cells[index]:SetColorTexture(red, green, value % 2, 1)
end

-- The cells after the two markers.
local payload = {}
local function put(value, count)
    for place = count - 1, 0, -1 do
        payload[#payload + 1] = math.floor(value / 8 ^ place) % 8
    end
end

local function show(state)
    for index = #payload, 1, -1 do
        payload[index] = nil
    end
    put(VERSION, VERSION_CELLS)
    put(state, STATE_CELLS)
    -- Each cell times its place, so swapped cells don't pass.
    local sum = 0
    for place, value in ipairs(payload) do
        sum = sum + place * value
    end
    put(sum % 8 ^ CHECKSUM_CELLS, CHECKSUM_CELLS)
    paint(1, MAGENTA)
    paint(2, CYAN)
    for index, value in ipairs(payload) do
        paint(index + 2, value)
    end
end

-- At scale 1 the screen is 768 units tall, so a pixel is 768 / height units.
local function layout()
    local _, height = GetPhysicalScreenSize()
    local size = CELL_PIXELS * 768 / height
    strip:SetSize(size * #cells, size)
    for index, texture in ipairs(cells) do
        texture:SetSize(size, size)
        texture:ClearAllPoints()
        texture:SetPoint("TOPLEFT", strip, "TOPLEFT", (index - 1) * size, 0)
    end
end

layout()
show(STATE_IDLE)
strip:RegisterEvent("DISPLAY_SIZE_CHANGED")
strip:RegisterEvent("UI_SCALE_CHANGED")
strip:SetScript("OnEvent", layout)

local function fishingName()
    if C_Spell and C_Spell.GetSpellName then
        return C_Spell.GetSpellName(7620)
    end
    if GetSpellInfo then
        return (GetSpellInfo(7620))
    end
    return nil
end

local function isFishing()
    if not UnitChannelInfo then
        return false
    end
    local ok, name, _, _, _, _, _, _, spellID = pcall(UnitChannelInfo, "player")
    if not ok or not name then
        return false
    end
    return FISHING_SPELLS[spellID] == true or name == fishingName()
end

local function isCasting()
    if not UnitCastingInfo then
        return false
    end
    local ok, name = pcall(UnitCastingInfo, "player")
    return ok and name ~= nil
end

-- Item data can be missing right after login; an unknown item counts as a pole, so the app
-- isn't told to stop for a pole that is there.
local function holdsFishingPole()
    local itemID = GetInventoryItemID and GetInventoryItemID("player", MAIN_HAND_SLOT)
    if not itemID then
        return false
    end
    local query = (C_Item and C_Item.GetItemInfoInstant) or GetItemInfoInstant
    if not query then
        return true
    end
    local ok, _, _, _, _, _, classID, subclassID = pcall(query, itemID)
    if not ok or classID == nil then
        return true
    end
    return classID == WEAPON_CLASS and subclassID == FISHING_POLE_SUBCLASS
end

local sinceUpdate = 0
strip:SetScript("OnUpdate", function(_, elapsed)
    sinceUpdate = sinceUpdate + elapsed
    if sinceUpdate < UPDATE_INTERVAL then
        return
    end
    sinceUpdate = 0
    if isFishing() then
        show(STATE_FISHING)
    elseif isCasting() then
        show(STATE_BUSY)
    elseif not holdsFishingPole() then
        show(STATE_NO_POLE)
    else
        show(STATE_IDLE)
    end
end)
