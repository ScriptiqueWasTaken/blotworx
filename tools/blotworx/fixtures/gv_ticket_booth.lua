-- Append after the Zone A airlock replay has crossed Zone B's entry trigger.
local tim = assert(GetEntity("gv_zoneb_dynamic.Gremlin_Variant2 01"))
local booth = GetPosition(tim)
assert(math.abs(booth[1] - 159.7) < 0.15 and math.abs(booth[2] - 7.2) < 0.15
    and math.abs(booth[3] + 9.9) < 0.15 and not IsHidden(tim),
    "Tim did not stay at his authored booth position")

-- Wait for the original intro to enable its spawners. Group iteration skips
-- the unplaced factory sources that share the live enemies' scene names.
local enemies = {}
for _ = 1, 200 do
    enemies = {}
    ForEachEntityInGroup(function(enemy) enemies[#enemies + 1] = enemy end, "GV_ZoneB_Enemies")
    if #enemies == 5 then break end
    Wait(0.1)
end
assert(#enemies == 5, "Zone B did not spawn all five booth enemies")
assert(GetGlobal("GV_ZoneB_TotalSpatters") == 5, "Zone B started with the wrong combat count")
for index, enemy in ipairs(enemies) do
    assert(GetCurrentTeam(enemy) == 2, "A booth enemy did not start on the hostile team")
    ForceThinned(enemy)
    Wait(0.2)
    assert(not IsAlive(enemy), "The booth enemy survived its thinner threshold")
    assert(GetGlobal("GV_ZoneB_TotalSpatters") == 5 - index,
        "The original booth callback did not count a defeat")
end
assert(PlayerReadTimDismisall, "The original all-thinner dismissal did not run")
Print("BOOTH_ALL_THINNER_PROGRESSION_PASSED")
