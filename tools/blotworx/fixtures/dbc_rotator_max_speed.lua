Wait(0.5)
local entityName = "dbc_1st_zonea_dynamic.Robot_Arm"
local arm = assert(GetEntity(entityName))
local speed = assert(GetPropertyFloat(entityName, "Max Speed"))

assert(select("#", SetRotatorMaxSpeed(arm, 75)) == 0)
assert(GetPropertyFloat(entityName, "Max Speed") == 75)
assert(select("#", SetRotatorMaxSpeed(nil, 10)) == 0)
assert(GetPropertyFloat(entityName, "Max Speed") == 75)
assert(select("#", SetRotatorMaxSpeed(entityName, speed)) == 0)
assert(GetPropertyFloat(entityName, "Max Speed") == speed)
Print("BLOTWORX_ROTATOR_SPEED_PASS", speed)
