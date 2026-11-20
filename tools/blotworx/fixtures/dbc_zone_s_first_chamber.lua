-- Send combat stimuli through the runtime while leaving every authored
-- OnDamage connection and progression callback enabled.
Wait(1)
local function blister(index)
    return assert(GetEntity(("dbc_zones.HeartTentacleA101_Inert %02d"):format(index)))
end
local function hit(target)
    StimulusEntityWithAliasCalculateDirectionInRadius(GetPlayer(), ST_THINNER, 10, 1000, target, "brush")
end
local function target_value(target)
    return GetPropertyFloat(target, "Target Value")
end
local function invulnerable(target)
    return GetPropertyBool(target, "ForceInvulnerable")
end

assert(target_value(blister(1)) == 1 and invulnerable(blister(1)) == false)
assert(target_value(blister(2)) == 0 and invulnerable(blister(2)) == true)
assert(GetGlobal("DBC_LastHeart01") == 0)
hit(blister(2))
Wait(0.2)
assert(GetGlobal("DBC_LastHeart01") == 0, "a locked blister advanced the fight")

-- At stage zero the first blister's callback would accept the hit. Verify
-- that the live health receiver rejects it while invulnerability is forced.
local first = blister(1)
SetPropertyBool(first, "ForceInvulnerable", true)
assert(invulnerable(first) == true)
hit(first)
Wait(0.2)
assert(GetGlobal("DBC_LastHeart01") == 0, "forced invulnerability failed to stop damage")
SetPropertyBool(first, "ForceInvulnerable", false)
assert(invulnerable(first) == false)

for index = 1, 2 do
    hit(blister(index))
    Wait(0.2)
    assert(GetGlobal("DBC_LastHeart01") == index, "the original damage callback did not advance")
    Wait(2.3)
    assert(target_value(blister(index)) == 0, "the popped blister retained its target value")
    assert(target_value(blister(index + 1)) == 1, "the next blister's target value did not update")
    assert(invulnerable(blister(index + 1)) == false, "the next blister remained invulnerable")
    Print(("BLOTWORX_ZONE_S_BLISTER_%d_UNLOCKED"):format(index + 1))
end

hit(blister(3))
Wait(0.2)
assert(GetGlobal("DBC_LastHeart01") == 3, "the first chamber's special damage callback did not advance")
Wait(2.3)
assert(target_value(blister(3)) == 0)
assert(target_value(blister(4)) == 0 and invulnerable(blister(4)) == true, "the next chamber's gate was bypassed")
Print("BLOTWORX_ZONE_S_FIRST_CHAMBER_CLEARED")
