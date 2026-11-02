//! Embeds the Windows application icon and copies WebView2Loader.dll next to the built exe. With
//! the GNU toolchain `webview2-com-sys` links the loader dynamically, so the exe needs the DLL
//! beside it to start. Also bakes in the feedback webhook (env `LOCKIN_FEEDBACK_WEBHOOK`, or the
//! git-ignored `feedback-webhook.txt`) so the URL never lands in the public source.

fn main() {
    println!("cargo:rerun-if-changed=vendor/WebView2Loader.dll");
    println!("cargo:rerun-if-changed=assets/brand/app.rc");
    println!("cargo:rerun-if-changed=assets/brand/lockin.ico");
    println!("cargo:rerun-if-changed=assets");
    println!("cargo:rerun-if-env-changed=LOCKIN_FEEDBACK_WEBHOOK");
    println!("cargo:rerun-if-changed=feedback-webhook.txt");
    let webhook = std::env::var("LOCKIN_FEEDBACK_WEBHOOK")
        .ok()
        .or_else(|| std::fs::read_to_string("feedback-webhook.txt").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| s.starts_with("https://"));
    if let Some(url) = webhook {
        println!("cargo:rustc-env=LOCKIN_FEEDBACK_WEBHOOK={url}");
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let resource = out.join("lockin-icon.o");
        let status = std::process::Command::new("windres")
            .args([
                "--input",
                "assets/brand/app.rc",
                "--output-format=coff",
                "--include-dir",
                "assets/brand",
                "--output",
            ])
            .arg(&resource)
            .status()
            .expect("run windres to compile the LockIn icon resource");
        assert!(
            status.success(),
            "windres failed to compile assets/brand/app.rc"
        );
        println!("cargo:rustc-link-arg={}", resource.display());
    }

    // OUT_DIR = <target>/<profile>/build/lockin-<hash>/out
    let Some(profile_dir) = out.ancestors().nth(3) else {
        return;
    };
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
