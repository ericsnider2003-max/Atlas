# Why this crate is vendored

webview2-com-sys 0.38.2, unchanged except for two things:

1. `src/lib.rs`: on the GNU toolchain (how atlas.exe is built), the five
   WebView2 loader functions are no longer linked against
   `WebView2Loader.dll`. That import made Windows refuse to start atlas.exe
   at all unless the DLL sat beside it, which breaks "one file, double-click
   it". Atlas defines the five functions itself (`src/webview2_loader.rs`)
   and loads Microsoft's signed `WebView2Loader.dll` only when the hub is
   first shown.
2. `build.rs` and the tree: only the x64 libraries are kept.

MSVC builds are untouched: they still link `WebView2LoaderStatic.lib`.
