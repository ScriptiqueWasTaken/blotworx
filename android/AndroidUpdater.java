package dev.bloticle.blotworx;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.ClipData;
import android.content.Intent;
import android.content.pm.PackageInfo;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.os.Build;
import android.provider.Settings;
import android.util.Log;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.util.Arrays;
import java.util.Locale;
import org.json.JSONArray;
import org.json.JSONObject;

final class AndroidUpdater {
    private static final String RELEASES = "https://api.github.com/repos/Bloticle/blotworx/releases?per_page=20";
    private static final String ASSET_ROOT = "https://github.com/bloticle/blotworx/releases/download/";
    private static final long MAX_APK = 512L * 1024 * 1024;
    private static final int ALLOW_INSTALL = 317;
    private final Activity activity;
    private boolean awaitingInstallPermission;

    AndroidUpdater(Activity activity) { this.activity = activity; }

    void check() {
        new Thread(() -> {
            try {
                Asset asset = findUpdate(version(installedInfo(0)));
                if (asset != null) show(() -> offer(asset));
            } catch (Exception error) {
                Log.w("Blotworx", "Could not check for an Android update", error);
            }
        }, "Blotworx update check").start();
    }

    private static HttpURLConnection connect(String url) throws IOException {
        HttpURLConnection connection = (HttpURLConnection) new URL(url).openConnection();
        connection.setConnectTimeout(10000);
        connection.setReadTimeout(20000);
        connection.setRequestProperty("User-Agent", "Blotworx-Android");
        return connection;
    }

    private static final class Asset {
        String url, digest, tag;
        long code, size;
    }

    private static Asset findUpdate(long current) throws Exception {
        HttpURLConnection connection = connect(RELEASES);
        ByteArrayOutputStream output = new ByteArrayOutputStream();
        try (InputStream input = connection.getInputStream()) {
            byte[] bytes = new byte[8192];
            for (int read; (read = input.read(bytes)) != -1;) {
                if (output.size() + read > 4 * 1024 * 1024) throw new IOException("Release list is too large");
                output.write(bytes, 0, read);
            }
        } finally { connection.disconnect(); }
        JSONArray releases = new JSONArray(new String(output.toByteArray(), StandardCharsets.UTF_8));
        Asset best = null;
        for (int i = 0; i < releases.length(); i++) {
            JSONObject release = releases.getJSONObject(i);
            if (release.optBoolean("draft")) continue;
            JSONArray assets = release.getJSONArray("assets");
            for (int j = 0; j < assets.length(); j++) {
                JSONObject entry = assets.getJSONObject(j);
                String name = entry.optString("name");
                String label = entry.optString("label");
                String url = entry.optString("browser_download_url");
                String digest = entry.optString("digest");
                long size = entry.optLong("size");
                if (!name.matches("Blotworx-[A-Za-z0-9._-]+-Android-arm64\\.apk")
                    || !label.matches("android-version-code=[1-9][0-9]{0,9}")
                    || !url.toLowerCase(Locale.ROOT).startsWith(ASSET_ROOT) || !url.endsWith("/" + name)
                    || !digest.matches("sha256:[a-f0-9]{64}") || size <= 0 || size > MAX_APK) continue;
                long code = Long.parseLong(label.substring("android-version-code=".length()));
                if (code <= current || (best != null && code <= best.code)) continue;
                best = new Asset();
                best.url = url;
                best.digest = digest.substring("sha256:".length());
                best.tag = release.optString("tag_name");
                best.size = size;
                best.code = code;
            }
        }
        return best;
    }

    private void show(Runnable action) {
        activity.runOnUiThread(() -> { if (!activity.isFinishing() && !activity.isDestroyed()) action.run(); });
    }

    private void offer(Asset asset) {
        new AlertDialog.Builder(activity).setTitle("Blotworx update available")
            .setMessage(asset.tag + " · " + ((asset.size + 1048575) / 1048576) + " MiB")
            .setNegativeButton("Later", null).setPositiveButton("Download", (dialog, button) -> download(asset)).show();
    }

