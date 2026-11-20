-- Standalone Zone D replay. Stage the approach to each valve because this
-- fixture exercises the real spin input and IO without recording traversal.
-- Supply dialogue choice 0 every 12 input frames and a spin press every
-- 34 frames starting at frame 480. Run at least 1240 simulation ticks.
-- Standalone test entry has no prior airlock to clean up.
Cleanup_GV_JungleToAsiaAirlock = function() end
Wait(16)
local player = GetPlayer()
EnableAbility_Player(player, "Spin", 1)
local approaches = {
    vector4(-0.85, 0.1, 0),
    vector4(0.85, 0.1, 0),
    vector4(0, 0.1, -0.85),
    vector4(0, 0.1, 0.85)
}
for i = 1, 4 do
    local gear = assert(GetEntity(("gremlinvillage_zoned_dynamic.GV_FanGear0%d"):format(i)))
    local ready = false
    for attempt, offset in ipairs(approaches) do
        SetTransformation(player, GetPosition(gear) + offset, GetFacing(player))
        Print(("ZONE_D_VALVE_%d_APPROACH_%d"):format(i, attempt))
        for poll = 1, 12 do
            Wait(0.2)
            ready = (i == 4 and GetGlobal("GV_ZoneD_FanPuzzleComplete") == 2) or
                    (i < 4 and GV_ZoneD_ValveActive == i + 1)
            if ready then break end
        end
        if ready then break end
    end
    assert(ready, "Spin did not activate Zone D valve " .. i)
    Print("ZONE_D_VALVE_HIT_" .. i)
    Wait(0.4)
end
Wait(10)
Print("ZONE_D_VENT_SEQUENCE_SETTLED")
