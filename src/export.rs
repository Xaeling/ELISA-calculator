//! 把拟合曲线导出为图片文件（JPEG / PNG）。
//!
//! 不依赖 GUI，自己完成光栅化：网格、坐标轴、刻度、曲线、散点、图例、方程与参数文本。
//! 字体：**内嵌** Mi Sans（子集化后编译进二进制），不依赖系统安装。
//! 仅在极端情况下（内嵌字体解析失败）才回退系统字体；两者都没有时回退英文标签。

use ab_glyph::{Font, FontArc, PxScale, ScaleFont};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder};
use std::sync::OnceLock;

use crate::models::Model;

/// 导出图片格式
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImgFormat {
    Jpeg,
    Png,
}

impl ImgFormat {
    pub fn ext(&self) -> &'static str {
        match self {
            ImgFormat::Jpeg => "jpg",
            ImgFormat::Png => "png",
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            ImgFormat::Jpeg => "JPEG",
            ImgFormat::Png => "PNG",
        }
    }
}

/// 导出所需的全部信息
pub struct ExportSpec {
    pub name: String,
    pub model: Model,
    pub equation: String,
    pub params: Vec<(String, String)>,
    pub extras: Vec<(String, String)>,
    pub stats: String,
    pub x_label: String,
    pub y_label: String,
    pub notes: Vec<String>,
    /// 实测点 (X, Y)
    pub pts: Vec<(f64, f64)>,
    /// 拟合曲线
    pub curve: Vec<(f64, f64)>,
    pub log_x: bool,
    /// 分辨率倍率：1.0 → 宽 860px；4.47 → 宽 3840px(4K)
    pub scale: f64,
    /// R²：用于绘制可信度徽章（插值模型传 None，此时 R² 恒为 1 不代表拟合优度）
    pub r2: Option<f64>,
}

/// 版面基准宽度（逻辑像素）。所有版式常量都以此为基准，实际输出 = BASE_W × scale。
/// 因为所有图元（文字/线段/方块）都是按坐标程序化绘制的，整体等比放大即为
/// “矢量级”重绘 —— 4K/8K 下线条与文字依然锐利，不是位图拉伸。
const BASE_W: f64 = 860.0;
/// 安全上限（像素总数）：避免极端分辨率导致内存耗尽
const MAX_PIXELS: f64 = 96_000_000.0;

/// 导出分辨率预设：(目标宽度 px, 显示名)
pub const RES_PRESETS: [(u32, &'static str); 4] = [
    (1920, "1920 px（HD）"),
    (2560, "2560 px（2K）"),
    (3840, "3840 px（4K，推荐）"),
    (7680, "7680 px（8K）"),
];

// ---------------- 颜色 ----------------

const C_BG: [u8; 4] = [255, 255, 255, 255];
const C_PANEL: [u8; 4] = [247, 249, 252, 255];
const C_AXIS: [u8; 4] = [60, 70, 85, 255];
const C_GRID: [u8; 4] = [225, 229, 235, 255];
const C_TEXT: [u8; 4] = [30, 36, 46, 255];
const C_DIM: [u8; 4] = [110, 120, 135, 255];
const C_CURVE: [u8; 4] = [200, 40, 40, 255];
const C_DOT: [u8; 4] = [30, 90, 200, 255];
const C_ACCENT: [u8; 4] = [20, 90, 170, 255];
const C_GREEN: [u8; 4] = [10, 120, 70, 255];

// R² 可信度徽章配色（文字色 / 底色）
const BADGE_HI_FG: [u8; 4] = [255, 255, 255, 255];
const BADGE_HI_BG: [u8; 4] = [16, 138, 80, 255];    // 绿：可信度高
const BADGE_MID_FG: [u8; 4] = [255, 255, 255, 255];
const BADGE_MID_BG: [u8; 4] = [196, 116, 12, 255];  // 橙：中等
const BADGE_LO_FG: [u8; 4] = [255, 255, 255, 255];
const BADGE_LO_BG: [u8; 4] = [190, 45, 45, 255];    // 红：可信度低

/// 坐标轴格距：X 轴 50 一格，Y 轴 0.5 一格
const X_STEP: f64 = 50.0;
const Y_STEP: f64 = 0.5;

/// R² 可信度等级
#[derive(Clone, Copy, PartialEq)]
pub enum R2Grade {
    /// R² ≥ 0.99：可信度高
    High,
    /// 0.95 ≤ R² < 0.99：中等
    Mid,
    /// R² < 0.95：可信度低
    Low,
}

/// 按 R² 判定可信度等级。
///
/// 阈值参照免疫分析（ELISA）常规质控习惯：
///   · ≥ 0.99  —— 标准曲线可用，反算浓度可信；
///   · ≥ 0.95  —— 勉强可用，建议检查离群点；
///   · < 0.95  —— 不可信，应检查数据 / 更换模型。
pub fn r2_grade(r2: f64) -> R2Grade {
    if !r2.is_finite() {
        R2Grade::Low
    } else if r2 >= 0.99 {
        R2Grade::High
    } else if r2 >= 0.95 {
        R2Grade::Mid
    } else {
        R2Grade::Low
    }
}

/// 徽章文案（中文；无中文字体时由调用方改用英文）
pub fn r2_label(g: R2Grade, cn: bool) -> &'static str {
    match (g, cn) {
        (R2Grade::High, true) => "可信度高",
        (R2Grade::High, false) => "HIGH CONFIDENCE",
        (R2Grade::Mid, true) => "可信度中等",
        (R2Grade::Mid, false) => "MODERATE",
        (R2Grade::Low, true) => "可信度低",
        (R2Grade::Low, false) => "LOW CONFIDENCE",
    }
}

