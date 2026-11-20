StreamZones(GetPlayer(), "stream_3")
WaitForLevelLoad()
Wait(1)

local arm = assert(GetEntity("dbc_scaletest.DBC_HandTrebuchetA1 01"))
local prefab = "dbc_scaletest.dbc_gremlin_clean_v2 01.dbc_gremlin_clean_v2."
local cage = assert(GetEntity(prefab .. "Gremlin_CageBreakable01"))
local calvin = assert(GetEntity(prefab .. "Caged_Gremlin01"))
local initial = GetPosition(cage)
local distance = (GetPosition(calvin) - initial).l
assert(distance < 1.2, string.format("Calvin starts %.3f units from the cage", distance))

AnimGBSequence(arm, "Drop1")
local travel = 0
for i = 1, 12 do
    Wait(0.15)
    local position = GetPosition(cage)
    travel = math.max(travel, (position - initial).l)
    local gap = (GetPosition(calvin) - position).l
    assert(math.abs(gap - distance) < 0.05,
        string.format("Calvin's cage offset changed from %.3f to %.3f on sample %d", distance, gap, i))
end
assert(travel > 0.25, "Cage did not follow the trebuchet arm")
Print(string.format("COURTYARD cage traveled %.3f with Calvin inside", travel))

-- The authored rescue sequence clears this parent before enabling motion.
ClearParent(calvin)
local detached = GetPosition(calvin)
AnimGBSequence(arm, "Rest")
Wait(1)
assert((GetPosition(calvin) - detached).l < 0.03,
    "Calvin continued to follow the cage after detaching")
Print("COURTYARD Calvin detached from the cage")
