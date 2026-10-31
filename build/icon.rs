//! 程序图标生成：纯代码光栅化，不依赖任何外部图片素材。
//!
//! 设计（ELISACalc 回归/拟合计算程序）
//! ```text
//!   ┌───────────────────────┐
//!   │  ┌─                   │   蓝色渐变圆角方块（#4a9eff → #0b4a9e）
//!   │  │ ●                  │   白色 S 形 logistic 拟合曲线
//!   │  │   ■                │   曲线上 3 个正方形数据点（呼应软件内方形点标记）
//!   │  │      ■             │   淡白色 L 形坐标轴
//!   │  └────────■           │
//!   └───────────────────────┘
//! ```
//! 输出：
//! - `appicon.ico`：多尺寸（16~256）Windows 图标资源，DIB(BGRA) 格式
//! - `appicon.png`：64×64，供 Slint 的 `Window.icon` 使用（Linux/macOS 窗口图标）
//!
//! ICO / PNG 容器均为手写字节拼接，不引入任何第三方 crate。

use std::path::Path;

// ---------------- 配色 ----------------

/// 渐变顶部 #4a9eff
const TOP: [f64; 3] = [0x4a as f64 / 255.0, 0x9e as f64 / 255.0, 0xff as f64 / 255.0];
/// 渐变底部 #0b4a9e
const BOT: [f64; 3] = [0x0b as f64 / 255.0, 0x4a as f64 / 255.0, 0x9e as f64 / 255.0];
/// 曲线 / 数据点：纯白
const FG: [f64; 3] = [1.0, 1.0, 1.0];

/// 超采样倍数（抗锯齿）
const SS: usize = 3;

/// ICO 中包含的尺寸
const SIZES: &[usize] = &[16, 24, 32, 48, 64, 128, 256];

// ---------------- 光栅化 ----------------

/// 超采样画布：内部尺寸为 n*SS，每个通道以 f64 累加，最后盒式降采样
struct Canvas {
    n: usize,
    w: usize,
    buf: Vec<[f64; 4]>,
}

impl Canvas {
    fn new(n: usize) -> Self {
        let w = n * SS;
        Self { n, w, buf: vec![[0.0; 4]; w * w] }
    }

    /// 在超采样像素坐标 (x, y) 处以 alpha 混合颜色
    fn blend_raw(&mut self, x: i64, y: i64, c: [f64; 4]) {
        if x < 0 || y < 0 {
            return;
        }
        let (x, y) = (x as usize, y as usize);
        if x >= self.w || y >= self.w {
            return;
        }
        let a = c[3].clamp(0.0, 1.0);
        if a <= 0.0 {
            return;
        }
        let p = &mut self.buf[y * self.w + x];
        for k in 0..3 {
            p[k] = p[k] * (1.0 - a) + c[k] * a;
        }
        p[3] = p[3] * (1.0 - a) + a;
    }

    /// 抗锯齿实心圆（n 空间半径）
    fn disc(&mut self, cx: f64, cy: f64, rad: f64, c: [f64; 4]) {
        let (r, cx, cy) = (rad * SS as f64, cx * SS as f64, cy * SS as f64);
        let x0 = (cx - r).floor() as i64 - 1;
        let x1 = (cx + r).ceil() as i64 + 1;
        let y0 = (cy - r).floor() as i64 - 1;
        let y1 = (cy + r).ceil() as i64 + 1;
        for y in y0..y1 {
            for x in x0..x1 {
                let dx = x as f64 + 0.5 - cx;
                let dy = y as f64 + 0.5 - cy;
                let d = (dx * dx + dy * dy).sqrt();
                let a = (r - d + 0.5).clamp(0.0, 1.0);
                if a > 0.0 {
                    let mut cc = c;
                    cc[3] *= a;
                    self.blend_raw(x, y, cc);
                }
            }
        }
    }

    /// 抗锯齿实心矩形（n 空间，中心 + 半宽半高）
    fn rect(&mut self, cx: f64, cy: f64, hw: f64, hh: f64, c: [f64; 4]) {
        let (hw, hh, cx, cy) = (hw * SS as f64, hh * SS as f64, cx * SS as f64, cy * SS as f64);
        let x0 = (cx - hw).floor() as i64 - 1;
        let x1 = (cx + hw).ceil() as i64 + 1;
        let y0 = (cy - hh).floor() as i64 - 1;
        let y1 = (cy + hh).ceil() as i64 + 1;
        for y in y0..y1 {
            for x in x0..x1 {
                let dx = (x as f64 + 0.5 - cx).abs();
                let dy = (y as f64 + 0.5 - cy).abs();
                let a = (hw - dx + 0.5).min(hh - dy + 0.5).clamp(0.0, 1.0);
                if a > 0.0 {
                    let mut cc = c;
                    cc[3] *= a;
                    self.blend_raw(x, y, cc);
                }
            }
        }
    }

