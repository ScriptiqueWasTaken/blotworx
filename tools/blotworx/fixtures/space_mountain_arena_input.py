"""Emit the inputs for the scripted Zone D arena hazard replay."""

import json

frames = [{} for _ in range(1560)]
for tick in range(175, 950, 25):
    frames[tick]["dialogue_choice"] = 0
for tick in range(461, 489):
    frames[tick]["input"] = {"movement": [-0.25, -0.96]}
for tick in range(979, len(frames), 30):
    frames[tick]["dialogue_choice"] = 0

print(json.dumps({"level": "Levels/TL_SpaceMountain_TronPete.Level", "frames": frames}))