    private void download(Asset asset) {
        new Thread(() -> {
            File temp = null;
            try {
                temp = File.createTempFile("blotworx-update-", ".apk", activity.getCacheDir());
                MessageDigest digest = MessageDigest.getInstance("SHA-256");
                HttpURLConnection connection = connect(asset.url);
                long length = 0;
                try (InputStream input = connection.getInputStream(); FileOutputStream output = new FileOutputStream(temp)) {
                    byte[] bytes = new byte[65536];
                    for (int read; (read = input.read(bytes)) != -1;) {
                        length += read;
                        if (length > asset.size) throw new IOException("Unexpected update size");
                        digest.update(bytes, 0, read);
                        output.write(bytes, 0, read);
                    }
                } finally { connection.disconnect(); }
                StringBuilder hex = new StringBuilder(64);
                for (byte value : digest.digest()) hex.append(String.format(Locale.ROOT, "%02x", value & 255));
                if (length != asset.size || !asset.digest.contentEquals(hex)) throw new IOException("Update checksum did not match");
                validatePackage(temp, asset.code);
                if (!temp.renameTo(BlotworxUpdateProvider.file(activity))) throw new IOException("Could not stage update");
                show(() -> new AlertDialog.Builder(activity).setTitle("Update ready")
                    .setMessage("Android will open its installer. Your imported game data and saves stay in place.")
                    .setNegativeButton("Later", null).setPositiveButton("Install", (dialog, button) -> install()).show());
            } catch (Exception error) {
                show(() -> showError(error));
            } finally { if (temp != null) temp.delete(); }
        }, "Blotworx update download").start();
    }

    @SuppressWarnings("deprecation")
    private static long version(PackageInfo info) {
        return Build.VERSION.SDK_INT >= 28 ? info.getLongVersionCode() : info.versionCode;
    }

    @SuppressWarnings("deprecation")
    private PackageInfo installedInfo(int flags) throws PackageManager.NameNotFoundException {
        return activity.getPackageManager().getPackageInfo(activity.getPackageName(), flags);
    }

    @SuppressWarnings("deprecation")
    private void validatePackage(File file, long code) throws Exception {
        int flags = Build.VERSION.SDK_INT >= 28 ? PackageManager.GET_SIGNING_CERTIFICATES : PackageManager.GET_SIGNATURES;
        PackageInfo installed = installedInfo(flags);
        PackageInfo update = activity.getPackageManager().getPackageArchiveInfo(file.getAbsolutePath(), flags);
        if (update == null || !installed.packageName.equals(update.packageName) || version(update) != code || code <= version(installed)) {
            throw new IOException("Update APK has an unexpected package or version");
        }
        boolean matches = Build.VERSION.SDK_INT >= 28
            ? installed.signingInfo != null && update.signingInfo != null && Arrays.equals(installed.signingInfo.getApkContentsSigners(), update.signingInfo.getApkContentsSigners())
            : installed.signatures != null && Arrays.equals(installed.signatures, update.signatures);
        if (!matches) throw new IOException("Update APK is signed with a different key");
    }

    @SuppressWarnings("deprecation")
    private void install() {
        try {
            if (!activity.getPackageManager().canRequestPackageInstalls()) {
                awaitingInstallPermission = true;
                activity.startActivityForResult(new Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                    Uri.fromParts("package", activity.getPackageName(), null)), ALLOW_INSTALL);
                return;
            }
            Uri uri = BlotworxUpdateProvider.uri(activity);
            Intent install = new Intent(Intent.ACTION_INSTALL_PACKAGE, uri);
            install.setClipData(ClipData.newRawUri("Blotworx update", uri));
            install.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
            activity.startActivity(install);
        } catch (RuntimeException error) { showError(error); }
    }

    void permissionResult(int request) {
        if (request != ALLOW_INSTALL || !awaitingInstallPermission) return;
        awaitingInstallPermission = false;
        if (activity.getPackageManager().canRequestPackageInstalls()) install();
    }

    private void showError(Exception error) {
        new AlertDialog.Builder(activity).setTitle("Blotworx update failed")
            .setMessage(error.getMessage()).setPositiveButton("OK", null).show();
    }
}
