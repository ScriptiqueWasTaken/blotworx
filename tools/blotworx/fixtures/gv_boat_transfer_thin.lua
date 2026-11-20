-- Load the boat-transfer airlock from GV_ZoneB_Start, then thin the three targets.
Wait(1)
ActivateScene("Environments/thepark/fantasyLand/SmallWorld/GSA/GV_ZoneA_airlock.gsa")
ActivateScene("Environments/thepark/fantasyLand/SmallWorld/GSA/GV_boattransfer01.gsa")
StreamInterior(nil, "stream_boatairlock")
WaitForLevelLoad()
local bandages = {
    "GV_ZoneC_PipeBandage_Toon3 01",
    "GV_ZoneC_PipeBandage_Toon2 01",
    "GV_ZoneC_PipeBandage_Toon 01",
}
for index, name in ipairs(bandages) do
    local bandage = assert(GetEntity("gv_boattransfer01." .. name))
    local flag = string.format("GV_BT_Gear%02d", index)
    assert(_G[flag] == 1, "Gear started released: " .. flag)
    ForceThinned(bandage)
    Wait(0.3)
    assert(_G[flag] == 0, "Thinning the bandage did not release " .. flag)
end
Wait(10)
assert(GetGlobal("GV_BT_GearDoorKey") == 2 and GetGlobal("GV_BT_BTDoorOpen") == 2,
    "The original gear callbacks did not open the transfer door")
Print("AUTHORED_THINNER_GEAR_DOOR_OPENED")

-- Place Mickey inside the authored trigger and let its contact, BSQ, and
-- camera completion events take the normal path into Zone C.
local player = GetPlayer()
local trigger = assert(GetEntity("gv_boattransfer01.Trigger 05"))
SetTransformation(player, GetPosition(trigger), GetFacing(player))
Wait(18)
assert(Cleanup_GV_ZoneB2 == nil, "Zone B cleanup did not finish after entering the exit trigger")
assert(GetEntity("gv_zonec_dynamic.GV_ZoneC_EntranceDoorA1 01"), "Zone C entrance door unavailable")
Print("AUTHORED_BOAT_TRANSFER_ZONE_C_REACHED")
