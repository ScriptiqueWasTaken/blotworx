-- Stage Tim's paint outcome with the placed booth Spatter as survivor.
Wait(1)
if type(Cleanup_GV_ZoneA) ~= "function" then
    ActivateScene("Environments/thepark/fantasyLand/SmallWorld/GSA/GV_ZoneA_airlock.gsa")
    WaitForLevelLoad()
end
local spatter = assert(GetEntity("gv_zoneb_dynamic.SpatterMelee 04"))
assert(GetPropertyFloat(spatter, "PatrolSpeed") == 7)
assert(select("#", AI_ClearTarget(spatter)) == 0)
local source = assert(GetEntity("gv_zoneb_dynamic.FX_PipeBurst"))
local spawn = ForceSpawn
local burst
ForceSpawn = function(owner, count)
    local result = spawn(owner, count)
    if owner == "gv_zoneb_dynamic.Spawner_PipeBurst" then
        burst = assert(result[1], "Pipe-burst clone missing")
        assert(burst ~= source and IsValidHandle(burst))
        Print("TIM_PIPE_BURST_SPAWNED")
    end
    return result
end
GV_ZoneB_SpawnerGroup01Enemy_1_Dead = true
GV_ZoneB_SpawnerGroup01Enemy_2_Dead = true
SetGlobal("GV_ZoneB_Friendlies", 1)
SetGlobal("GV_ZoneB_TotalSpatters", 0)
GV_ZoneB_TimPuzzleCheck()
assert(GetPropertyFloat(spatter, "PatrolSpeed") == 3, "Tim's patrol speed was not applied")
assert(AI_Alive == 3, "Tim did not select the staged survivor")
Print("TIM_PAINT_DISMISSAL_RETURNED")
for _ = 1, 120 do
    if burst then break end
    Wait(0.1)
end
assert(burst, "Tim's follow-up did not spawn the pipe burst")
ForceSpawn = spawn
Wait(2.5)
assert(not IsValidHandle(burst), "Pipe-burst clone survived its authored lifetime")
assert(IsValidHandle(source), "The reusable source was retired with its clone")
Print("TIM_PIPE_BURST_LIFECYCLE_PASSED")
