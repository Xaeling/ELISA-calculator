// 构建脚本：
//   1. 用代码生成程序图标（ICO / PNG），不依赖任何外部图片素材；
//   2. Windows 下把 ICO 作为资源嵌入 exe，使 exe 在资源管理器、任务栏、标题栏都显示该图标；
//   3. 编译 Slint 界面。
//
// 图标生成逻辑见 build/icon.rs（纯 Rust 光栅化 + 手写 ICO/PNG 容器）。

#[path = "build/icon.rs"]
mod icon;

use std::path::PathBuf;

fn main() {
    // 内嵌字体：改动字体文件后要重新编译（Slint import 与 include_bytes! 都需要）
    for f in ["ui/assets/MiSans-Regular.ttf", "ui/assets/MiSans-Demibold.ttf"] {
        println!("cargo:rerun-if-changed={}", f);
    }
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR not set"));
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    // ---- 1. 生成图标 ----
    // PNG：供 Slint 的 Window.icon 使用（所有平台，尤其是 Linux/macOS 没有 exe 资源图标）
    let png = out_dir.join("appicon.png");
    if let Err(e) = icon::write_png(&png, 64) {
        panic!("生成 appicon.png 失败：{}", e);
    }

    // Windows：生成 ICO 并用 .rc 资源嵌入 exe
    if target_os == "windows" {
        let ico = out_dir.join("appicon.ico");
        if let Err(e) = icon::write_ico(&ico) {
            panic!("生成 appicon.ico 失败：{}", e);
        }
        let rc = out_dir.join("appicon.rc");
        let esc = ico.display().to_string().replace('\\', "\\\\");
        std::fs::write(&rc, format!("1 ICON \"{}\"\n", esc))
            .expect("写入 appicon.rc 失败");

        // embed-resource 仅在 Windows 目标下参与构建（见 Cargo.toml 的 target 段）
        #[cfg(target_os = "windows")]
        embed_resource::compile(&rc, embed_resource::NONE);
    }

    // ---- 2. 编译 Slint 界面 ----
    // 把 OUT_DIR 加入搜索路径，使 ui/main.slint 中的 @image-url("appicon.png") 能找到生成的图标
    let config = slint_build::CompilerConfiguration::new()
        .with_include_paths(vec![out_dir]);
    slint_build::compile_with_config("ui/main.slint", config).expect("编译 Slint 界面失败");

    println!("cargo:rerun-if-changed=ui/main.slint");
    println!("cargo:rerun-if-changed=build/icon.rs");
    println!("cargo:rerun-if-changed=build.rs");
}