/// 徽章配色：(文字, 底色)
pub fn r2_colors(g: R2Grade) -> ([u8; 4], [u8; 4]) {
    match g {
        R2Grade::High => (BADGE_HI_FG, BADGE_HI_BG),
        R2Grade::Mid => (BADGE_MID_FG, BADGE_MID_BG),
        R2Grade::Low => (BADGE_LO_FG, BADGE_LO_BG),
    }
}

/// 对应的提示语（用于导出图与结果文本）
pub fn r2_advice(g: R2Grade, cn: bool) -> &'static str {
    match (g, cn) {
        (R2Grade::High, true) => "拟合优度良好，标准曲线可用于反算浓度 / 剂量。",
        (R2Grade::High, false) => "Good fit; the curve is suitable for back-calculation.",
        (R2Grade::Mid, true) => "拟合一般，建议检查是否有离群点或考虑更换模型。",
        (R2Grade::Mid, false) => "Moderate fit; check outliers or consider another model.",
        (R2Grade::Low, true) => "拟合较差，不建议用于反算：请检查数据录入、本底与模型选择。",
        (R2Grade::Low, false) => "Poor fit; not recommended for back-calculation.",
    }
}

// ---------------- 文本辅助 ----------------

/// 把 Unicode 上标字符映射回基础字符；非上标返回 None。
///
/// 覆盖 `¹²³⁰⁴⁵⁶⁷⁸⁹` 与 `⁺⁻⁼⁽⁾ⁿⁱ`，以及下标 `₀₋₉`。
/// 这些字符在中文字体里普遍缺字形，必须自己绘制。
fn sup_base(ch: char) -> Option<char> {
    let c = match ch {
        '\u{00B2}' => '2',   // ²
        '\u{00B3}' => '3',   // ³
        '\u{00B9}' => '1',   // ¹
        '\u{2070}' => '0',   // ⁰
        '\u{2074}' => '4',   // ⁴
        '\u{2075}' => '5',   // ⁵
        '\u{2076}' => '6',   // ⁶
        '\u{2077}' => '7',   // ⁷
        '\u{2078}' => '8',   // ⁸
        '\u{2079}' => '9',   // ⁹
        '\u{207A}' => '+',   // ⁺
        '\u{207B}' => '-',   // ⁻ (U+207B)
        '\u{207C}' => '=',   // ⁼
        '\u{207D}' => '(',   // ⁽
        '\u{207E}' => ')',   // ⁾
        '\u{207F}' => 'n',   // ⁿ
        '\u{2071}' => 'i',   // ⁱ
        _ => return None,
    };
    Some(c)
}

// ---------------- 画布 ----------------

struct Canvas {
    w: u32,
    h: u32,
    px: Vec<[u8; 4]>,
}

impl Canvas {
    fn new(w: u32, h: u32, bg: [u8; 4]) -> Self {
        Self {
            w,
            h,
            px: vec![bg; (w as usize) * (h as usize)],
        }
    }

    fn blend(&mut self, x: i32, y: i32, c: [u8; 4], a: f64) {
        if x < 0 || y < 0 {
            return;
        }
        let (x, y) = (x as u32, y as u32);
        if x >= self.w || y >= self.h {
            return;
        }
        let a = a.clamp(0.0, 1.0);
        if a <= 0.0 {
            return;
        }
        let a = a as f32;
        let i = (y * self.w + x) as usize;
        for k in 0..3 {
            let old = self.px[i][k] as f32;
            self.px[i][k] = (old * (1.0 - a) + c[k] as f32 * a) as u8;
        }
        self.px[i][3] = 255;
    }

    fn fill_rect(&mut self, x0: i32, y0: i32, w: i32, h: i32, c: [u8; 4]) {
        for y in y0..(y0 + h) {
            for x in x0..(x0 + w) {
                self.blend(x, y, c, 1.0);
            }
        }
    }

