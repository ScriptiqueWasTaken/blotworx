-- Run after the ticket-booth thinner outcome while accepting original dialogue.
Wait(12)
local player = GetPlayer()
local entry = GetPosition("gv_boattransfer01.Trigger 03")
local target, nearest
for _, candidate in ipairs({"gv_zoneb.GV_ZoneB_Boat", "gv_zoneb.GV_ZoneB_Boat2", "gv_zoneb.GV_ZoneB_Boat3"}) do
    local position = GetPosition(candidate)
    local distance = (position - entry).l
    if position[2] > 4 and position[1] < entry[1] and (nearest == nil or distance < nearest) then
        target, nearest = candidate, distance
    end
end
assert(target, "No boat was available before the Zone B junction")
SetTransformation(player, GetPosition(target) + vector4(0, 1.2, 0), GetFacing(player))
for _ = 1, 30 do
    Wait(1)
    local position = GetPosition(player)
    assert((position - GetPosition(target)).l < 1.5, "Mickey fell off the moving boat")
    if position[1] > 205 then
        assert(GV_ZoneB_BoatPathChangeState > 0, "The rider crossed the junction without triggering its route change")
        assert(GetPropertyFloat("gv_zoneb.SK_BoatPath08", "BranchPathChance") == 0,
            "The authored branching route did not change")
        Print("RIDE_PHYSICALLY_REACHED_TRANSFER")
        return
    end
end
error("The boat did not reach the transfer stretch")
