Wait(0.25)
local cloud = assert(GetEntity("gv_zonei_dynamic.GV_ZoneI_Cloud 04"))
local departure = assert(GetEntity("gv_zonei_dynamic.CloudSK 07"))
SplineFollower_TeleportToKnot(cloud, departure)
SplineFollower_SetDisabled(cloud, false)

-- This untimed edge is about 14.4235 world units along the cubic. The
-- authored 2 units/s covers half this edge in about 3.606 seconds.
Wait(7.6)
local p = GetPosition(cloud)
Print(("GV_CLOUD_UNTIMED_MIDPOINT %.4f %.4f %.4f"):format(p.r, p.y, p.z))
assert(math.abs(p.r - 139.2271) <= 0.15)
assert(math.abs(p.y - 47.3935) <= 0.15)
assert(math.abs(p.z + 268.1928) <= 0.15)
-- Speed is live. Another half-second at 4 units/s must continue from the
-- same curve phase, with a tolerance for two ticks of sample scheduling.
SetSplineFollowerInitialSpeed(cloud, 4)
Wait(0.5)
local accelerated = GetPosition(cloud)
Print(("GV_CLOUD_UNTIMED_ACCELERATED %.4f %.4f %.4f"):format(accelerated.r, accelerated.y, accelerated.z))
assert(math.abs(accelerated.r - 137.0337) <= 0.24)
assert(math.abs(accelerated.y - 47.1542) <= 0.24)
assert(math.abs(accelerated.z + 268.2238) <= 0.24)
Print("BLOTWORX_GV_CLOUD_UNTIMED_PASS")