    fn rect_outline(&mut self, x0: i32, y0: i32, w: i32, h: i32, c: [u8; 4]) {
        for x in x0..(x0 + w) {
            self.blend(x, y0, c, 1.0);
            self.blend(x, y0 + h - 1, c, 1.0);
        }
        for y in y0..(y0 + h) {
            self.blend(x0, y, c, 1.0);
            self.blend(x0 + w - 1, y, c, 1.0);
        }
    }

    /// 抗锯齿实心圆
    fn disc(&mut self, cx: f64, cy: f64, r: f64, c: [u8; 4]) {
        let r = r.max(0.5);
        let (x0, x1) = ((cx - r).floor() as i32 - 1, (cx + r).ceil() as i32 + 1);
        let (y0, y1) = ((cy - r).floor() as i32 - 1, (cy + r).ceil() as i32 + 1);
        for y in y0..y1 {
            for x in x0..x1 {
                let d = ((x as f64 + 0.5 - cx).powi(2) + (y as f64 + 0.5 - cy).powi(2)).sqrt();
                let a = (r - d + 0.5).clamp(0.0, 1.0);
                if a > 0.0 {
                    self.blend(x, y, c, a);
                }
            }
        }
    }

    /// 抗锯齿正方形（实测点标记）
    fn square(&mut self, cx: f64, cy: f64, half: f64, c: [u8; 4]) {
        let (x0, x1) = ((cx - half).floor() as i32 - 1, (cx + half).ceil() as i32 + 1);
        let (y0, y1) = ((cy - half).floor() as i32 - 1, (cy + half).ceil() as i32 + 1);
        for y in y0..y1 {
            for x in x0..x1 {
                let dx = (x as f64 + 0.5 - cx).abs();
                let dy = (y as f64 + 0.5 - cy).abs();
                let d = dx.max(dy);
                let a = (half - d + 0.5).clamp(0.0, 1.0);
                if a > 0.0 {
                    self.blend(x, y, c, a);
                }
            }
        }
    }

    /// 抗锯齿线段。
    ///
    /// 用圆盘序列描出等宽圆头笔迹：步长按“半径的一半”推进，保证相邻圆盘充分
    /// 重叠（否则高分辨率下会出现串珠状断续）。圆盘边缘用 `(r - d + 0.5)` 做
    /// 1 物理像素的羽化，因此放大后线条边缘依旧平滑，不会出现锯齿。
    fn line(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, c: [u8; 4], w: f64) {
        let (dx, dy) = (x1 - x0, y1 - y0);
        let r = (w / 2.0).max(0.55);
        let dist = (dx * dx + dy * dy).sqrt();
        // 步长 = 半径的一半 → 相邻圆盘重叠约 50%
        let steps = ((dist / (r * 0.5)).ceil() as i32).max(1);
        for i in 0..=steps {
            let t = i as f64 / steps as f64;
            self.disc(x0 + dx * t, y0 + dy * t, r, c);
        }
    }

    /// 在 (x, y) 处绘制文本，y 为文本顶部；返回文本宽度。
    ///
    /// 支持**上标**：`² ³ ¹ ⁰ ⁴⁻⁹` 等 Unicode 上标字符会被绘制成
    /// 0.62 倍字号、基线上移的形式。
    ///
    /// 为什么要这样处理：这些字符在 simhei / 微软雅黑 等常见中文字体里**没有字形**，
    /// 直接渲染会得到小方块（缺字形时的默认回退显示）。与其依赖字体，不如自己画。
    fn text(&mut self, x: i32, y: i32, size: f32, s: &str, c: [u8; 4]) -> f64 {
        let Some(f) = font() else {
            return 0.0;
        };
        let scale = PxScale::from(size);
        let sf = f.as_scaled(scale);
        let baseline = y as f32 + sf.ascent();
        let mut cx = x as f32;

        for ch in s.chars() {
            // 上标：缩小字号 + 基线上移
            let (chs, cs, lift) = if let Some(b) = sup_base(ch) {
                (b, size * 0.62, size * 0.40)
            } else {
                (ch, size, 0.0)
            };
            let gs = PxScale::from(cs);
            let id = f.glyph_id(chs);
            let g = id.with_scale_and_position(gs, ab_glyph::point(cx, baseline - lift));
            cx += f.as_scaled(gs).h_advance(id);
            if let Some(o) = f.outline_glyph(g) {
                let bb = o.px_bounds();
                let ox = bb.min.x.round() as i32;
                let oy = bb.min.y.round() as i32;
                o.draw(|gx, gy, cov| {
                    self.blend(ox + gx as i32, oy + gy as i32, c, cov as f64);
                });
            }
        }
        (cx - x as f32) as f64
    }

    fn text_w(&self, size: f32, s: &str) -> f64 {
        match font() {
            Some(f) => s
                .chars()
                .map(|ch| {
                    if let Some(b) = sup_base(ch) {
                        f.as_scaled(PxScale::from(size * 0.62)).h_advance(f.glyph_id(b)) as f64
                    } else {
                        f.as_scaled(PxScale::from(size)).h_advance(f.glyph_id(ch)) as f64
                    }
                })
                .sum(),
            None => s.chars().count() as f64 * size as f64 * 0.6,
        }
    }

