//! Copies WebView2Loader.dll next to the built exe. With the GNU toolchain `webview2-com-sys`
//! links the loader dynamically, so the exe needs the DLL beside it to start.

fn main() {
    println!("cargo:rerun-if-changed=vendor/WebView2Loader.dll");
    println!("cargo:rerun-if-changed=assets");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    // OUT_DIR = <target>/<profile>/build/lockin-<hash>/out
    let Some(profile_dir) = out.ancestors().nth(3) else { return };
    let src = std::path::Path::new("vendor/WebView2Loader.dll");
    let dst = profile_dir.join("WebView2Loader.dll");
    let stale = match (std::fs::metadata(src), std::fs::metadata(&dst)) {
        (Ok(a), Ok(b)) => a.len() != b.len(),
        _ => true,
    };
    if stale {
        std::fs::copy(src, &dst).expect("copy WebView2Loader.dll");
    }
}
