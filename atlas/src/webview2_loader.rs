//! Windows' web view, loaded only when the hub is first shown.
//!
//! The hub sits inside Atlas's window through WebView2 (`hubwin`). Microsoft
//! splits WebView2 in two: the runtime, which ships with Windows 11 and Edge,
//! and a small loader, `WebView2Loader.dll`, which each app carries. On the
//! toolchain atlas.exe is built with, the bindings linked that loader as an
//! import. That meant Windows refused to start atlas.exe *at all*, before any
//! of Atlas ran, unless the DLL sat in the same folder. That breaks "one file,
//! double-click it" (found 23 Sep 2026 by reading the built exe's import
//! table, before it ever reached the laptop).
//!
//! So the bindings are vendored with that import removed
//! (`vendor/webview2-com-sys/ATLAS_VENDORED.md`), and the five loader
//! functions are defined here. Each one loads Microsoft's own signed loader,
//! carried inside atlas.exe and written to `tools/webview2/` beside Atlas the
//! first time it's needed, then passes the call through. If the loader can't
//! be written or loaded, only the Hub page is affected: it says so in words,
//! and the rest of Atlas runs as before.
#![cfg(all(windows, target_env = "gnu"))]

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::OnceLock;

type Hresult = i32;
/// HRESULT_FROM_WIN32(ERROR_MOD_NOT_FOUND): "the module could not be found".
const NOT_FOUND: Hresult = 0x8007_007Eu32 as i32;

/// Microsoft's WebView2 loader, x64, as shipped in the WebView2 SDK (signed
/// by Microsoft; redistributable with an app, which is what it's for).
const LOADER: &[u8] = include_bytes!("../vendor/webview2-com-sys/x64/WebView2Loader.dll");

extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
}

/// Where the loader is written: in Atlas's own folder, with its other tools.
pub fn place() -> PathBuf {
    crate::roots::install_root().join("tools").join("webview2").join("WebView2Loader.dll")
}

static MODULE: OnceLock<usize> = OnceLock::new();

fn module() -> Option<*mut c_void> {
    let m = *MODULE.get_or_init(|| {
        let path = place();
        let current = std::fs::read(&path).map(|b| b == LOADER).unwrap_or(false);
        if !current {
            if let Some(dir) = path.parent() {
                crate::heard!(std::fs::create_dir_all(dir));
            }
            if std::fs::write(&path, LOADER).is_err() {
                return 0;
            }
        }
        let wide: Vec<u16> = path.as_os_str().encode_wide_z();
        unsafe { LoadLibraryW(wide.as_ptr()) as usize }
    });
    (m != 0).then_some(m as *mut c_void)
}

trait WideZ {
    fn encode_wide_z(&self) -> Vec<u16>;
}

impl WideZ for std::ffi::OsStr {
    fn encode_wide_z(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain(std::iter::once(0)).collect()
    }
}

/// The loader's function by name, if the loader could be loaded.
fn find(name: &[u8]) -> Option<*mut c_void> {
    let m = module()?;
    let f = unsafe { GetProcAddress(m, name.as_ptr()) };
    (!f.is_null()).then_some(f)
}

#[no_mangle]
unsafe extern "system" fn CreateCoreWebView2EnvironmentWithOptions(
    browser_folder: *const u16,
    user_data_folder: *const u16,
    options: *mut c_void,
    handler: *mut c_void,
) -> Hresult {
    type F = unsafe extern "system" fn(*const u16, *const u16, *mut c_void, *mut c_void) -> Hresult;
    match find(b"CreateCoreWebView2EnvironmentWithOptions\0") {
        Some(f) => std::mem::transmute::<*mut c_void, F>(f)(browser_folder, user_data_folder, options, handler),
        None => NOT_FOUND,
    }
}

#[no_mangle]
unsafe extern "system" fn CreateCoreWebView2Environment(handler: *mut c_void) -> Hresult {
    type F = unsafe extern "system" fn(*mut c_void) -> Hresult;
    match find(b"CreateCoreWebView2Environment\0") {
        Some(f) => std::mem::transmute::<*mut c_void, F>(f)(handler),
        None => NOT_FOUND,
    }
}

#[no_mangle]
unsafe extern "system" fn GetAvailableCoreWebView2BrowserVersionString(
    browser_folder: *const u16,
    version: *mut *mut u16,
) -> Hresult {
    type F = unsafe extern "system" fn(*const u16, *mut *mut u16) -> Hresult;
    match find(b"GetAvailableCoreWebView2BrowserVersionString\0") {
        Some(f) => std::mem::transmute::<*mut c_void, F>(f)(browser_folder, version),
        None => NOT_FOUND,
    }
}

#[no_mangle]
unsafe extern "system" fn GetAvailableCoreWebView2BrowserVersionStringWithOptions(
    browser_folder: *const u16,
    options: *mut c_void,
    version: *mut *mut u16,
) -> Hresult {
    type F = unsafe extern "system" fn(*const u16, *mut c_void, *mut *mut u16) -> Hresult;
    match find(b"GetAvailableCoreWebView2BrowserVersionStringWithOptions\0") {
        Some(f) => std::mem::transmute::<*mut c_void, F>(f)(browser_folder, options, version),
        None => NOT_FOUND,
    }
}

#[no_mangle]
unsafe extern "system" fn CompareBrowserVersions(a: *const u16, b: *const u16, result: *mut i32) -> Hresult {
    type F = unsafe extern "system" fn(*const u16, *const u16, *mut i32) -> Hresult;
    match find(b"CompareBrowserVersions\0") {
        Some(f) => std::mem::transmute::<*mut c_void, F>(f)(a, b, result),
        None => NOT_FOUND,
    }
}
