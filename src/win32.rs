//! Windows 原生窗口操作：无边框窗口的**拖动**与**边缘缩放**。
//!
//! ## 为什么必须用 Win32
//!
//! `no-frame: true` 后，Slint 只能提供「鼠标在窗口内的坐标」。若据此自己
//! `set_position()` 移动窗口，窗口一动该坐标就被**抵消回原值**，下一帧算出位移为 0
//! → 窗口回弹 → 表现为拖动时**持续闪烁/抖动**。这是坐标反馈循环，纯 Slint 层无解。
//!
//! 正确做法是把按下事件交给 Windows 自己处理：
//!
//! ```text
//! ReleaseCapture();
//! SendMessageW(hwnd, WM_NCLBUTTONDOWN, HTCAPTION, 0);          // 移动
//! SendMessageW(hwnd, WM_NCLBUTTONDOWN, HTLEFT / HTTOPLEFT, 0);  // 八向缩放
//! ```
//!
//! `DefWindowProc` 随即进入原生的移动/缩放模态循环 —— 与拖动原生标题栏完全一致，
//! 平滑、不闪烁，且**无需任何窗口子类化**（比 `WM_NCHITTEST` 方案简单得多）。
//!
//! 这里**手写 FFI**，不引入第三方 crate：`user32` / `dwmapi` 均为系统 DLL，
//! 不影响「单文件免安装」，也不带来额外许可与版本兼容风险。

use std::sync::OnceLock;

pub type Hwnd = usize;

// ---- 常量 ----

const WM_NCLBUTTONDOWN: u32 = 0x00A1;

/// 移动窗口（标题栏）
pub const HTCAPTION: usize = 2;
// 八向缩放命中码
pub const HTLEFT: usize = 10;
pub const HTRIGHT: usize = 11;
pub const HTTOP: usize = 12;
pub const HTTOPLEFT: usize = 13;
pub const HTTOPRIGHT: usize = 14;
pub const HTBOTTOM: usize = 15;
pub const HTBOTTOMLEFT: usize = 16;
pub const HTBOTTOMRIGHT: usize = 17;

#[repr(C)]
struct Margins {
    cx_left: i32,
    cy_top: i32,
    cx_right: i32,
    cy_bottom: i32,
}

#[link(name = "user32")]
extern "system" {
    fn FindWindowW(class: *const u16, title: *const u16) -> Hwnd;
    fn SendMessageW(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> isize;
    fn IsWindow(hwnd: Hwnd) -> i32;
    fn ReleaseCapture() -> i32;
}

#[link(name = "dwmapi")]
extern "system" {
    fn DwmExtendFrameIntoClientArea(hwnd: Hwnd, margins: *const Margins) -> i32;
}

// ---- 句柄缓存 ----

static CACHE: OnceLock<std::sync::Mutex<Hwnd>> = OnceLock::new();

/// UTF-16 以 0 结尾
fn wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    let mut v: Vec<u16> = std::ffi::OsStr::new(s).encode_wide().collect();
    v.push(0);
    v
}

/// 取窗口句柄：优先用缓存；缓存失效（窗口已销毁）时按标题重新查找
fn hwnd(title: &str) -> Option<Hwnd> {
    let m = CACHE.get_or_init(|| std::sync::Mutex::new(0));
    let mut g = m.lock().unwrap_or_else(|e| e.into_inner());
    if *g != 0 && unsafe { IsWindow(*g) } != 0 {
        return Some(*g);
    }
    let w = wide(title);
    let h = unsafe { FindWindowW(std::ptr::null(), w.as_ptr()) };
    *g = h;
    if h == 0 {
        None
    } else {
        Some(h)
    }
}

// ---- 对外操作 ----

/// 开始拖动窗口（系统原生移动循环）。该调用是**同步**的，直到松开鼠标才返回。
pub fn start_drag(title: &str) -> bool {
    let Some(h) = hwnd(title) else {
        return false;
    };
    unsafe {
        // 必须先释放可能存在的鼠标捕获，否则系统不会进入移动循环
        ReleaseCapture();
        SendMessageW(h, WM_NCLBUTTONDOWN, HTCAPTION, 0);
    }
    true
}

/// 开始缩放窗口边缘 / 角（系统原生缩放循环）。`code` 取 HTLEFT..HTBOTTOMRIGHT。
pub fn start_resize(title: &str, code: usize) -> bool {
    if !(HTLEFT..=HTBOTTOMRIGHT).contains(&code) {
        return false;
    }
    let Some(h) = hwnd(title) else {
        return false;
    };
    unsafe {
        ReleaseCapture();
        SendMessageW(h, WM_NCLBUTTONDOWN, code, 0);
    }
    true
}

/// 为无边框窗口补上 DWM 阴影（底边扩展 1px 即可让 DWM 绘制投影）
pub fn enable_shadow(title: &str) {
    if let Some(h) = hwnd(title) {
        let m = Margins {
            cx_left: 0,
            cy_top: 0,
            cx_right: 0,
            cy_bottom: 1,
        };
        unsafe {
            DwmExtendFrameIntoClientArea(h, &m);
        }
    }
}
