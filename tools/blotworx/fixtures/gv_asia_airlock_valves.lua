-- Stage the original timed valve callbacks from GV_ZoneC_Start.
Wait(1)
GV_ZoneC_StreamAsiaAirlock()
WaitForLevelLoad()
assert(type(GV_JTA_Camera1GlobalSet) == "function")
GV_JTA_Camera1GlobalSet()
WaitForLevelLoad()
assert(Cleanup_GV_ZoneC2 == nil, "Zone C cleanup did not finish")
Print("ASIA_AIRLOCK_STREAMED")
local pete_name = "gv_jungletoasiaairlock.SmallWorldPete"
local source = assert(GetEntity(pete_name))
local spawned_pete
local force_spawn = ForceSpawn
ForceSpawn = function(owner, count)
    local result = force_spawn(owner, count)
    if owner == "gv_jungletoasiaairlock.Spawner 01" then
        spawned_pete = assert(result[1])
    end
    return result
end
FireThread(GV_JTA_Timer)
Wait(0.25)
assert(TimerGetState() == "Ticking")
GV_AAL_Valve01Function("gv_jungletoasiaairlock.GV_ZoneF_Valve_Inert01")
GV_AAL_Valve02Function("gv_jungletoasiaairlock.GV_ZoneF_Valve_Inert02")
GV_AAL_Valve03Function("gv_jungletoasiaairlock.GV_ZoneF_Valve_Inert03")
Wait(2.5)
assert(GV_JTA_Solution == 3 and GV_JTA_TimerOperation == 3)
Print("ASIA_AIRLOCK_TIMED_VALVES_SOLVED")
ForceSpawn = force_spawn
assert(spawned_pete and spawned_pete ~= source, "Pete was not cloned")
assert(GetEntity(pete_name) == spawned_pete, "Pete's source shadows the spawned actor")
assert(GetEntityGroupSize(pete_name) == 1, "Pete's group must count one live actor")
local members = {}
ForEachEntityInGroup(function(actor)
    members[#members + 1] = actor
end, pete_name)
assert(#members == 1 and members[1] == spawned_pete, "Pete's group must visit the live actor")
Print("ASIA_AIRLOCK_PETE_LIVE_LOOKUP")
local patrol_node = assert(GetEntity("gv_jungletoasiaairlock.PatrolNode 02"))
local distance = GetDistanceBetweenEntities(spawned_pete, patrol_node)
Print("ASIA_AIRLOCK_PETE_PATROL_DISTANCE " .. tostring(distance))
Wait(4)
if GetEntity(pete_name) == spawned_pete then
    Print("ASIA_AIRLOCK_PETE_PATROL_DISTANCE " .. tostring(GetDistanceBetweenEntities(spawned_pete, patrol_node)))
end
