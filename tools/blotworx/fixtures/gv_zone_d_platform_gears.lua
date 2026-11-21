-- Start Levels/GV_ZoneD_Start.level and acknowledge dialogue every eight
-- ticks. ForcePainted runs the same body transition as contact with paint.
Cleanup_GV_JungleToAsiaAirlock = function() end
Wait(16)
local gears = {}
for i = 1, 5 do
    local gear = assert(GetEntity("GV_ZoneD_GearMachine" .. i))
    assert(IsValidHandle(gear))
    gears[i] = gear
    ForceThinned(gear)
end
Wait(1.5)
local deliver = GV_ZoneD_GearMachineGroup
local painted = { Trio = 0, Duo = 0 }
GV_ZoneD_GearMachineGroup = function(entity, group, mode)
    if mode == "Painted" then
        painted[group] = painted[group] + 1
    end
    return deliver(entity, group, mode)
end
for _, gear in ipairs(gears) do
    ForcePainted(gear)
    Wait(1.5)
    assert(GetPropertyBool(gear, "Is Painted"), "The gear body was not painted")
end
Wait(4)
assert(painted.Trio == 3 and painted.Duo == 2,
    ("Expected three Trio and two Duo completions, got %d and %d"):format(painted.Trio, painted.Duo))
assert(GV_ZoneD_DragonDoorAnim == "opening", "The five gears did not open the dragon door")
local trio_left, trio_up = GetLeft(gears[1]), GetUp(gears[1])
local duo_left, duo_up = GetLeft(gears[4]), GetUp(gears[4])
Wait(0.55)
assert((GetLeft(gears[1]) - trio_left):length2() + (GetUp(gears[1]) - trio_up):length2() > 0.0001,
    "The painted Trio platform did not rotate")
assert((GetLeft(gears[4]) - duo_left):length2() + (GetUp(gears[4]) - duo_up):length2() > 0.0001,
    "The painted Duo platform did not rotate")
Print("ZONE_D_PLATFORM_GEARS_COMPLETED")
