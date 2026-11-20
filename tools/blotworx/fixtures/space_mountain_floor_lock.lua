-- Run after the original Zone D map-load callbacks.
local panel = assert(GetEntity("sm_tl_zoned.Toon 22"))
assert(GetPropertyBool(panel, "Is Real"), "Zone D intro must lock the floor panel")
assert(GetPropertyBool(panel, "Is Painted"), "Zone D intro must restore the floor panel")
ForceSketched(panel)
assert(GetPropertyBool(panel, "Is Painted"), "Real floor panel accepted an ink change")
Print("PASS Zone D intro restored and locked the floor panel")
