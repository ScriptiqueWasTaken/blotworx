-- Run from GV_ZoneC_Start with a fresh save and dialogue advance inputs.
-- The authored landmark places Mickey above the part of the river where
-- the controller previously stood without a rigid contact damage callback.
Wait(4)
local player = GetPlayer()
local river = GetEntity("gv_zonec_audio.Sound_Marker_ThinnerRiver_1 20")
assert(river and river ~= 0, "Zone C river landmark is missing")
local before = GetPropertyInt(player, "Health", 0)
SetTransformation(player, GetPosition(river) + vector4(0, 1.3, 0), GetFacing(player))
Wait(0.8)
assert(GetPropertyInt(player, "Health", 0) == before - 1, "River did not apply one point of damage")
Print("ZONE_C_RIVER_SUPPORT_DAMAGED_PLAYER")
