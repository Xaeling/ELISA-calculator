//! 检测操作系统当前的明暗主题偏好（不引入第三方依赖）。
//!
//! - Windows：读注册表 `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\AppsUseLightTheme`
//!   （0 = 深色，1 = 浅色）。用 `reg.exe` 查询并隐藏控制台窗口。
//! - macOS：`defaults read -g AppleInterfaceStyle`，输出含 `Dark` 即深色。
//! - 其他 / 查询失败：返回 `None`，调用方保持上一次结果。

/// 系统是否偏好深色主题；无法判断时返回 None。
pub fn prefers_dark() -> Option<bool> {
    #[cfg(windows)]
    {
        windows_prefers_dark()
    }
    #[cfg(target_os = "macos")]
    {
        macos_prefers_dark()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        linux_prefers_dark()
    }
}

#[cfg(windows)]
fn windows_prefers_dark() -> Option<bool> {
    use std::os::windows::process::CommandExt;

    // CREATE_NO_WINDOW：避免查询时闪出黑色控制台窗口
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let out = std::process::Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "/v",
            "AppsUseLightTheme",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;

    let txt = String::from_utf8_lossy(&out.stdout).to_string();
    for line in txt.lines() {
        if !line.contains("AppsUseLightTheme") {
            continue;
        }
        // 形如：    AppsUseLightTheme    REG_DWORD    0x0
        let tok = line.split_whitespace().last()?;
        let hex = tok.trim_start_matches("0x");
        let v = u32::from_str_radix(hex, 16).ok()?;
        return Some(v == 0); // AppsUseLightTheme == 0 → 深色
    }
    None
}

#[cfg(target_os = "macos")]
fn macos_prefers_dark() -> Option<bool> {
    let out = std::process::Command::new("defaults")
        .args(["read", "-g", "AppleInterfaceStyle"])
        .output()
        .ok()?;
    // 浅色时 defaults 返回非零退出码且无输出
    let txt = String::from_utf8_lossy(&out.stdout).to_lowercase();
    if txt.contains("dark") {
        Some(true)
    } else if out.status.success() && txt.trim().is_empty() {
        Some(false)
    } else {
        None
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn linux_prefers_dark() -> Option<bool> {
    // GTK 设置：gtk-application-prefer-dark-theme
    let out = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "gtk-theme"])
        .output()
        .ok()?;
    let txt = String::from_utf8_lossy(&out.stdout).to_lowercase();
    if txt.contains("dark") {
        Some(true)
    } else if out.status.success() {
        Some(false)
    } else {
        None
    }
}