    /// 绘制一个圆角徽章（用于 R² 可信度标识），返回右边界 x
    fn badge(&mut self, x: i32, y: i32, size: f32, label: &str, bg: [u8; 4], fg: [u8; 4]) -> i32 {
        // 统一用 f64：tw 来自 text_w()->f64，而 size 是 f32，
        // Rust 不会在 f64 + f32 之间自动提升，必须显式转换。
        let pad = size as f64 * 0.45;
        let tw = self.text_w(size, label);
        let w = (tw + pad * 2.0).ceil() as i32;
        let h = (size as f64 * 1.55).ceil() as i32;
        self.fill_rect(x, y, w, h, bg);
        self.rect_outline(x, y, w, h, fg);
        // 文字垂直居中
        let ty = y + ((h as f64 - size as f64) / 2.0) as i32;
        self.text(x + pad as i32, ty, size, label, fg);
        x + w
    }

    fn to_rgb(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(self.px.len() * 3);
        for p in &self.px {
            v.push(p[0]);
            v.push(p[1]);
            v.push(p[2]);
        }
        v
    }
}

// ---------------- 字体 ----------------

static FONT: OnceLock<Option<FontArc>> = OnceLock::new();

fn font() -> Option<&'static FontArc> {
    FONT.get_or_init(load_font).as_ref()
}

pub fn has_cjk_font() -> bool {
    font().is_some()
}

/// 在系统字体目录里按**文件名关键字**查找字体。
///
/// 为什么需要：Mi Sans 是用户自行安装的字体，文件名不固定
/// （可能是 `MiSans-Regular.ttf` / `Mi Sans Demibold.otf` / `MiSans.ttc` 等），
/// 没法写死路径。所以扫描常见字体目录，匹配文件名含关键字的 ttf/otf/ttc。
fn find_font_by_keyword(keywords: &[&str]) -> Option<std::path::PathBuf> {
    let dirs: &[&str] = if cfg!(windows) {
        &["C:\\Windows\\Fonts"]
    } else if cfg!(target_os = "macos") {
        &["/System/Library/Fonts", "/Library/Fonts"]
    } else {
        &["/usr/share/fonts", "/usr/local/share/fonts"]
    };

    let mut all: Vec<std::path::PathBuf> = dirs
        .iter()
        .filter(|d| !d.is_empty())
        .map(std::path::PathBuf::from)
        .collect();
    // Windows：用户级字体目录（Win10+ 装在用户目录下时不进 C:\Windows\Fonts）
    if cfg!(windows) {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            all.push(std::path::Path::new(&local).join(r"Microsoft\Windows\Fonts"));
        }
        if let Ok(home) = std::env::var("USERPROFILE") {
            all.push(
                std::path::Path::new(&home)
                    .join(r"AppData\Local\Microsoft\Windows\Fonts"),
            );
        }
    } else if let Ok(home) = std::env::var("HOME") {
        all.push(std::path::Path::new(&home).join(".fonts"));
        all.push(std::path::Path::new(&home).join(".local/share/fonts"));
    }

    // 偏好顺序：Regular > Medium > Demibold > 其他（Regular 最适合正文与图表标注）
    let mut hits: Vec<std::path::PathBuf> = Vec::new();
    for dir in &all {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        for e in rd.flatten() {
            let path = e.path();
            let Some(ext) = path.extension().and_then(|x| x.to_str()) else {
                continue;
            };
            if !matches!(ext.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc") {
                continue;
            }
            let lower = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if keywords.iter().any(|k| lower.contains(k)) {
                hits.push(path);
            }
        }
    }
    if hits.is_empty() {
        return None;
    }
    hits.sort();
    // 优先挑名字里带 regular 的
    hits.iter()
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains("regular")
        })
        .cloned()
        .or_else(|| hits.into_iter().next())
}

/// 内嵌的 Mi Sans（子集化，1.6MB）。
///
/// 与界面 `ui/main.slint` 顶部的 `import "assets/MiSans-Regular.ttf"` 是同一份文件，
/// 这样导出图与界面字体一致，且**完全不依赖系统是否安装 Mi Sans**。
/// （编译期打进二进制，运行时不再读任何字体文件。）
const EMBEDDED_MISANS: &[u8] = include_bytes!("../ui/assets/MiSans-Regular.ttf");

