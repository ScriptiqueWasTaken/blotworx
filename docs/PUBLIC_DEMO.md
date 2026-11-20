# Blotworx public demo

Blotworx runs the original Epic Mickey campaign with files imported from your own Wii disc image. This Windows x64 preview requires the North American release. The download contains the engine only.

1. Extract the demo ZIP to a folder you can write to.
2. Open **Blotworx.exe** and choose or drop your disc image. Setup supports ISO, GCM, RVZ, WBFS, WIA, CISO, GCZ, NFS and a ZIP containing one supported image. Split images must be supplied unpacked with all parts together.
3. Wait for setup to finish. From the original title screen, press **Enter** or your controller’s **A** button. Select **Start Game**, then choose a save slot.

Use the same slot to continue from its last checkpoint. Game data and saves live in separate folders under `%LOCALAPPDATA%\Blotworx Engine`. Keep your original disc image if you want to reimport it later.

Keep the demo online for its first launch so it can fetch and verify its audio and movie codecs. They are cached for later launches.

Blotworx checks GitHub for updates at launch. The update prompt shows the release notes and download size. You can dismiss it or download the update and restart to install. Your imported files and saves stay in place.

Press **Esc** to pause and **F10** to open Blotworx settings. This is a development preview. Later-level progression, game behavior and visual accuracy are still being worked on.

If the demo closes during startup, copy `%LOCALAPPDATA%\Blotworx Engine\blotworx.log` before launching it again and attach it to your bug report. Each launch replaces the log.
