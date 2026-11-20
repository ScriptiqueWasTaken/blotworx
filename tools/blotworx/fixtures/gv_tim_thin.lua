-- Run in GV_ZoneB_Start with a fresh save. Stage Tim's thinner outcome.
Wait(1)
assert(GetEntity("gv_zoneb_dynamic.Gremlin_Variant2 01"), "Tim is missing")
SetGlobal("GV_ZoneB_Friendlies", 0)
SetGlobal("GV_ZoneB_TotalSpatters", 0)
GV_ZoneB_TimPuzzleCheck()
Print("TIM_DISMISSAL_RETURNED")