    /// 抗锯齿线段（n 空间）
    fn line(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, width: f64, c: [f64; 4]) {
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len = (dx * dx + dy * dy).sqrt();
        let steps = ((len / (width * 0.25)).ceil() as i64).max(1);
        for i in 0..=steps {
            let t = i as f64 / steps as f64;
            self.disc(x0 + dx * t, y0 + dy * t, width / 2.0, c);
        }
    }

    /// 降采样为 RGBA8
    fn to_rgba(&self) -> Vec<u8> {
        let n = self.n;
        let inv = 1.0 / (SS * SS) as f64;
        let mut out = vec![0u8; n * n * 4];
        for y in 0..n {
            for x in 0..n {
                let mut acc = [0.0f64; 4];
                for dy in 0..SS {
                    for dx in 0..SS {
                        let s = self.buf[(y * SS + dy) * self.w + (x * SS + dx)];
                        for k in 0..4 {
                            acc[k] += s[k];
                        }
                    }
                }
                let i = (y * n + x) * 4;
                for k in 0..4 {
                    out[i + k] = (acc[k] * inv * 255.0).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
        out
    }
}

/// 点是否落在圆角矩形内（n 空间）
fn in_rrect(x: f64, y: f64, n: f64, r: f64) -> bool {
    if x < 0.0 || y < 0.0 || x > n || y > n {
        return false;
    }
    let cx = x.clamp(r, n - r);
    let cy = y.clamp(r, n - r);
    let dx = x - cx;
    let dy = y - cy;
    dx * dx + dy * dy <= r * r
}

fn lerp(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// logistic 曲线在参数 t∈[0,1] 处的归一化 y（0 顶 1 底），呈 S 形下降
fn curve_y(t: f64) -> f64 {
    let s = 1.0 / (1.0 + (-6.0 * (t - 0.5)).exp()); // 0.047 → 0.953
    0.18 + 0.64 * s
}

/// 渲染一个尺寸的图标，返回 RGBA8
fn render(n: usize) -> Vec<u8> {
    let nf = n as f64;
    let mut cv = Canvas::new(n);
    let radius = 0.24 * nf;

    // 1) 背景：圆角方块 + 垂直渐变
    for sy in 0..(n * SS) {
        for sx in 0..(n * SS) {
            let x = sx as f64 / SS as f64;
            let y = sy as f64 / SS as f64;
            if !in_rrect(x, y, nf, radius) {
                continue;
            }
            let c = lerp(TOP, BOT, (y / nf).clamp(0.0, 1.0));
            cv.blend_raw(sx as i64, sy as i64, [c[0], c[1], c[2], 1.0]);
        }
    }

    // 2) 左上角柔光，避免纯色过于扁平
    let hx = 0.28 * nf;
    let hy = 0.20 * nf;
    let hr = 1.05 * nf;
    for sy in 0..(n * SS) {
        for sx in 0..(n * SS) {
            let x = sx as f64 / SS as f64;
            let y = sy as f64 / SS as f64;
            if !in_rrect(x, y, nf, radius) {
                continue;
            }
            let d = ((x - hx).powi(2) + (y - hy).powi(2)).sqrt() / hr;
            let a = (1.0 - d).clamp(0.0, 1.0);
            if a > 0.0 {
                cv.blend_raw(sx as i64, sy as i64, [1.0, 1.0, 1.0, 0.14 * a * a]);
            }
        }
    }

    // 3) L 形坐标轴（淡白）
    let axis = [FG[0], FG[1], FG[2], 0.30];
    let (x0, x1) = (0.15 * nf, 0.91 * nf);
    let (y0, y1) = (0.16 * nf, 0.87 * nf);
    cv.line(x0, y1, x1, y1, 0.035 * nf, axis);
    cv.line(x0, y0, x0, y1, 0.035 * nf, axis);

    // 4) 拟合曲线（白色 S 形，粗）
    let fg = [FG[0], FG[1], FG[2], 1.0];
    let steps = 240;
    let mut prev: Option<(f64, f64)> = None;
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let px = x0 + (x1 - x0) * t;
        let py = y0 + (y1 - y0) * curve_y(t);
        if let Some((qx, qy)) = prev {
            cv.line(qx, qy, px, py, 0.125 * nf, fg);
        }
        prev = Some((px, py));
    }

    // 5) 数据点：正方形，呼应软件内的方形点标记
    for t in [0.16f64, 0.5, 0.84] {
        let px = x0 + (x1 - x0) * t;
        let py = y0 + (y1 - y0) * curve_y(t);
        let half = 0.085 * nf;
        cv.rect(px, py, half, half, fg);
    }

    cv.to_rgba()
}

// ---------------- ICO（DIB/BGRA 容器）----------------

/// 把 RGBA 转成一个 ICO 图像项的数据（BITMAPINFOHEADER + XOR + AND mask）
fn dib(rgba: &[u8], n: usize) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::with_capacity(40 + n * n * 4);
    // BITMAPINFOHEADER
    v.extend_from_slice(&40u32.to_le_bytes()); // biSize
    v.extend_from_slice(&(n as i32).to_le_bytes()); // biWidth
    v.extend_from_slice(&(2 * n as i32).to_le_bytes()); // biHeight（XOR + AND）
    v.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    v.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    v.extend_from_slice(&0u32.to_le_bytes()); // biCompression = BI_RGB
    v.extend_from_slice(&0u32.to_le_bytes()); // biSizeImage
    v.extend_from_slice(&0i32.to_le_bytes()); // biXPelsPerMeter
    v.extend_from_slice(&0i32.to_le_bytes()); // biYPelsPerMeter
    v.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
    v.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant

    // XOR：BGRA，自底向上
    for y in (0..n).rev() {
        for x in 0..n {
            let i = (y * n + x) * 4;
            v.push(rgba[i + 2]); // B
            v.push(rgba[i + 1]); // G
            v.push(rgba[i]); // R
            v.push(rgba[i + 3]); // A
        }
    }

    // AND mask：1bpp，每行补齐到 4 字节，自底向上；透明处置 1
    let row_bytes = ((n + 31) / 32) * 4;
    for y in (0..n).rev() {
        let mut row = vec![0u8; row_bytes];
        for x in 0..n {
            if rgba[(y * n + x) * 4 + 3] < 128 {
                row[x / 8] |= 0x80 >> (x % 8);
            }
        }
        v.extend_from_slice(&row);
    }
    v
}

/// 生成多尺寸 .ico
pub fn write_ico(path: &Path) -> std::io::Result<()> {
    let mut images: Vec<(usize, Vec<u8>)> = Vec::new();
    for &n in SIZES {
        images.push((n, dib(&render(n), n)));
    }

    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // type = icon
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());

    let mut offset = 6 + 16 * images.len();
    for (n, data) in &images {
        out.push(if *n >= 256 { 0 } else { *n as u8 }); // width
        out.push(if *n >= 256 { 0 } else { *n as u8 }); // height
        out.push(0); // color count
        out.push(0); // reserved
        out.extend_from_slice(&1u16.to_le_bytes()); // planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bit count
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += data.len();
    }
    for (_, data) in &images {
        out.extend_from_slice(data);
    }

    std::fs::write(path, out)
}

// ---------------- PNG（供 Slint Window.icon）----------------

static CRC_TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();

fn crc_table() -> &'static [u32; 256] {
    CRC_TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, item) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *item = c;
        }
        t
    })
}

