//! Thin JNI shell: owns nothing but the bridge between the Kotlin
//! `MainActivity` (which owns the `SurfaceView` + lifecycle) and
//! `gpui_mobile::android::host` (which owns the render thread + GPUI app).
//!
//! Contract, straight from `gpui_mobile::android::host`'s docs — from the Java
//! UI thread, in this order:
//!   1. `Activity.onCreate`        → `set_host_activity` + `host::start`
//!   2. `SurfaceHolder.surfaceCreated` / `surfaceChanged` → `surface_created`
//!   3. `SurfaceHolder.surfaceDestroyed`                  → `surface_destroyed`
//!   4. `onResume` / `onPause`                            → `resumed` / `paused`
//!   5. `onTouchEvent` / `dispatchKeyEvent`               → `motion_event` / `key`
//!
//! All UI is built in (gpui-kit); there is zero Kotlin UI logic.

use gpui_mobile::android::host;
use gpui_mobile::android::jni as gm_jni;
use jni::errors::Error as JniError;
use jni::objects::{JClass, JFloatArray, JIntArray, JObject};
use jni::sys::{jfloatArray, jint, jintArray};
use jni::{EnvUnowned, Outcome};

/// Stub NativeActivity entry point.
///
/// `gpui-mobile` always enables `android-activity`'s `native-activity` feature,
/// whose C glue (`android_native_app_glue.c`) references `android_main` and
/// expects the application to define it. We use the **host-driven** path instead
/// (a plain Kotlin `Activity` + `host::start`), so NativeActivity is never
/// registered and this is never called. It exists purely so the symbol resolves
/// at `dlopen` time — without it the `.so` fails to load with
/// `cannot locate symbol "android_main"`.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(_app: android_activity::AndroidApp) {
    log::error!("android_main: NativeActivity entry reached, but this demo uses the host-driven path");
}

/// Run `f` with a borrowed `&mut Env` and log (rather than unwind) any error.
///
/// jni 0.22's `export` macro hands native methods an `EnvUnowned`, which guards
/// against unwinding into the JVM. We never want a Rust panic to abort the
/// process from a UI-thread callback, so errors just log and fall back to `T::default()`.
fn with_env<T: Default>(
    env: &mut EnvUnowned<'_>,
    f: impl FnOnce(&mut jni::Env<'_>) -> Result<T, JniError>,
) -> T {
    match env.with_env_no_catch(f).into_outcome() {
        Outcome::Ok(v) => v,
        Outcome::Err(e) => {
            log::error!("jni error: {e}");
            T::default()
        }
        Outcome::Panic(_) => {
            log::error!("jni call panicked");
            T::default()
        }
    }
}

/// `Activity.onCreate`. Registers the live Activity (a recreated Activity is a
/// new object, so this runs every time) and starts the process-lived render
/// thread exactly once. The `core::launch` closure runs on the render thread
/// once the first surface arrives.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_sheneternity_shen08_1music_MainActivity_nativeCreate(
    mut env: EnvUnowned<'_>,
    _class: JClass,
    activity: JObject,
) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("gpui-demo"),
    );
    gm_jni::install_panic_hook();
    log::info!("nativeCreate: registering host activity");

    // `set_host_activity` reports failures as a `String` (not a jni error);
    // log it and continue — the render thread can still start and will simply
    // have no activity-attached window until the surface arrives.
    with_env(&mut env, |env| {
        if let Err(e) = gm_jni::set_host_activity(env, &activity) {
            log::error!("set_host_activity: {e}");
        }
        Ok(())
    });

    // Idempotent: only the first call spawns the render thread; later Activity
    // recreations reuse the surviving thread / platform / GPUI App.
    host::start(|cx| android_core::launch(cx));
}

