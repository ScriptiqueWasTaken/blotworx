-- Pair with the recorded Zone D inputs to advance the original conversations.
Wait(15.3)
local player = GetPlayer()
local gate = assert(GetEntity("sm_tl_zoned_gameplay.PlaneTrigger 01"))
SetTransformation(player, GetPosition(gate) - GetFacing(gate) * 1.3 + vector4(0, 0.3, 0), GetFacing(gate))

Wait(34)
local health = GetHealth(player)
assert(health == 5, "The rotated floor hazard damaged Mickey on the approach platform")
Print("ZONE D approach platform health intact")

local hazard = assert(GetEntity("sm_tl_zoned.TriggerDamageVolume 01"))
SetTransformation(player, GetPosition(hazard) - vector4(0, 0.4, 0), GetFacing(player))
Wait(0.25)
assert(GetHealth(player) < health, "Contact with the floor hazard did not damage Mickey")
Print("ZONE D rotated floor hazard applied damage on contact")
