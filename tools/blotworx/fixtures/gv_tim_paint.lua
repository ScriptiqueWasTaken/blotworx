-- Stage Tim's paint outcome with the placed booth Spatter as survivor.
Wait(1)
if type(Cleanup_GV_ZoneA) ~= "function" then
    ActivateScene("Environments/thepark/fantasyLand/SmallWorld/GSA/GV_ZoneA_airlock.gsa")
    WaitForLevelLoad()
end
local spatter = assert(GetEntity("gv_zoneb_dynamic.SpatterMelee 04"))
assert(GetPropertyFloat(spatter, "PatrolSpeed") == 7)
assert(select("#", AI_ClearTarget(spatter)) == 0)
GV_ZoneB_SpawnerGroup01Enemy_1_Dead = true
GV_ZoneB_SpawnerGroup01Enemy_2_Dead = true
SetGlobal("GV_ZoneB_Friendlies", 1)
SetGlobal("GV_ZoneB_TotalSpatters", 0)
GV_ZoneB_TimPuzzleCheck()
assert(GetPropertyFloat(spatter, "PatrolSpeed") == 3, "Tim's patrol speed was not applied")
assert(AI_Alive == 3, "Tim did not select the staged survivor")
Print("TIM_PAINT_DISMISSAL_RETURNED")