/// `SurfaceHolder.Callback.surfaceCreated` and `surfaceChanged`. Hands the
/// native window to the render thread. A repeat of the surface already held is
/// treated as a resize by the host.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_sheneternity_shen08_1music_MainActivity_nativeSurfaceCreated(
    env: EnvUnowned<'_>,
    _class: JClass,
    surface: JObject,
    scale: f32,
) {
    // `ANativeWindow_fromSurface` acquires a reference we own; `NativeWindow::from_ptr`
    // adopts it and releases it on drop (on the render thread). `NativeWindow` is
    // `Send`, so handing it across threads is sound. `env.as_raw()` is the
    // `*mut JNIEnv` for this already-attached UI thread.
    // jni 0.22 (jni-sys 0.4) and ndk-sys 0.6 (jni-sys 0.3) name the same
    // `JNINativeInterface_` C struct through two distinct Rust types, so cast
    // through the raw pointer. `as_raw()` is the JNIEnv* of this attached UI thread.
    let native = unsafe {
        ndk_sys::ANativeWindow_fromSurface(
            // jni 0.22 (jni-sys 0.4) and ndk-sys 0.6 (jni-sys 0.3) name the
            // same `JNIEnv*` C type through two distinct Rust types, and
            // ndk-sys's alias is private. Cast through `*mut _` so inference
            // picks ndk-sys's own pointee without naming it.
            env.as_raw() as *mut _,
            surface.as_raw(),
        )
    };
    if native.is_null() {
        log::error!("nativeSurfaceCreated: ANativeWindow_fromSurface returned null");
        return;
    }
    let window = unsafe {
        // `as *mut _` papers over jni 0.21 vs 0.22 naming the same
        // `ANativeWindow` through two jni-sys versions.
        ndk::native_window::NativeWindow::from_ptr(
            std::ptr::NonNull::new_unchecked(native as *mut _),
        )
    };
    host::surface_created(window, scale);
}

/// `SurfaceHolder.Callback.surfaceDestroyed`. Blocks until the render thread
/// has stopped using the surface (mandatory to avoid a use-after-free).
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_sheneternity_shen08_1music_MainActivity_nativeSurfaceDestroyed(
    _env: EnvUnowned<'_>,
    _class: JClass,
) {
    host::surface_destroyed();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_sheneternity_shen08_1music_MainActivity_nativeResumed(
    _env: EnvUnowned<'_>,
    _class: JClass,
) {
    host::resumed();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_sheneternity_shen08_1music_MainActivity_nativePaused(
    _env: EnvUnowned<'_>,
    _class: JClass,
) {
    host::paused();
}

/// `onTouchEvent`. Returns whether GPUI handled it (the Java side should return
/// `handled` from `onTouchEvent`).
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_sheneternity_shen08_1music_MainActivity_nativeTouch(
    mut env: EnvUnowned<'_>,
    _class: JClass,
    action: jint,
    action_index: jint,
    ids: jintArray,
    xs: jfloatArray,
    ys: jfloatArray,
) -> bool {
    with_env(&mut env, |env| {
        // Wrap the raw JNI arrays once; `len`/`get_region` take `&Env`.
        let (ids_arr, xs_arr, ys_arr) = unsafe {
            (
                JIntArray::from_raw(env, ids),
                JFloatArray::from_raw(env, xs),
                JFloatArray::from_raw(env, ys),
            )
        };
        let count = xs_arr.len(env)?;
        let mut ids_v = vec![0i32; count as usize];
        let mut xs_v = vec![0f32; count as usize];
        let mut ys_v = vec![0f32; count as usize];
        ids_arr.get_region(env, 0, &mut ids_v)?;
        xs_arr.get_region(env, 0, &mut xs_v)?;
        ys_arr.get_region(env, 0, &mut ys_v)?;

        let pointers: Vec<host::Pointer> = (0..count as usize)
            .map(|i| host::Pointer {
                id: ids_v[i],
                x: xs_v[i],
                y: ys_v[i],
            })
            .collect();

        Ok(host::motion_event(action as u32, action_index as usize, &pointers))
    })
}

/// `dispatchKeyEvent`. `action` is 0 for down, 1 for up.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_sheneternity_shen08_1music_MainActivity_nativeKey(
    _env: EnvUnowned<'_>,
    _class: JClass,
    key_code: jint,
    action: jint,
    meta_state: jint,
) {
    host::key(key_code, action, meta_state);
}