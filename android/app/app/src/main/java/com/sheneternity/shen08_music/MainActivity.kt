package com.sheneternity.shen08_music

import android.annotation.TargetApi
import android.app.Activity
import android.os.Build
import android.os.Bundle
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.view.WindowManager

/**
 * The thinnest possible shell. It owns exactly three things the Rust side
 * cannot get for itself:
 *
 *   1. the [SurfaceView] / [SurfaceHolder] (the native window + its lifetime),
 *   2. the Activity lifecycle (resume / pause / destroy),
 *   3. raw input dispatch (touch + key events).
 *
 * Everything else — window creation, rendering, the whole UI tree — lives in
 * Rust (`gpuidemo-core` on gpui-kit, hosted by gpui-mobile's `android::host`).
 * There is deliberately no Kotlin UI logic here.
 *
 * Each callback forwards to a `native*` function implemented in
 * `rust/android/src/lib.rs`, which hands the event to `gpui_mobile::android::host`.
 */
class MainActivity : Activity(), SurfaceHolder.Callback {

    private lateinit var surfaceView: SurfaceView

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // Register this Activity with the Rust host and (on first launch only)
        // spawn the render thread. Safe to call on every onCreate.
        nativeCreate(this)

        surfaceView = SurfaceView(this)
        surfaceView.holder.addCallback(this)

        // The Rust renderer paints the entire surface, so keep the window from
        // drawing anything behind it and go truly edge-to-edge.
        window.setBackgroundDrawable(null)
        window.setFlags(
            WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON,
            WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON,
        )

        setContentView(surfaceView)

        // Hide the system bars for an immersive, full-bleed canvas.
        hideSystemBars()
    }

    @TargetApi(Build.VERSION_CODES.R)
    private fun hideSystemBars() {
        // `window.insetsController` is API 30+ and returns null below that; the
        // `?.let` guard makes this a no-op on API 26–29 (minSdk is 26).
        window.insetsController?.let { controller ->
            // The `Type` constants live on `WindowInsets`, not the controller.
            controller.hide(WindowInsets.Type.systemBars())
            controller.systemBarsBehavior =
                WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        }
    }

    // ── SurfaceHolder.Callback ────────────────────────────────────────────

    override fun surfaceCreated(holder: SurfaceHolder) {
        nativeSurfaceCreated(holder.surface, resources.displayMetrics.density)
    }

    // Also forwarded: a size change re-attaches / resizes the same surface.
    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        nativeSurfaceCreated(holder.surface, resources.displayMetrics.density)
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        // nativeSurfaceDestroyed blocks until the render thread stops using the
        // surface, so it is safe for the framework to recycle it afterwards.
        nativeSurfaceDestroyed()
    }

    // ── Lifecycle ─────────────────────────────────────────────────────────

    override fun onResume() {
        super.onResume()
        nativeResumed()
        hideSystemBars()
    }

    override fun onPause() {
        nativePaused()
        super.onPause()
    }

    // ── Input ─────────────────────────────────────────────────────────────

    override fun onTouchEvent(event: MotionEvent): Boolean {
        val handled = nativeTouch(
            event.actionMasked,
            event.actionIndex,
            IntArray(event.pointerCount) { event.getPointerId(it) },
            FloatArray(event.pointerCount) { event.getX(it) },
            FloatArray(event.pointerCount) { event.getY(it) },
        )
        return handled || super.onTouchEvent(event)
    }

    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        val action = when (event.action) {
            KeyEvent.ACTION_DOWN -> 0
            KeyEvent.ACTION_UP -> 1
            else -> return super.dispatchKeyEvent(event)
        }
        nativeKey(event.keyCode, action, event.metaState)
        return true
    }

    // ── Native bridge ─────────────────────────────────────────────────────
    private external fun nativeCreate(activity: Activity)
    private external fun nativeSurfaceCreated(surface: android.view.Surface, scale: Float)
    private external fun nativeSurfaceDestroyed()
    private external fun nativeResumed()
    private external fun nativePaused()
    private external fun nativeTouch(
        action: Int,
        actionIndex: Int,
        ids: IntArray,
        xs: FloatArray,
        ys: FloatArray,
    ): Boolean
    private external fun nativeKey(keyCode: Int, action: Int, metaState: Int)

    companion object {
        init {
            // Loads libandroid_lib.so (built by cargo ndk into jniLibs/{arm64-v8a,x86_64}).
            // The crate name in rust-lib/android-lib/Cargo.toml is `android_lib`,
            // and JNI symbols like nativeCreate live in that .so.
            System.loadLibrary("android_lib")
        }
    }
}
