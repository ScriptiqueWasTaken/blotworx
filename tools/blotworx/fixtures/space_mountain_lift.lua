-- Enter the authored plate fixture without replaying the preceding puzzles.
-- Plate I/O, the movie, the story sequence, and GrabCamera execute normally.
local lift = assert(GetEntity("sm_tl_zonebvisit1only.TL_SM_Gear1 03"))
local function height(entity)
    return GetPosition(entity).y
end

Print(("LIFT start %.4f"):format(height(lift)))
Wait(0.15)
local player = GetPlayer()
SetTransformation(player, GetPosition(lift) + vector4(0, 0.5, 0), GetFacing(player))
for sample = 1, 18 do
    Wait(0.5)
    Print(("LIFT ride %d %.4f player %.4f"):format(sample, height(lift), height(player)))
end
