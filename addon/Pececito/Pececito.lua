-- Three squares in the top-left corner of the screen, which the Pececito desktop app reads.
-- Magenta and cyan mark the strip; the middle square is green while the fishing channel runs,
-- yellow during any other cast (such as applying a lure), red with no fishing pole in hand, and
-- blue otherwise. The app looks for these exact colors, so keep them in sync with it.

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
local function cell(red, green, blue)
    local texture = strip:CreateTexture(nil, "OVERLAY")
    texture:SetColorTexture(red, green, blue, 1)
    cells[#cells + 1] = texture
    return texture
end

cell(1, 0, 1)
local state = cell(0, 0, 1)
cell(0, 1, 1)

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
        state:SetColorTexture(0, 1, 0, 1)
    elseif isCasting() then
        state:SetColorTexture(1, 1, 0, 1)
    elseif not holdsFishingPole() then
        state:SetColorTexture(1, 0, 0, 1)
    else
        state:SetColorTexture(0, 0, 1, 1)
    end
end)