fn load_font() -> Option<FontArc> {
    // ① 内嵌 Mi Sans（首选，保证与界面一致、零外部依赖）
    if let Ok(f) = FontArc::try_from_vec(EMBEDDED_MISANS.to_vec()) {
        return Some(f);
    }

    // ② 回退：内嵌加载失败时（不应发生）再找系统字体
    if let Some(p) = find_font_by_keyword(&["misans", "mi sans", "mi-sans"]) {
        if let Ok(b) = std::fs::read(&p) {
            if let Ok(f) = FontArc::try_from_vec(b) {
                return Some(f);
            }
        }
    }

    // ③ 回退：各平台常见中文字体
    const CANDIDATES: &[&str] = &[
        "C:\\Windows\\Fonts\\simhei.ttf",
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\msyhbd.ttc",
        "C:\\Windows\\Fonts\\simsun.ttc",
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Medium.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJKsc-Regular.otf",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/arphic/uming.ttc",
    ];
    for path in CANDIDATES {
        let p = std::path::Path::new(path);
        if !p.exists() {
            continue;
        }
        if let Ok(bytes) = std::fs::read(p) {
            if let Ok(f) = FontArc::try_from_vec(bytes) {
                return Some(f);
            }
        }
    }
    None
}

/// 有中文字体用中文，否则用英文（避免导出图里出现空白）
fn tr(cn: &'static str, en: &'static str) -> &'static str {
    if has_cjk_font() {
        cn
    } else {
        en
    }
}

// ---------------- 文本排版辅助 ----------------

/// 按最大宽度把长文本切成多行
fn wrap(cv: &Canvas, s: &str, size: f32, max_w: f64) -> Vec<String> {
    if s.is_empty() {
        return vec![String::new()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in s.chars() {
        let mut test = cur.clone();
        test.push(ch);
        if !cur.is_empty() && cv.text_w(size, &test) > max_w {
            lines.push(cur.clone());
            cur.clear();
            if ch == ' ' {
                continue;
            }
        }
        cur.push(ch);
    }
    lines.push(cur);
    lines
}

// ---------------- 主入口 ----------------

/// 绘制并保存图片，返回保存路径
pub fn export_plot(spec: &ExportSpec, fmt: ImgFormat) -> Result<std::path::PathBuf, String> {
    if spec.pts.is_empty() && spec.curve.is_empty() {
        return Err("没有可导出的曲线，请先拟合。".to_string());
    }

    let cv = draw(spec)?;
    let rgb = cv.to_rgb();
    let bytes = encode(&rgb, cv.w, cv.h, fmt)?;

    let dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let fname = format!("ELISA曲线_{}_{}.{}", sanitize(&spec.name), stamp(), fmt.ext());
    let path = dir.join(fname);
    std::fs::write(&path, bytes).map_err(|e| format!("写入文件失败：{}", e))?;
    Ok(path)
}

fn encode(rgb: &[u8], w: u32, h: u32, fmt: ImgFormat) -> Result<Vec<u8>, String> {
    match fmt {
        ImgFormat::Jpeg => {
            let mut buf = Vec::new();
            // 质量 98：导出图多为“曲线 + 文字”，高码率可显著减少文字边缘的振铃伪影
            let mut enc = JpegEncoder::new_with_quality(&mut buf, 98);
            enc.encode(rgb, w, h, ExtendedColorType::Rgb8)
                .map_err(|e| format!("JPEG 编码失败：{}", e))?;
            Ok(buf)
        }
        ImgFormat::Png => {
            let mut buf = Vec::new();
            PngEncoder::new(&mut buf)
                .write_image(rgb, w, h, ExtendedColorType::Rgb8)
                .map_err(|e| format!("PNG 编码失败：{}", e))?;
            Ok(buf)
        }
    }
}

fn sanitize(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        if matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\n' | '\r' | '\t')
        {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    let out = out.trim().to_string();
    if out.is_empty() {
        "标准曲线".to_string()
    } else {
        out
    }
}

/// 时间戳（本地时区按 UTC+8 计算，避免引入额外依赖）
fn stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
        + 8 * 3600;
    let days = secs / 86400;
    let sod = secs % 86400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        y,
        m,
        d,
        sod / 3600,
        (sod / 60) % 60,
        sod % 60
    )
}

/// Howard Hinnant 的 civil_from_days 算法（days since 1970-01-01）
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as i64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

// ---------------- 绘制 ----------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r2_grade_thresholds() {
        // 阈值：≥0.99 高，≥0.95 中，<0.95 低
        assert_eq!(r2_grade(0.9999), R2Grade::High);
        assert_eq!(r2_grade(0.99), R2Grade::High);
        assert_eq!(r2_grade(0.9899), R2Grade::Mid);
        assert_eq!(r2_grade(0.95), R2Grade::Mid);
        assert_eq!(r2_grade(0.9499), R2Grade::Low);
        assert_eq!(r2_grade(0.0), R2Grade::Low);
        // 非有限值按最低处理（不应 panic）
        assert_eq!(r2_grade(f64::NAN), R2Grade::Low);
        assert_eq!(r2_grade(f64::INFINITY), R2Grade::High);
    }

    #[test]
    fn r2_badge_colors_are_distinct() {
        let a = r2_colors(R2Grade::High).1;
        let b = r2_colors(R2Grade::Mid).1;
        let c = r2_colors(R2Grade::Low).1;
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
        // 高可信为绿（G 通道最大），低可信为红（R 通道最大）
        assert!(a[1] > a[0] && a[1] > a[2]);
        assert!(c[0] > c[1] && c[0] > c[2]);
    }

    #[test]
    fn superscript_chars_map_to_base() {
        // ² ³ ¹ 是最常用的上标，中文字体常缺字形 → 必须能映射回基础字符
        assert_eq!(sup_base('\u{00B2}'), Some('2'));
        assert_eq!(sup_base('\u{00B3}'), Some('3'));
        assert_eq!(sup_base('\u{00B9}'), Some('1'));
        assert_eq!(sup_base('\u{2070}'), Some('0'));
        assert_eq!(sup_base('\u{2079}'), Some('9'));
        assert_eq!(sup_base('\u{207B}'), Some('-'));
        assert_eq!(sup_base('\u{207F}'), Some('n'));
        // 普通字符不是上标
        assert_eq!(sup_base('2'), None);
        assert_eq!(sup_base('R'), None);
        assert_eq!(sup_base('中'), None);
    }

    #[test]
    fn superscript_narrows_measured_width() {
        // 上标应以缩小字号参与排版：R² 的宽度必须小于 R2
        if font().is_none() {
            return; // 无字体环境下跳过（仅比较相对关系）
        }
        let cv = Canvas::new(4, 4, C_BG);
        let w_sup = cv.text_w(20.0, "R\u{00B2}");
        let w_full = cv.text_w(20.0, "R2");
        assert!(
            w_sup < w_full,
            "上标应更窄：R²={} 应小于 R2={}",
            w_sup,
            w_full
        );
    }

    #[test]
    fn text_with_superscript_does_not_panic() {
        let mut cv = Canvas::new(200, 60, C_BG);
        // 即使字体缺失也应安全返回，不 panic
        let w = cv.text(5, 10, 16.0, "R\u{00B2} = 0.99691", C_TEXT);
        assert!(w >= 0.0);
    }
}

