-- Start Levels/2D_ClockCleaners01.level. Press Use at tick 306, then
-- hold movement starting at tick 722. Capture at tick 1020.
Wait(10)
local projector = assert(GetEntity("sw_clocka_shared.seq_projector 02"))
local stand = assert(GetRelativePrefabEntity(projector, ".ExitProjectorStand"))
assert(IsValidHandle(stand) and GetPropertyFloat(stand, "UsableRadius") > 0)
SetTransformation(GetPlayer(), GetPosition(stand), GetFacing(GetPlayer()))

-- Stage a revisit while retaining the authored portal, camera, and streaming.
local enter = assert(GV_BigBen_IntroStart)
GV_BigBen_IntroStart = function(...)
    GV_BigBen_Intro = 1
    return enter(...)
end
Print("CLOCK_RETURN_STAGED")
Wait(16)
local arrival = assert(GetEntity("GV_ZoneF_BigBenExitMarker"))
assert(IsValidHandle(arrival), "Zone F did not stream in")
Print("CLOCK_RETURN_MARKER_RESOLVED")
