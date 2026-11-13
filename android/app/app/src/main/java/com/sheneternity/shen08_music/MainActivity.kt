package com.sheneternity.shen08_music

import android.annotation.TargetApi
import android.app.Activity
import android.os.Build
import android.os.Bundle
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.View
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.view.WindowManager

/**
 * 尽可能薄的壳。它只拥有三样 Rust 侧自己拿不到的东西：
 *
 *   1. [SurfaceView] / [SurfaceHolder]（native window + 它的生命周期），
 *   2. Activity 生命周期（resume / pause / destroy），
 *   3. 原始输入分发（触摸 + 按键事件）。
 *
 * 其余一切 —— 窗口创建、渲染、整个 UI 树 —— 都在 Rust 里
 * （`gpuidemo-core`，基于 gpui-kit，由 gpui-mobile 的 `android::host` 承载）。
 * 这里刻意不写任何 Kotlin UI 逻辑。
 *
 * 每个回调都转发给一个在 `rust/android/src/lib.rs` 里实现的 `native*` 函数，
 * 由它把事件交给 `gpui_mobile::android::host`。
 */
class MainActivity : Activity(), SurfaceHolder.Callback {

    private lateinit var surfaceView: SurfaceView

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // 把这个 Activity 注册给 Rust host，并（只在首次启动时）
        // 拉起渲染线程。每次 onCreate 调用都是安全的。
        nativeCreate(this)

        surfaceView = SurfaceView(this)
        surfaceView.holder.addCallback(this)

        // Rust 渲染器会绘制整块 surface，所以要阻止窗口在它背后绘制任何东西，
        // 做到真正的全面屏。
        window.setBackgroundDrawable(null)
        window.setFlags(
            WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON,
            WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON,
        )

        setContentView(surfaceView)

        // 隐藏系统栏，得到沉浸式、铺满全屏的画布。
        hideSystemBars()
    }

    @TargetApi(Build.VERSION_CODES.R)
    private fun hideSystemBars() {
        // `window.insetsController` 是 API 30+ 才有的，更低版本返回 null；
        // `?.let` 守卫让它在 API 26–29 上变成空操作（minSdk 是 26）。
        window.insetsController?.let { controller ->
            // `Type` 常量在 `WindowInsets` 上，不在 controller 上。
            controller.hide(WindowInsets.Type.systemBars())
            controller.systemBarsBehavior =
                WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        }
    }

    // ── SurfaceHolder.Callback ────────────────────────────────────────────

    override fun surfaceCreated(holder: SurfaceHolder) {
        nativeSurfaceCreated(holder.surface, resources.displayMetrics.density)
    }

    // 一并转发：尺寸变化会重新 attach / 调整同一块 surface。
    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        nativeSurfaceCreated(holder.surface, resources.displayMetrics.density)
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        // nativeSurfaceDestroyed 会阻塞，直到渲染线程停止使用这块 surface，
        // 所以之后框架回收它是安全的。
        nativeSurfaceDestroyed()
    }
    // ── 生命周期 ─────────────────────────────────────────────────────────

    override fun onResume() {
        super.onResume()
        nativeResumed()
        hideSystemBars()
    }

    override fun onPause() {
        nativePaused()
        super.onPause()
    }

    // ── 输入 ─────────────────────────────────────────────────────────────

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

    // ── Native 桥 ─────────────────────────────────────────────────────────
    //
    // 在 rust/android/src/lib.rs 里实现。`_1` 是 `gpui_demo` 包名里
    // 下划线的 JNI 转义。

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
            // 加载 libgpuidemo.so（由 cargo ndk 构建到 jniLibs/x86_64）。
            System.loadLibrary("android_lib")
        }
    }
}