fn crc32(bytes: &[u8]) -> u32 {
    let t = crc_table();
    let mut c = 0xffff_ffffu32;
    for &b in bytes {
        c = t[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
    }
    c ^ 0xffff_ffff
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in bytes {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// deflate：仅使用"存储块"（BTYPE=00），不压缩，实现极简
fn zlib_store(raw: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.push(0x78); // CMF: CM=8(deflate), CINFO=7
    out.push(0x01); // FLG: 无预设字典，校验位使 (CMF*256+FLG) % 31 == 1

    let mut pos = 0usize;
    while pos < raw.len() {
        let end = (pos + 65535).min(raw.len());
        let chunk = &raw[pos..end];
        let last = end >= raw.len();
        // 块头：1 bit BFINAL + 2 bit BTYPE(00)，字节内低位在前
        out.push(if last { 0x01 } else { 0x00 });
        out.extend_from_slice(&(chunk.len() as u16).to_le_bytes()); // LEN
        out.extend_from_slice(&(!(chunk.len() as u16)).to_le_bytes()); // NLEN
        out.extend_from_slice(chunk);
        pos = end;
    }
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

fn png_chunk(tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut body = Vec::new();
    body.extend_from_slice(tag);
    body.extend_from_slice(data);
    v.extend_from_slice(&body);
    v.extend_from_slice(&crc32(&body).to_be_bytes());
    v
}

/// 生成 RGBA PNG（8bit，色彩类型 6）
pub fn write_png(path: &Path, n: usize) -> std::io::Result<()> {
    let rgba = render(n);
    let mut raw = Vec::with_capacity(n * (n * 4 + 1));
    for y in 0..n {
        raw.push(0u8); // filter type: None
        let i = y * n * 4;
        raw.extend_from_slice(&rgba[i..i + n * 4]);
    }

    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(n as u32).to_be_bytes());
    ihdr.extend_from_slice(&(n as u32).to_be_bytes());
    ihdr.push(8); // bit depth
    ihdr.push(6); // color type RGBA
    ihdr.push(0); // compression
    ihdr.push(0); // filter
    ihdr.push(0); // interlace

    let mut out: Vec<u8> = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    out.extend_from_slice(&png_chunk(b"IHDR", &ihdr));
    out.extend_from_slice(&png_chunk(b"IDAT", &zlib_store(&raw)));
    out.extend_from_slice(&png_chunk(b"IEND", &[]));
    std::fs::write(path, out)
}
