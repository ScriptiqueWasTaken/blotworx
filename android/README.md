# Blotworx on Android

This experimental build targets Vulkan-capable ARM64 devices running Android 8.0 or later. It uses the device's document picker to import a North American Epic Mickey Wii disc image. Import makes a temporary copy of the selected image, so leave enough free storage for the image and the extracted game. Use a compatible gamepad for gameplay.

The APK includes FFmpeg for Bink cinematics and uses Android's AAudio for PCM output. The external Wwise media decoder has not been ported, so some sounds and music may be missing. Android gameplay, import and media playback still need an on-device check.

Blotworx checks GitHub for updates at startup. It verifies the download and asks Android to open its installer after you choose **Install**. Keep the application ID and signing key, increment the Android `versionCode` for each APK, and label its GitHub release asset `android-version-code=N`.

## Build

Use Rust with the `aarch64-linux-android` target, Android NDK `27.3.13750724`, Android SDK platform 35 and Build Tools `35.0.0`. On Windows set `CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER`, `CC_aarch64_linux_android`, `CXX_aarch64_linux_android` and `AR_aarch64_linux_android` to the corresponding API 26 NDK compiler and archiver. Run `cargo build --locked --lib --target aarch64-linux-android`.

Download and unpack `ffmpeg-7.1.5.tar.xz`. Its SHA-256 is `de668509caf9e35e3cd162473441fdb29538c6d96ed080292b3cf9e6fc5d558f`. From the FFmpeg source directory, run `build-codec.sh` with the NDK toolchain directory and GNU make path using Git Bash. This builds the LGPL FFmpeg executable without external libraries. [FFmpeg source archive](https://ffmpeg.org/releases/ffmpeg-7.1.5.tar.xz)

Run `package-apk.ps1` with the SDK and JDK locations, both native binaries, FFmpeg's `COPYING.LGPLv2.1` file, an output APK path and a private keystore with alias `blotworx`. Supply the signing password through `BLOTWORX_SIGNING_PASSWORD`. The script aligns the APK, signs it and verifies its signature and package metadata.