fn draw(spec: &ExportSpec) -> Result<Canvas, String> {
    // ---- 1. 先在「基准坐标系」里排版，测出需要的行数与总高度 ----
    let mut info_lines: Vec<(String, f32, [u8; 4])> = Vec::new(); // (文本, 基准字号, 颜色)
    info_lines.push((
        format!("{}：{}", tr("拟合方程", "Equation"), spec.equation),
        19.0,
        C_ACCENT,
    ));
    if !spec.params.is_empty() {
        let joined: Vec<String> = spec.params.iter().map(|(k, v)| format!("{} = {}", k, v)).collect();
        info_lines.push((
            format!("{}：{}", tr("参数", "Parameters"), joined.join("，  ")),
            15.0,
            C_TEXT,
        ));
    }
    for (k, v) in &spec.extras {
        info_lines.push((format!("{} = {}", k, v), 15.0, C_GREEN));
    }
    info_lines.push((spec.stats.clone(), 15.0, C_TEXT));
    // R² 可信度：结论语（徽章画在统计行右侧，见下方绘制段）
    if let Some(r2) = spec.r2 {
        let g = r2_grade(r2);
        info_lines.push((
            format!("→ {}", r2_advice(g, has_cjk_font())),
            14.0,
            r2_colors(g).1,
        ));
    }
    info_lines.push((
        format!("{}：{}", tr("模型", "Model"), spec.model.formula()),
        14.0,
        C_DIM,
    ));
    info_lines.push((
        format!(
            "{}：X = {}，Y = {}",
            tr("数据转换", "Transforms"),
            spec.x_label,
            spec.y_label
        ),
        14.0,
        C_DIM,
    ));
    for n in &spec.notes {
        info_lines.push((n.clone(), 14.0, C_DIM));
    }
    if spec.model.is_interpolation() {
        info_lines.push((
            tr(
                "该模型为插值，曲线严格通过每个数据点，R² 恒为 1。",
                "Interpolation: the curve passes through every data point; R² is always 1.",
            )
            .to_string(),
            14.0,
            C_DIM,
        ));
    }

    // 换行在基准坐标系里完成（用基准字号测量），保证不同分辨率下断行位置一致
    let probe = Canvas::new(1, 1, C_BG);
    let mut expanded: Vec<(String, f32, [u8; 4])> = Vec::new();
    for (text, size, c) in &info_lines {
        for line in wrap(&probe, text, *size, BASE_W - 80.0) {
            expanded.push((line, *size, *c));
        }
    }

    let head_h = 108.0f64;
    let plot_h = 420.0f64;
    let axis_label_h = 34.0f64;
    let info_h = 24.0 + expanded.len() as f64 * 24.0 + 30.0;
    let base_h = head_h + plot_h + axis_label_h + info_h + 40.0;

    // ---- 2. 决定实际倍率（必要时按像素总量回退，防止内存耗尽）----
    let mut s = spec.scale;
    if !s.is_finite() || s < 0.1 {
        s = 1.0;
    }
    let pixels = |sc: f64| BASE_W * sc * base_h * sc;
    if pixels(s) > MAX_PIXELS {
        s = (MAX_PIXELS / (BASE_W * base_h)).sqrt();
    }
    let fs = s as f32;

    let w = (BASE_W * s).round().max(1.0) as u32;
    let h = (base_h * s).round().max(1.0) as u32;

    // 长度 / 字号 缩放助手
    let px = |v: f64| v * s;
    let fz = |v: f32| v * fs;

    let mut cv = Canvas::new(w, h, C_BG);

    // ---- 页眉 ----
    let title = format!("{} — {}", spec.name, spec.model.label());
    cv.text(px(40.0) as i32, px(34.0) as i32, fz(27.0), &title, C_TEXT);
    cv.text(
        px(40.0) as i32,
        px(74.0) as i32,
        fz(15.0),
        &format!("{}：{}", tr("公式", "Formula"), spec.model.formula()),
        C_DIM,
    );
    cv.line(
        px(40.0),
        px(100.0),
        px(BASE_W - 40.0),
        px(100.0),
        C_GRID,
        px(1.5),
    );

    // ---- 绘图区（基准 700×420 横向）----
    let pl = (
        px(80.0) as i32,
        px(head_h) as i32,
        px(80.0 + 700.0) as i32,
        px(head_h + plot_h) as i32,
    );
    draw_plot(&mut cv, spec, pl, s);

    // ---- 轴标题 ----
    let xlab = format!(
        "{}  {}",
        spec.x_label,
        if spec.log_x {
            tr("（对数坐标）", "(log scale)")
        } else {
            ""
        }
    );
    let xw = cv.text_w(fz(16.0), &xlab);
    cv.text(
        ((px(BASE_W) - xw) / 2.0) as i32,
        pl.3 + px(8.0) as i32,
        fz(16.0),
        &xlab,
        C_AXIS,
    );

    // ---- 信息区 ----
    let mut y = pl.3 + px(axis_label_h) as i32 + px(18.0) as i32;
    cv.line(px(40.0), px(y as f64 / s - 10.0), px(BASE_W - 40.0), px(y as f64 / s - 10.0), C_GRID, px(1.5));
    // 统计行（含 R²）的下标：在其右侧追加可信度徽章
    let stats_row = info_lines
        .iter()
        .position(|(t, _, _)| *t == spec.stats)
        .and_then(|i| {
            // 该行可能因换行展开成多行，取其在 expanded 中的起始下标
            let before: usize = info_lines[..i]
                .iter()
                .map(|(t, sz, _)| wrap(&probe, t, *sz, BASE_W - 80.0).len())
                .sum();
            Some(before)
        });

    for (i, (text, size, c)) in expanded.iter().enumerate() {
        if !text.trim().is_empty() {
            cv.text(px(40.0) as i32, y, fz(*size), text, *c);

            // 在统计行右侧画 R² 可信度徽章
            if stats_row == Some(i) {
                if let Some(r2) = spec.r2 {
                    let g = r2_grade(r2);
                    let (fg, bg) = r2_colors(g);
                    let label = r2_label(g, has_cjk_font());
                    let tw = cv.text_w(fz(*size), text);
                    let bx = px(40.0) as i32 + tw as i32 + px(14.0) as i32;
                    // fz() 返回 f32，px() 返回 f64 —— 显式转 f64 再运算
                    let by = y + ((px(24.0) - fz(*size) as f64 * 1.55) / 2.0) as i32;
                    cv.badge(bx, by, fz(*size), label, bg, fg);
                }
            }
        }
        y += px(24.0) as i32;
    }

    // ---- 页脚 ----
    let foot = format!(
        "{} · {} {}",
        tr(
            "ELISACalc 回归/拟合计算程序",
            "ELISACalc Regression/Fitting"
        ),
        tr("导出时间", "Exported"),
        stamp()
    );
    cv.text(
        px(40.0) as i32,
        h as i32 - px(32.0) as i32,
        fz(13.0),
        &foot,
        C_DIM,
    );

    Ok(cv)
}

