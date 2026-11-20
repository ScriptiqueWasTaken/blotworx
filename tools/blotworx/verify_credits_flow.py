"""Replay the two EM1 credits paths against imported game data without skip input."""
import argparse
from concurrent.futures import ThreadPoolExecutor
from hashlib import sha256
import json
from math import ceil
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile


def reel_seconds(data, filename):
    with (data / "Movies" / filename).open("rb") as source:
        header = source.read(36)
    if len(header) != 36 or header[:4] != b"BIKi":
        raise ValueError(f"Expected Wii BIKi header in {filename}")
    frames = struct.unpack_from("<I", header, 8)[0]
    numerator, denominator = struct.unpack_from("<II", header, 28)
    return frames * denominator / numerator


def frontend_frames(ticks):
    frames = [{} for _ in range(ticks + 1)]
    # The startup movies precede the title gate. Their input ends before the
    # Credits card is selected, leaving the complete credit reel unskipped.
    for frame in frames[:220]:
        frame["movie_skip_held"] = True
    for frame in frames[175:216]:
        frame["interact_pressed"] = True
    for start, end, aim in [
        (330, 376, [0.63, 0.69]),
        (495, 531, [0.54, 0.35]),
        (615, 638, [0.67, 0.62]),
    ]:
        for frame in frames[start:end]:
            frame["aim"] = aim
    for start, end in [(345, 365), (505, 525), (619, 620)]:
        for frame in frames[start:end]:
            frame["interact_pressed"] = True
    return frames


def digest(path):
    value = sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def capture(engine, data, run, level, frames, tick, budget, lua=None):
    source = run / "input.json"
    source.write_text(json.dumps({"level": level, "frames": frames}), encoding="utf-8")
    command = [
        str(engine), "diagnose", "--data", str(data), "--level", level,
        "--capture", str(run / "capture.png"), "--capture-tick", str(tick),
        "--save-dir", str(run / "saves"), "--input-playback", str(source),
    ]
    if lua:
        script = run / "sequence.lua"
        script.write_text(lua, encoding="utf-8")
        command.extend(["--run-script", str(script)])
    startup = None
    if os.name == "nt":
        startup = subprocess.STARTUPINFO()
        startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
        startup.wShowWindow = subprocess.SW_HIDE
    with (run / "stdout.log").open("wb") as stdout, (run / "stderr.log").open("wb") as stderr:
        subprocess.run(
            command, startupinfo=startup, stdout=stdout, stderr=stderr,
            timeout=budget, check=True,
        )
    report = json.loads((run / "capture.runtime.json").read_bytes())
    require(
        report["runtime_error"] is None and report["script_errors"] == [],
        f"Runtime error {report['runtime_error']}, script errors {report['script_errors']}",
    )
    return report


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def reached_last_frame(movie, media, audio_key):
    require(movie["completed"] and not movie["skipped"], "Movie did not complete naturally")
    require(movie["frame"] == movie["header"]["frames"] - 1, "Last video frame was not presented")
    require(movie[audio_key]["media_id"] == media, f"Wrong Wwise media for {audio_key}")
    require(movie["error"] is None, movie["error"])


def validate_extras(report):
    menu = report["main_menu"]
    extras = menu["extras"]
    require(
        ["ExtrasMenuLoad", "Slot1"] in menu["commands"] and ["Extras", "Credits"] in menu["commands"],
        "The authored menu did not start Credits from slot 1",
    )
    require(extras["page"] == "Landing" and extras["slot"] == 0, "Credits did not return to the same Extras slot")
    require(
        menu["background"]["request"]["filename"] == "FE_Extras_Loop.bik",
        "The Extras background did not resume",
    )
    reached_last_frame(extras["last_credits"], 448639764, "credits_audio")


def validate_campaign(report):
    requests = report["movie_requests"]
    require(
        requests["submitted"] == requests["completed"] == 2
        and requests["last_completed"] == "Movies/fmv_teaser.bik",
        "The campaign did not complete Credits and then the teaser",
    )
    reached_last_frame(report["cinematic"], 411479065, "teaser_audio")
    require(report["title_screen"]["shown"], "The interactive title did not return")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("engine", type=Path)
    parser.add_argument("data", type=Path, help="Imported EM1 DATA/files directory")
    parser.add_argument("save", type=Path, help="A GAME v16 checkpoint to copy into the isolated Extras slot")
    parser.add_argument("--output", type=Path, default=Path("target/perf"))
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    engine, data = args.engine.resolve(strict=True), args.data.resolve(strict=True)
    credits = reel_seconds(data, "fmv_credits.bik")
    teaser = reel_seconds(data, "fmv_teaser.bik")
    output = Path(tempfile.mkdtemp(prefix="credits-natural-", dir=args.output.resolve()))
    extras = output / "extras"
    campaign = output / "campaign"
    (extras / "saves").mkdir(parents=True)
    campaign.mkdir()
    slot = extras / "saves" / "save0.dat"
    shutil.copyfile(args.save, slot)
    before = digest(slot)
    identity = {
        "executable_sha256": digest(engine),
        "initial_save_sha256": before,
        "movies_sha256": {
            name: digest(data / "Movies" / name)
            for name in ("fmv_credits.bik", "fmv_teaser.bik")
        },
    }
    (output / "identity.json").write_text(json.dumps(identity, indent=2) + "\n", encoding="utf-8")
    tick = ceil((credits + 28) * 30)
    print(f"Running both natural-completion replays in {output}", flush=True)
    with ThreadPoolExecutor(max_workers=2) as pool:
        extra_job = pool.submit(
            capture, engine, data, extras, "Levels/Main_Menu.level", frontend_frames(tick),
            tick, credits + 120,
        )
        campaign_job = pool.submit(
            capture, engine, data, campaign, "Levels/EB_VillageTest.level", [{} for _ in range(340)],
            240, credits + teaser + 120,
            'Wait(.12); StartSequence(GetPlayer(), "Credits:0", true, true)\n',
        )
        validate_extras(extra_job.result())
        require(digest(slot) == before, "Extras credits changed the scratch save")
        validate_campaign(campaign_job.result())
    print("PASS: natural Extras credits return and natural campaign credits, teaser, and title")


if __name__ == "__main__":
    main()
