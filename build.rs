// Build script: embed `resources/app.ico` into the Windows executable as the
// default application icon. On non-Windows targets this is a no-op.
//
// The resource script is compiled with `windres` (MinGW) or `rc.exe` (MSVC)
// when available. If neither is found the build still succeeds; the .exe simply
// won't carry a custom icon (the runtime window icon is still applied).
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() != "windows" {
        return;
    }

    let manifest_dir = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(v) => v,
        Err(_) => return,
    };
    let out_dir = match std::env::var("OUT_DIR") {
        Ok(v) => v,
        Err(_) => return,
    };

    let rc_path = format!("{manifest_dir}/resources/app.rc");
    let obj_out = format!("{out_dir}/app_icon_res.o");

    let tool = std::env::var("WINDRES")
        .or_else(|_| std::env::var("RC"))
        .unwrap_or_else(|_| "windres".to_string());

    let status = std::process::Command::new(&tool)
        .args(["-i", &rc_path, "-o", &obj_out])
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("cargo:rustc-link-arg={obj_out}");
            println!("cargo:rerun-if-changed=resources/app.rc");
            println!("cargo:rerun-if-changed=resources/app.ico");
        }
        _ => {
            eprintln!(
                "warning: failed to compile resources/app.rc with `{tool}` (tool not found or failed); \
                 the Windows executable will not embed a custom icon"
            );
        }
    }
}
