package dev.bloticle.blotworx;

import android.app.NativeActivity;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.provider.OpenableColumns;
import android.view.InputDevice;
import android.view.KeyEvent;
import android.view.MotionEvent;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.util.Locale;

public final class BlotworxActivity extends NativeActivity {
    static { System.loadLibrary("blotworx"); }

    private static final int IMPORT = 1;
    private boolean importing;

    @Override protected void onCreate(android.os.Bundle savedState) {
        try {
            android.system.Os.setenv("BLOTWORX_ANDROID_CODECS", getApplicationInfo().nativeLibraryDir, true);
        } catch (android.system.ErrnoException error) {
            android.util.Log.e("Blotworx", "Cannot initialize codec directory", error);
        }
        super.onCreate(savedState);
    }

    private int gamepadKeys;
    private int gamepadHat;
    private float leftX, leftY, rightX, rightY, leftTrigger, rightTrigger;

    private static native void updateGamepad(int buttons, float lx, float ly, float rx, float ry,
                                             float lt, float rt);

    private void publishGamepad() {
        updateGamepad(gamepadKeys | gamepadHat, leftX, leftY, rightX, rightY, leftTrigger, rightTrigger);
    }

    @Override public boolean dispatchKeyEvent(KeyEvent event) {
        if (event.isFromSource(InputDevice.SOURCE_GAMEPAD) || event.isFromSource(InputDevice.SOURCE_JOYSTICK)) {
            int bit = buttonMask(event.getKeyCode());
            if (bit != 0) {
                if (event.getAction() == KeyEvent.ACTION_DOWN) gamepadKeys |= bit;
                if (event.getAction() == KeyEvent.ACTION_UP) gamepadKeys &= ~bit;
                publishGamepad();
                return true;
            }
        }
        return super.dispatchKeyEvent(event);
    }

    @Override public boolean dispatchGenericMotionEvent(MotionEvent event) {
        if (event.isFromSource(InputDevice.SOURCE_JOYSTICK) && event.getAction() == MotionEvent.ACTION_MOVE) {
            leftX = event.getAxisValue(MotionEvent.AXIS_X);
            leftY = -event.getAxisValue(MotionEvent.AXIS_Y);
            rightX = axis(event, MotionEvent.AXIS_Z, MotionEvent.AXIS_RX);
            rightY = -axis(event, MotionEvent.AXIS_RZ, MotionEvent.AXIS_RY);
            leftTrigger = axis(event, MotionEvent.AXIS_LTRIGGER, MotionEvent.AXIS_BRAKE);
            rightTrigger = axis(event, MotionEvent.AXIS_RTRIGGER, MotionEvent.AXIS_GAS);
            float hatX = event.getAxisValue(MotionEvent.AXIS_HAT_X);
            float hatY = event.getAxisValue(MotionEvent.AXIS_HAT_Y);
            gamepadHat = (hatX < -0.5f ? 4 : hatX > 0.5f ? 8 : 0)
                       | (hatY < -0.5f ? 1 : hatY > 0.5f ? 2 : 0);
            publishGamepad();
            return true;
        }
        return super.dispatchGenericMotionEvent(event);
    }

    @Override protected void onPause() {
        super.onPause();
        gamepadKeys = gamepadHat = 0;
        leftX = leftY = rightX = rightY = leftTrigger = rightTrigger = 0;
        publishGamepad();
    }

    private static float axis(MotionEvent event, int preferred, int alternate) {
        InputDevice device = event.getDevice();
        return event.getAxisValue(device != null && device.getMotionRange(preferred, event.getSource()) != null
                                  ? preferred : alternate);
    }

    private static int buttonMask(int button) {
        switch (button) {
            case KeyEvent.KEYCODE_DPAD_UP: return 1;
            case KeyEvent.KEYCODE_DPAD_DOWN: return 2;
            case KeyEvent.KEYCODE_DPAD_LEFT: return 4;
            case KeyEvent.KEYCODE_DPAD_RIGHT: return 8;
            case KeyEvent.KEYCODE_BUTTON_START: return 0x10;
            case KeyEvent.KEYCODE_BUTTON_SELECT: return 0x20;
            case KeyEvent.KEYCODE_BUTTON_L1: return 0x100;
            case KeyEvent.KEYCODE_BUTTON_R1: return 0x200;
            case KeyEvent.KEYCODE_BUTTON_A: return 0x1000;
            case KeyEvent.KEYCODE_BUTTON_B: return 0x2000;
            case KeyEvent.KEYCODE_BUTTON_X: return 0x4000;
            case KeyEvent.KEYCODE_BUTTON_Y: return 0x8000;
            default: return 0;
        }
    }

    public void chooseDiscImage() {
        runOnUiThread(() -> {
            if (importing) return;
            importing = true;
            Intent picker = new Intent(Intent.ACTION_OPEN_DOCUMENT);
            picker.addCategory(Intent.CATEGORY_OPENABLE);
            picker.setType("*/*");
            try {
                startActivityForResult(picker, IMPORT);
            } catch (RuntimeException error) {
                finishImport(null, error);
            }
        });
    }

    @Override protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request != IMPORT) return;
        Uri uri = result == RESULT_OK && data != null ? data.getData() : null;
        if (uri == null) {
            importing = false;
            return;
        }
        new Thread(() -> {
            File output = null;
            try {
                output = File.createTempFile("disc-", extension(uri), getCacheDir());
                try (InputStream input = getContentResolver().openInputStream(uri);
                     FileOutputStream file = new FileOutputStream(output)) {
                    if (input == null) throw new IOException("Unable to open the selected disc image.");
                    byte[] buffer = new byte[1024 * 1024];
                    long copied = 0;
                    int count;
                    while ((count = input.read(buffer)) != -1) {
                        copied += count;
                        if (copied > 9_000_000_000L) throw new IOException("The selected disc image is too large.");
                        file.write(buffer, 0, count);
                    }
                }
                finishImport(output, null);
            } catch (Exception error) {
                if (output != null) output.delete();
                finishImport(null, error);
            }
        }, "Blotworx document import").start();
    }

    private String extension(Uri uri) throws IOException {
        try (Cursor cursor = getContentResolver().query(uri,
                new String[]{OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (cursor != null && cursor.moveToFirst()) {
                String name = cursor.getString(0);
                int dot = name == null ? -1 : name.lastIndexOf('.');
                if (dot >= 0) {
                    String extension = name.substring(dot).toLowerCase(Locale.ROOT);
                    if (extension.matches("\\.(iso|gcm|rvz|wbfs|wia|ciso|gcz|nfs|zip)")) return extension;
                }
            }
        }
        throw new IOException("Choose a supported Wii disc image or ZIP.");
    }

    private void finishImport(File image, Exception error) {
        File temporary = new File(getFilesDir(), "disc-selection.tmp");
        File ready = new File(getFilesDir(), "disc-selection");
        String result = image == null ? "error\n" + error.getMessage() : "image\n" + image;
        try {
            try (FileOutputStream marker = new FileOutputStream(temporary)) {
                marker.write(result.getBytes(StandardCharsets.UTF_8));
            }
            if (!temporary.renameTo(ready)) throw new IOException("Cannot publish disc selection.");
        } catch (IOException failure) {
            if (image != null) image.delete();
            android.util.Log.e("Blotworx", "Cannot publish disc selection", failure);
        } finally {
            temporary.delete();
            runOnUiThread(() -> importing = false);
        }
    }
}
