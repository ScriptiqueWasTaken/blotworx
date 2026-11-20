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
-- The authored conversation starts Pete's departure and eventually retires him.
-- The replay advances dialogue while leaving Pete's movement to the engine.
Wait(12)
assert(GetEntityGroupSize(pete_name) == 0, "Pete did not complete his patrol and departure")
Print("ASIA_AIRLOCK_PETE_DEPARTED")

-- Stage Mickey at the authored Zone D entrance to check the trigger and cleanup.
local player = GetPlayer()
local gate = assert(GetEntity("gremlinvillage_zoned.Trigger_PeteIntro"))
local contact = GetPosition(gate) - vector4(0, 2.5, 0)
local facing = GetFacing(player)
SetTransformation(player, contact + vector4(0, 0, 2), facing)
Wait(0.2)
SetTransformation(player, contact, facing)
Wait(4)
assert(GetGlobal("GV_Pete_Intro_Played") == 1, "Zone D entrance trigger did not fire")
assert(Cleanup_GV_JungleToAsiaAirlock == nil, "Zone D kept the airlock's scripts alive")
Print("ASIA_AIRLOCK_EXIT_TO_ZONE_D")