fn draw_plot(cv: &mut Canvas, spec: &ExportSpec, pl: (i32, i32, i32, i32), s: f64) {
    let (l, t, r, b) = (pl.0 as f64, pl.1 as f64, pl.2 as f64, pl.3 as f64);
    let px = |v: f64| v * s;
    let fz = |v: f32| v * s as f32;

    let use_log = spec.log_x && spec.pts.iter().all(|(x, _)| *x > 0.0);
    let fx = |x: f64| if use_log { x.log10() } else { x };
    let ok = |x: &f64, y: &f64| x.is_finite() && y.is_finite() && (!use_log || *x > 0.0);

    let mut vx: Vec<f64> = Vec::new();
    let mut vy: Vec<f64> = Vec::new();
    for (x, y) in &spec.pts {
        if ok(x, y) {
            vx.push(fx(*x));
            vy.push(*y);
        }
    }
    for (x, y) in &spec.curve {
        if ok(x, y) {
            vx.push(fx(*x));
            vy.push(*y);
        }
    }
    cv.fill_rect(pl.0, pl.1, pl.2 - pl.0, pl.3 - pl.1, C_PANEL);

    if vx.len() < 2 {
        let s_txt = tr("暂无有效数据", "No valid data");
        let sw = cv.text_w(fz(18.0), s_txt);
        cv.text(
            ((l + r) / 2.0 - sw / 2.0) as i32,
            ((t + b) / 2.0 - px(9.0)) as i32,
            fz(18.0),
            s_txt,
            C_DIM,
        );
        return;
    }

    let mut xmin = vx.iter().cloned().fold(f64::INFINITY, f64::min);
    let mut xmax = vx.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let mut ymin = vy.iter().cloned().fold(f64::INFINITY, f64::min);
    let mut ymax = vy.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if !((xmax - xmin).abs() > 0.0) {
        xmin -= 1.0;
        xmax += 1.0;
    }
    if !((ymax - ymin).abs() > 0.0) {
        ymin -= 1.0;
        ymax += 1.0;
    }
    let (xr, yr) = (xmax - xmin, ymax - ymin);
    xmin -= 0.06 * xr;
    xmax += 0.06 * xr;
    ymin -= 0.08 * yr;
    ymax += 0.08 * yr;

    let sx = |v: f64| l + (v - xmin) / (xmax - xmin) * (r - l);
    let sy = |v: f64| b - (v - ymin) / (ymax - ymin) * (b - t);

    // 网格 + 刻度：X 轴 50 一格，Y 轴 0.5 一格（格数过多时自动放大步长）
    let xtick = crate::models::axis_ticks(xmin, xmax, X_STEP, 40);
    let ytick = crate::models::axis_ticks(ymin, ymax, Y_STEP, 40);

    for v in &xtick {
        cv.line(sx(*v), t, sx(*v), b, C_GRID, px(1.0));
        let lab = if use_log {
            crate::models::fmt(10f64.powf(*v))
        } else {
            crate::models::fmt(*v)
        };
        let lw = cv.text_w(fz(13.0), &lab);
        cv.text(
            (sx(*v) - lw / 2.0) as i32,
            b as i32 + px(4.0) as i32,
            fz(13.0),
            &lab,
            C_DIM,
        );
    }
    for v in &ytick {
        cv.line(l, sy(*v), r, sy(*v), C_GRID, px(1.0));
        let lab = crate::models::fmt(*v);
        let lw = cv.text_w(fz(13.0), &lab);
        cv.text(
            (l - lw - px(8.0)) as i32,
            (sy(*v) - px(7.0)) as i32,
            fz(13.0),
            &lab,
            C_DIM,
        );
    }
    cv.rect_outline(pl.0, pl.1, pl.2 - pl.0, pl.3 - pl.1, C_AXIS);

    // 拟合曲线
    let mut prev: Option<(f64, f64)> = None;
    for (x, y) in &spec.curve {
        if !ok(x, y) {
            continue;
        }
        let (pxv, pyv) = (sx(fx(*x)), sy(*y).clamp(t, b));
        if let Some((qx, qy)) = prev {
            cv.line(qx, qy, pxv, pyv, C_CURVE, px(2.6));
        }
        prev = Some((pxv, pyv));
    }

    // 实测点（正方形，与界面绘图区一致）
    for (x, y) in &spec.pts {
        if !ok(x, y) {
            continue;
        }
        cv.square(sx(fx(*x)), sy(*y), px(4.2), C_DOT);
    }

    // 图例（右上角，白底小框）
    let legend_w = px(210.0) as i32;
    let legend_h = px(62.0) as i32;
    let lx = r as i32 - legend_w - px(14.0) as i32;
    let ly = t as i32 + px(14.0) as i32;
    cv.fill_rect(lx, ly, legend_w, legend_h, C_BG);
    cv.rect_outline(lx, ly, legend_w, legend_h, C_GRID);

    let t1 = tr("实测数据", "Measured");
    let t2 = tr("拟合曲线", "Fitted curve");
    cv.square(
        lx as f64 + px(22.0),
        ly as f64 + px(21.0),
        px(4.2),
        C_DOT,
    );
    cv.text(lx + px(36.0) as i32, ly + px(14.0) as i32, fz(15.0), t1, C_TEXT);
    cv.line(
        lx as f64 + px(10.0),
        ly as f64 + px(44.0),
        lx as f64 + px(34.0),
        ly as f64 + px(44.0),
        C_CURVE,
        px(2.6),
    );
    cv.text(lx + px(36.0) as i32, ly + px(34.0) as i32, fz(15.0), t2, C_TEXT);
}
