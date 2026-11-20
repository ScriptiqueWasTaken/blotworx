-- Run with diagnose --level Levels/GV_ZoneB_Start.level --run-script <this file>.
Wait(1)
local knot = assert(GetEntity("gv_zoneb.SK_BoatPath08"), "Authored boat junction missing")
assert(GetPropertyFloat(knot, "BranchPathChance") == 1, "Unexpected authored branch probability")
assert(select("#", SplineKnotComponent_SetBranchProbability(knot, 0.375)) == 0)
assert(GetPropertyFloat(knot, "BranchPathChance") == 0.375, "Script did not change the branch")
SplineKnotComponent_SetBranchProbability(knot, 1)

GV_ZoneB_BoatPathChange(knot)
assert(GetPropertyFloat(knot, "BranchPathChance") == 0, "Junction did not select the main track")
assert(GV_ZoneB_BoatPathChangeState == 1, "Authored handoff did not finish")
Print("BOAT_PATH_HANDOFF_PASSED")
