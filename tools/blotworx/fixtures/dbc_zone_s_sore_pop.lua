-- Play the first placed blister's original clip through the game actor API.
-- This isolates the particle regression from combat and boss progression.
Wait(1)
local sore = GetEntity("dbc_zones.HeartTentacleA101_Inert 01")
assert(sore ~= 0, "missing placed blister")
AnimGBSequence(sore, "Pop")
