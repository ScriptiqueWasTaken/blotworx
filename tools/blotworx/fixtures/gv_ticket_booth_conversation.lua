-- Start Levels/GV_ZoneB_Start.level. Press Use at tick 375 and acknowledge
-- dialogue every eight ticks. Stage the booth after the guardian tutorial.
Cleanup_GV_ZoneA = function() end
Wait(12)
local tim = assert(GetEntity("gv_zoneb_dynamic.Gremlin_Variant2 01"))
assert(IsValidHandle(tim))
SetGlobal("GV_ZoneB_TotalSpatters", 0)
SetGlobal("GV_ZoneB_OpenDoors_Played", 1)
SetGlobal("GV_ZoneB_ThanksFull_Played", 0)
SetGlobal("GV_ZoneB_Wrench_Played", 0)
SetGlobal("GV_ZoneB_Wrench", 0)
SetGlobal("GV_ZoneB_PumpFilled", 0)
SetGlobal("GV_ZoneB_Solution_Played", 1)
local marker = assert(GetEntity("gv_zoneb.Position_MickeyAtBooth"))
MoveToEntity(GetPlayer(), marker, false, true)
UnrestrictCutSceneInput()
Wait(12)
assert(GetGlobal("GV_ZoneB_Wrench_Played") == 1, "The chained Wrench conversation did not record its Played flag")
Print("TIM_WRENCH_PLAYED")
