-- Stage the original disabled mover without advancing the rest of the encounter.
Wait(1.25)
local disc = assert(GetEntity("dbc_zones.MoverDisc01_Inert 01"))
local origin = GetPosition(disc).y
local top = GetPosition(assert(GetEntity("dbc_zones.Mover19_SplineKnot 01"))).y
local height = top - origin
assert(height > 4)

-- This speed would take over 20 seconds to travel the edge without its authored time.
SetSplineFollowerInitialSpeed(disc, 0.2)
SplineFollower_SetDisabled(disc, false)

local function sample(label, fraction, tolerance)
    local actual = (GetPosition(disc).y - origin) / height
    assert(math.abs(actual - fraction) <= tolerance, ("%s phase %.4f"):format(label, actual))
    Print(("ZONE_S_SPLINE_%s %.4f"):format(label, actual))
end

-- Startup hold plus departure knot hold, then one second of upward travel.
Wait(1.9)
sample("HELD", 0, 0.01)
Wait(0.8)
sample("OUTBOUND_MIDPOINT", 0.5, 0.08)
Wait(0.6)
sample("TOP", 1, 0.01)

-- The other knot holds for 1.7 seconds and its outbound edge takes three seconds.
Wait(3.1)
sample("RETURN_MIDPOINT", 0.5, 0.07)
Wait(1.7)
sample("BOTTOM", 0, 0.01)
Print("BLOTWORX_ZONE_S_SPLINE_TIMING_PASS")
