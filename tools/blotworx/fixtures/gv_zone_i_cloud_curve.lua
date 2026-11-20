Wait(0.25)
local cloud = assert(GetEntity("gv_zonei_dynamic.GV_ZoneI_Cloud 01"))
local departure = assert(GetEntity("gv_zonei_dynamic.CloudSK 01"))
local arrival = assert(GetEntity("gv_zonei_dynamic.CloudSK 02"))
SetSplineFollowerInitialSpeed(cloud, 0.2)
SplineFollower_TeleportToKnot(cloud, departure)
SplineFollower_SetDisabled(cloud, false)

-- The imported follower enables EaseInOut and neither knot overrides it.
-- After one second of hold, sample 5% of the eight-second edge. The Wii
-- maps that phase to 0.01425. Allow 0.16 world units for up to two ticks
-- of sample scheduling around the departure.
Wait(1.4)
local early = GetPosition(cloud)
Print(("GV_CLOUD_EASED_DEPARTURE %.4f %.4f %.4f"):format(early.r, early.y, early.z))
assert(math.abs(early.r - 113.2617) <= 0.16)
assert(math.abs(early.y - 42.0347) <= 0.16)
assert(math.abs(early.z + 314.5846) <= 0.16)

Wait(3.6)
local halfway = GetPosition(cloud)
Print(("GV_CLOUD_MIDPOINT %.4f %.4f %.4f"):format(halfway.r, halfway.y, halfway.z))
assert(math.abs(halfway.r - 114.6357) <= 0.2)
assert(math.abs(halfway.y - 46.5355) <= 0.2)
assert(math.abs(halfway.z + 298.9699) <= 0.2)

Wait(4.1)
local endpoint = GetPosition(cloud)
local knot = GetPosition(arrival)
assert(endpoint.r == knot.r and endpoint.y == knot.y and endpoint.z == knot.z)
Print("BLOTWORX_GV_CLOUD_CURVE_PASS")
