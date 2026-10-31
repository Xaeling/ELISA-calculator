//! 回归/拟合模型：直线、二次、三次、Logit-log、Logistic(1/2/3)、Hill、指数、三次样条插值

use crate::fitting::{self, lm_fit, mean, natural_cubic_spline, poly_fit, quality, FitQuality};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Model {
    /// 直线回归 y = a + b x
    Linear,
    /// 二次曲线回归
    Quadratic,
    /// 三次曲线回归
    Cubic,
    /// Logit-log 直线回归（ELISA 常用）
    LogitLog,
    /// Logistic 曲线拟合 1：四参数（浓度幂形式）
    Logistic1,
    /// Logistic 曲线拟合 2：五参数（含不对称因子 G）
    Logistic2,
    /// Logistic 曲线拟合 3：四参数（指数形式）
    Logistic3,
    /// Hill 曲线拟合
    Hill,
    /// 指数曲线拟合 y = a·e^(bx) + c
    Exponential,
    /// 三次样条插值
    CubicSpline,
}

impl Model {
    pub const ALL: [Model; 10] = [
        Model::Linear,
        Model::Quadratic,
        Model::Cubic,
        Model::LogitLog,
        Model::Logistic1,
        Model::Logistic2,
        Model::Logistic3,
        Model::Hill,
        Model::Exponential,
        Model::CubicSpline,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Model::Linear => "直线回归",
            Model::Quadratic => "二次曲线回归",
            Model::Cubic => "三次曲线回归",
            Model::LogitLog => "Logit-log直线回归",
            Model::Logistic1 => "Logistic曲线拟合1（四参数）",
            Model::Logistic2 => "Logistic曲线拟合2（五参数）",
            Model::Logistic3 => "Logistic曲线拟合3（指数四参数）",
            Model::Hill => "Hill曲线拟合",
            Model::Exponential => "指数曲线拟合",
            Model::CubicSpline => "三次样条插值",
        }
    }

    /// 模型表达式（帮助窗口中展示）
    pub fn formula(&self) -> &'static str {
        match self {
            Model::Linear => "Y = a + b·X",
            Model::Quadratic => "Y = a0 + a1·X + a2·X²",
            Model::Cubic => "Y = a0 + a1·X + a2·X² + a3·X³",
            Model::LogitLog => "logit(p) = ln[p/(1-p)] = a + b·log10(X)",
            Model::Logistic1 => "Y = D + (A - D) / [1 + (X/C)^B]",
            Model::Logistic2 => "Y = D + (A - D) / [1 + (X/C)^B]^G",
            Model::Logistic3 => "Y = D + (A - D) / [1 + e^(-B·(X - C))]",
            Model::Hill => "Y = Bottom + (Top - Bottom)·X^n / (k^n + X^n)",
            Model::Exponential => "Y = a·e^(b·X) + c",
            Model::CubicSpline => "分段三次多项式（自然边界，通过全部数据点）",
        }
    }

    /// 说明文字
    pub fn note(&self) -> &'static str {
        match self {
            Model::Linear | Model::Quadratic | Model::Cubic => "最小二乘（Householder QR）求解，X 需互不相同。",
            Model::LogitLog => "要求 X>0；Y 为结合率/抑制率，若最大值落在 (1,100] 则按百分比处理。常用于 ELISA 半对数坐标线性化，输出 ED50。",
            Model::Logistic1 => "要求 X>0。A=低浓度端渐近值，D=高浓度端渐近值，C=拐点(ED50)，B=斜率因子（负值表示曲线上升）。",
            Model::Logistic2 => "要求 X>0。在四参数基础上增加不对称因子 G，可拟合非对称 S 形曲线。",
            Model::Logistic3 => "以 X 原值（非对数）进入指数项的四参数 Logistic，X 可取任意实数。",
            Model::Hill => "要求 X>0。k=半效浓度(ED50)，n=Hill 系数（>1 正协同，<1 负协同）。",
            Model::Exponential => "三参数指数模型，自动多起点搜索初值。",
            Model::CubicSpline => "插值而非回归：曲线严格通过每个数据点，要求 X 严格递增且无重复，至少需要 3 个点。",
        }
    }

    pub fn min_points(&self) -> usize {
        match self {
            Model::Linear => 3,
            Model::Quadratic => 4,
            Model::Cubic => 5,
            Model::LogitLog => 4,
            Model::Logistic1 => 5,
            Model::Logistic2 => 6,
            Model::Logistic3 => 5,
            Model::Hill => 5,
            Model::Exponential => 4,
            Model::CubicSpline => 3,
        }
    }

    /// 该模型是否要求 X>0。
    /// 只有 Logit-log 真正依赖 log10(X)，X=0 无定义；
    /// Logistic（1/2/3）、Hill、指数在 X=0 处均有定义，不剔除零点。
    pub fn requires_positive_x(&self) -> bool {
        matches!(self, Model::LogitLog)
    }

    /// 是否属于插值（不做回归统计意义上的 R² 评价）
    pub fn is_interpolation(&self) -> bool {
        matches!(self, Model::CubicSpline)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CurveKind {
    Logistic4,
    Logistic5,
    LogisticExp,
    Hill,
    Exp3,
}

fn clamp_exp(e: f64) -> f64 {
    if !e.is_finite() {
        if e > 0.0 {
            f64::INFINITY
        } else {
            0.0
        }
    } else if e > 700.0 {
        f64::INFINITY
    } else if e < -700.0 {
        0.0
    } else {
        e.exp()
    }
}

/// 非线性曲线求值。为保证参数恒正（C、k、n、G），这些量在参数向量中以自然对数形式存放。
pub fn curve_value(kind: CurveKind, x: f64, p: &[f64]) -> f64 {
    match kind {
        CurveKind::Logistic4 => {
            let (a, d, b) = (p[0], p[1], p[3]);
            if x <= 0.0 {
                return if b > 0.0 { a } else { d };
            }
            let e = clamp_exp(b * (x.ln() - p[2]));
            d + (a - d) / (1.0 + e)
        }
        CurveKind::Logistic5 => {
            let (a, d, b) = (p[0], p[1], p[3]);
            let g = clamp_exp(p[4]);
            if x <= 0.0 {
                return if b > 0.0 { a } else { d };
            }
            let e = clamp_exp(b * (x.ln() - p[2]));
            let t = clamp_exp(g * (1.0 + e).ln());
            d + (a - d) / t
        }
        CurveKind::LogisticExp => {
            let (a, d, c, b) = (p[0], p[1], p[2], p[3]);
            let e = clamp_exp(-b * (x - c));
            d + (a - d) / (1.0 + e)
        }
        CurveKind::Hill => {
            let (bottom, top) = (p[0], p[1]);
            let n = clamp_exp(p[3]);
            if x <= 0.0 {
                return bottom;
            }
            let e = clamp_exp(n * (p[2] - x.ln()));
            bottom + (top - bottom) / (1.0 + e)
        }
        CurveKind::Exp3 => p[0] * clamp_exp(p[1] * x) + p[2],
    }
}

/// 拟合结果对应的求值器
#[derive(Clone, Debug)]
pub enum Evaluator {
    Poly(Vec<f64>),
    LogitLog { a: f64, b: f64, scale: f64 },
    Curve { kind: CurveKind, p: Vec<f64> },
    Spline(fitting::Spline),
}

impl Evaluator {
    pub fn eval(&self, x: f64) -> Option<f64> {
        let v = match self {
            Evaluator::Poly(c) => fitting::poly_eval(c, x),
            Evaluator::LogitLog { a, b, scale } => {
                if x <= 0.0 {
                    return None;
                }
                let u = a + b * x.log10();
                let e = clamp_exp(-u);
                scale / (1.0 + e)
            }
            Evaluator::Curve { kind, p } => curve_value(*kind, x, p),
            Evaluator::Spline(sp) => fitting::spline_eval(sp, x),
        };
        if v.is_finite() {
            Some(v)
        } else {
            None
        }
    }

    /// 解析反函数：由 Y 直接解出 X。仅对单调模型可靠，无法解析时返回 None。
    pub fn invert(&self, y: f64) -> Option<f64> {
        let v = match self {
            // 直线可解析；二次及以上交给数值求解（可能存在多解）
            Evaluator::Poly(c) => {
                if c.len() != 2 || c[1].abs() < 1e-15 {
                    return None;
                }
                (y - c[0]) / c[1]
            }
            // Y = scale / (1 + e^-u)，u = a + b·log10(X)
            Evaluator::LogitLog { a, b, scale } => {
                if b.abs() < 1e-15 || y <= 0.0 || y >= *scale {
                    return None;
                }
                let u = -(scale / y - 1.0).ln();
                10f64.powf((u - a) / b)
            }
            Evaluator::Curve { kind, p } => match kind {
                // Y = D + (A-D)/(1 + (X/C)^B)
                CurveKind::Logistic4 => {
                    let (a, d, b) = (p[0], p[1], p[3]);
                    let t = (a - d) / (y - d) - 1.0;
                    if t <= 0.0 || b.abs() < 1e-15 {
                        return None;
                    }
                    p[2].exp() * t.powf(1.0 / b)
                }
                // Y = D + (A-D)/(1 + (X/C)^B)^G
                CurveKind::Logistic5 => {
                    let (a, d, b) = (p[0], p[1], p[3]);
                    let g = p[4].exp();
                    let r = (a - d) / (y - d);
                    if r <= 0.0 || g.abs() < 1e-15 || b.abs() < 1e-15 {
                        return None;
                    }
                    let t = r.powf(1.0 / g) - 1.0;
                    if t <= 0.0 {
                        return None;
                    }
                    p[2].exp() * t.powf(1.0 / b)
                }
                // Y = D + (A-D)/(1 + e^(-B(X-C)))
                CurveKind::LogisticExp => {
                    let (a, d, b) = (p[0], p[1], p[3]);
                    let u = (a - d) / (y - d) - 1.0;
                    if u <= 0.0 || b.abs() < 1e-15 {
                        return None;
                    }
                    p[2] - u.ln() / b
                }
                // Y = Bottom + (Top-Bottom)/(1 + (k/X)^n)
                CurveKind::Hill => {
                    let (bottom, top) = (p[0], p[1]);
                    let n = p[3].exp();
                    let u = (top - bottom) / (y - bottom) - 1.0;
                    if u <= 0.0 || n.abs() < 1e-15 {
                        return None;
                    }
                    p[2].exp() / u.powf(1.0 / n)
                }
                // Y = a·e^(bX) + c
                CurveKind::Exp3 => {
                    let (a, b, c) = (p[0], p[1], p[2]);
                    let u = (y - c) / a;
                    if u <= 0.0 || b.abs() < 1e-15 {
                        return None;
                    }
                    u.ln() / b
                }
            },
            Evaluator::Spline(_) => return None,
        };
        if v.is_finite() {
            Some(v)
        } else {
            None
        }
    }
}

/// 生成固定步长的坐标轴刻度值（按整数索引计算，避免浮点累加误差）。
///
/// - `step`：期望的格距（如 X 轴 50、Y 轴 0.5）
/// - 刻度落在 `[min, max]` 区间内且为 step 的整数倍
/// - 若按 `step` 算出的格数过多（> `max_lines`），自动放大为 step 的 10^k 倍，
///   既保持“50 一格 / 0.5 一格”的观感，又不会画上百条网格线
pub fn axis_ticks(min: f64, max: f64, step: f64, max_lines: usize) -> Vec<f64> {
    if !(min.is_finite() && max.is_finite()) || step <= 0.0 || !(max > min) {
        return Vec::new();
    }
    let mut step = step;
    loop {
        let count = ((max - min) / step).ceil() as i64 + 1;
        if count <= max_lines as i64 {
            break;
        }
        step *= 10.0;
        if !step.is_finite() {
            return Vec::new();
        }
    }
    let i0 = (min / step).ceil();
    let i1 = (max / step).floor();
    if i1 < i0 {
        return Vec::new();
    }
    let (i0, i1) = (i0 as i64, i1 as i64);
    if (i1 - i0 + 1) as usize > max_lines * 2 {
        return Vec::new();
    }
    (i0..=i1).map(|i| i as f64 * step).collect()
}

/// 由 Y 反算 X：先用解析反函数，不行再在 X 取值范围内数值扫描（可找出多解）。
pub fn solve_x(r: &FitResult, y: f64) -> Vec<f64> {
    // 非单调模型（二次及以上、样条）走数值扫描，避免漏根
    let analytic = !matches!(
        &r.evaluator,
        Evaluator::Spline(_) | Evaluator::Poly(_)
    ) || matches!(&r.evaluator, Evaluator::Poly(c) if c.len() == 2);

    let mut roots: Vec<f64> = Vec::new();
    if analytic {
        if let Some(x) = r.evaluator.invert(y) {
            let ok = !r.model.requires_positive_x() || x > 0.0;
            if ok {
                roots.push(x);
            }
        }
    }
    if roots.is_empty() {
        roots = scan_roots(r, y);
    }
    // 去重（相对容差）
    let mut out: Vec<f64> = Vec::new();
    for x in roots {
        if !out.iter().any(|o| (o - x).abs() <= 1e-6 * (1.0 + o.abs().max(x.abs()))) {
            out.push(x);
        }
    }
    out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// 在 X 范围内扫描 f(x) - y 的符号变化并二分求根（可返回多个解）
fn scan_roots(r: &FitResult, y: f64) -> Vec<f64> {
    let (x0, x1) = r.domain;
    let span = (x1 - x0).abs();
    let (mut lo, hi) = if span <= 0.0 {
        (x0 - 1.0, x1 + 1.0)
    } else {
        (x0 - 0.5 * span, x1 + 0.5 * span)
    };
    if r.model.requires_positive_x() {
        lo = lo.max(1e-12);
    }
    if !(hi > lo) {
        return Vec::new();
    }

    let g = |x: f64| match r.evaluator.eval(x) {
        Some(v) => v - y,
        None => f64::NAN,
    };
    let n = 2000usize;
    let mut out: Vec<f64> = Vec::new();
    let mut prev_x = lo;
    let mut prev = g(lo);
    if prev == 0.0 {
        out.push(lo);
    }
    for i in 1..=n {
        let x = lo + (hi - lo) * i as f64 / n as f64;
        let cur = g(x);
        if cur == 0.0 {
            out.push(x);
        } else if prev.is_finite() && cur.is_finite() && (prev > 0.0) != (cur > 0.0) {
            // 二分细化
            let (mut a, mut b) = (prev_x, x);
            let mut fa = prev;
            for _ in 0..80 {
                let m = 0.5 * (a + b);
                let fm = g(m);
                if !fm.is_finite() {
                    break;
                }
                if (fa > 0.0) != (fm > 0.0) {
                    b = m;
                } else {
                    a = m;
                    fa = fm;
                }
            }
            out.push(0.5 * (a + b));
        }
        prev_x = x;
        prev = cur;
    }
    out
}

pub struct FitResult {
    /// 拟合所用模型（供导出 / 结果展示取用）
    #[allow(dead_code)]
    pub model: Model,
    pub evaluator: Evaluator,
    /// 参数名与数值
    pub params: Vec<(String, f64)>,
    /// 代入数值后的方程文本
    pub equation: String,
    /// 附加信息（如 ED50、转换说明）
    pub extra: Vec<(String, String)>,
    pub sse: f64,
    pub r2: f64,
    pub rmse: f64,
    pub n: usize,
    /// 用于绘图的拟合曲线
    pub curve: Vec<(f64, f64)>,
    /// (X, Y实测, Y拟合)
    pub table: Vec<(f64, f64, f64)>,
    /// 参与拟合的 X 取值范围（升序后的首末值），用于反算求解的搜索区间
    #[allow(dead_code)]
    pub domain: (f64, f64),
    pub notes: Vec<String>,
}

/// 数值格式化：常规量级用定点（自动去掉多余的 0），过大/过小用科学计数法（相当于 C 的 %g）
pub fn fmt(v: f64) -> String {
    if !v.is_finite() {
        return v.to_string();
    }
    if v == 0.0 {
        return "0".to_string();
    }
    let a = v.abs();
    if a >= 1e-4 && a < 1e7 {
        let s = format!("{:.6}", v);
        let s = s.trim_end_matches('0').trim_end_matches('.');
        if s.is_empty() || s == "-" {
            "0".to_string()
        } else {
            s.to_string()
        }
    } else {
        format!("{:.5e}", v)
    }
}

/// 主拟合入口。`raw` 为已完成本底扣除与列转换后的 (X, Y) 数据。
pub fn fit(model: Model, raw: &[(f64, f64)]) -> Option<FitResult> {
    let mut pts: Vec<(f64, f64)> = raw
        .iter()
        .filter(|(x, y)| x.is_finite() && y.is_finite())
        .copied()
        .collect();
    let mut notes: Vec<String> = Vec::new();

    if model.requires_positive_x() {
        let before = pts.len();
        pts.retain(|(x, _)| *x > 0.0);
        if pts.len() < before {
            notes.push(format!(
                "该模型需对 X 取对数，X=0 无定义，已剔除 {} 个 X=0 的点。",
                before - pts.len()
            ));
        }
    }
    if pts.len() < model.min_points() {
        notes.push(format!(
            "有效数据点 {} 个，至少需要 {} 个。",
            pts.len(),
            model.min_points()
        ));
        return None;
    }
    pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let xs: Vec<f64> = pts.iter().map(|p| p.0).collect();
    let ys: Vec<f64> = pts.iter().map(|p| p.1).collect();

    let (evaluator, params, equation, extra) = match model {
        Model::Linear | Model::Quadratic | Model::Cubic => {
            let deg = match model {
                Model::Linear => 1,
                Model::Quadratic => 2,
                _ => 3,
            };
            let c = poly_fit(&xs, &ys, deg)?;
            let mut eq = format!("Y = {}", fmt(c[0]));
            for (i, ci) in c.iter().enumerate().skip(1) {
                eq.push_str(&format!(" {:+}·X^{}", fmt(*ci), i));
            }
            let params: Vec<(String, f64)> = c
                .iter()
                .enumerate()
                .map(|(i, v)| (format!("a{}", i), *v))
                .collect();
            (Evaluator::Poly(c), params, eq, vec![])
        }
        Model::LogitLog => fit_logit_log(&pts)?,
        Model::Logistic1 | Model::Logistic2 | Model::Logistic3 | Model::Hill | Model::Exponential => {
            let kind = match model {
                Model::Logistic1 => CurveKind::Logistic4,
                Model::Logistic2 => CurveKind::Logistic5,
                Model::Logistic3 => CurveKind::LogisticExp,
                Model::Hill => CurveKind::Hill,
                _ => CurveKind::Exp3,
            };
            let starts = initial_guesses(kind, &pts);
            let f = move |x: f64, p: &[f64]| curve_value(kind, x, p);
            let (mut p, _sse) = lm_fit(&pts, f, &starts, 300)?;
            // 收敛结果可能是等价表示之一，规范化为 B>0 的约定形式
            canonicalize(kind, &mut p);
            let (params, eq, extra) = describe_curve(kind, &p);
            (Evaluator::Curve { kind, p }, params, eq, extra)
        }
        Model::CubicSpline => {
            let sp = natural_cubic_spline(pts.clone())?;
            let eq = format!(
                "三次样条（{} 个节点，{} ~ {}）",
                sp.xs.len(),
                fmt(sp.xs[0]),
                fmt(sp.xs[sp.xs.len() - 1])
            );
            let params = vec![("节点数".to_string(), sp.xs.len() as f64)];
            (Evaluator::Spline(sp), params, eq, vec![])
        }
    };

    let q: FitQuality = quality(&pts, |x| evaluator.eval(x))?;
    let curve = make_curve(&evaluator, &pts, model);
    let table: Vec<(f64, f64, f64)> = pts
        .iter()
        .map(|(x, y)| (*x, *y, evaluator.eval(*x).unwrap_or(f64::NAN)))
        .collect();

    let mut extra2 = extra;
    if model.requires_positive_x() {
        extra2.push(("X 轴".to_string(), "浓度取正值（对数/幂运算）".to_string()));
    }

    // 参数可辨识性提示：参数多、数据少时，不同参数组合可能给出几乎相同的曲线，
    // 此时各参数的数值不稳定（不唯一），但曲线本身仍是有效拟合。
    let n_param = params.len();
    if matches!(model, Model::Logistic2) && pts.len() <= n_param * 2 {
        notes.push(
            "五参数模型的 B、C、G 之间存在较强相关性，数据点偏少时参数解可能不唯一              （曲线仍然有效，但单个参数值会随数据轻微波动）；若只关心 ED50，建议改用四参数模型。"
                .to_string(),
        );
    }
    if matches!(model, Model::Exponential) && pts.len() <= n_param * 3 {
        notes.push(
            "三参数指数模型中 a 与 c 存在相关性，数据点偏少时参数解可能不唯一。".to_string(),
        );
    }

    Some(FitResult {
        model,
        evaluator,
        params,
        equation: equation,
        extra: extra2,
        sse: q.sse,
        r2: q.r2,
        rmse: q.rmse,
        n: q.n,
        curve,
        table,
        domain: (xs[0], xs[xs.len() - 1]),
        notes,
    })
}

/// Logit-log 直线拟合。
///
/// Y 可能是「比例(0~1)」也可能是「百分比(0~100)」，两者相差 100 倍，
/// 而 logit 变换对 Y 的绝对量级敏感（p=0.5 对应的 Y 分别是 0.5 和 50），
/// 判错会导致 ED50/IC50 完全错误。
///
/// 早期版本用 `ymax > 1.0 && ymax <= 100.0` 推断为百分比 —— 但 **OD 吸光度数据
/// （如 0.2~1.8）也落在这个区间**，会被误判成百分比，得到的 ED50 是错的。
/// 现在改为：两种标度都试，按**还原到原始 Y 后的 SSE** 取更优者，并明确标注采用了哪种。
fn fit_logit_log(pts: &[(f64, f64)]) -> Option<(Evaluator, Vec<(String, f64)>, String, Vec<(String, String)>)> {
    let mut best: Option<(f64, f64, f64, f64)> = None; // (scale, a, b, sse_on_raw_y)

    for scale in [1.0f64, 100.0] {
        // 该标度下 Y/scale 必须全部落在 (0,1)，否则 logit 无定义
        let usable: Vec<(f64, f64)> = pts
            .iter()
            .filter(|(x, y)| *x > 0.0 && *y > 0.0 && *y < scale)
            .copied()
            .collect();
        if usable.len() < 2 {
            continue;
        }
        let tx: Vec<f64> = usable.iter().map(|(x, _)| x.log10()).collect();
        let ty: Vec<f64> = usable
            .iter()
            .map(|(_, y)| {
                let p = y / scale;
                (p / (1.0 - p)).ln()
            })
            .collect();
        let c = match poly_fit(&tx, &ty, 1) {
            Some(c) => c,
            None => continue,
        };
        let (a, b) = (c[0], c[1]);
        // 用还原到原始 Y 的 SSE 评价（与标度无关，可公平比较）
        let mut sse = 0.0;
        let mut ok = true;
        for (x, y) in &usable {
            let u = a + b * x.log10();
            let e = clamp_exp(-u);
            let yh = scale / (1.0 + e);
            if !yh.is_finite() {
                ok = false;
                break;
            }
            sse += (y - yh) * (y - yh);
        }
        if !ok {
            continue;
        }
        if best.map_or(true, |(_, _, _, s)| sse < s) {
            best = Some((scale, a, b, sse));
        }
    }

    let (scale, a, b, _) = best?;
    let mut extra = Vec::new();
    if b.abs() > 1e-12 {
        // logit(p)=0 对应 p=0.5，即 ED50 / IC50
        let ed50 = 10f64.powf(-a / b);
        extra.push(("ED50 / IC50".to_string(), fmt(ed50)));
    }
    extra.push((
        "Y 标度".to_string(),
        if (scale - 100.0).abs() < f64::EPSILON {
            "按百分比(0~100)处理".to_string()
        } else {
            "按比例(0~1)处理".to_string()
        },
    ));
    let eq = format!("logit(Y/{}) = {} {:+}·log10(X)", fmt(scale), fmt(a), fmt(b));
    let params = vec![("a (截距)".to_string(), a), ("b (斜率)".to_string(), b)];
    Some((Evaluator::LogitLog { a, b, scale }, params, eq, extra))
}

fn describe_curve(kind: CurveKind, p: &[f64]) -> (Vec<(String, f64)>, String, Vec<(String, String)>) {
    // 注意：为保证 C>0、k>0、n>0、G>0，非线性模型内部用 ln 形式做参数化
    // （即 p[2] 存的是 ln(C) 而非 C）。因此**展示值必须是 exp 还原后的值**，
    // 不能直接输出 p[i]——否则参数表里会出现 ln(C)，与方程字符串里的 C 不一致。
    let (names, vals, eq, extra): (
        Vec<&'static str>,
        Vec<f64>,
        String,
        Vec<(String, String)>,
    ) = match kind {
        CurveKind::Logistic4 => {
            let c = p[2].exp();
            (
                vec!["A", "D", "C", "B"],
                vec![p[0], p[1], c, p[3]],
                format!(
                    "Y = {} + ({} - {}) / [1 + (X/{})^{}]",
                    fmt(p[1]),
                    fmt(p[0]),
                    fmt(p[1]),
                    fmt(c),
                    fmt(p[3])
                ),
                vec![("ED50 (拐点 C)".to_string(), fmt(c))],
            )
        }
        CurveKind::Logistic5 => {
            let c = p[2].exp();
            let g = p[4].exp();
            (
                vec!["A", "D", "C", "B", "G"],
                vec![p[0], p[1], c, p[3], g],
                format!(
                    "Y = {} + ({} - {}) / [1 + (X/{})^{}]^{}",
                    fmt(p[1]),
                    fmt(p[0]),
                    fmt(p[1]),
                    fmt(c),
                    fmt(p[3]),
                    fmt(g)
                ),
                vec![
                    ("C".to_string(), fmt(c)),
                    ("G (不对称因子)".to_string(), fmt(g)),
                ],
            )
        }
        CurveKind::LogisticExp => (
            vec!["A", "D", "C", "B"],
            vec![p[0], p[1], p[2], p[3]],
            format!(
                "Y = {} + ({} - {}) / [1 + e^({}·(X - {}))]",
                fmt(p[1]),
                fmt(p[0]),
                fmt(p[1]),
                fmt(-p[3]),
                fmt(p[2])
            ),
            vec![("中点 C".to_string(), fmt(p[2]))],
        ),
        CurveKind::Hill => {
            let k = p[2].exp();
            let n = p[3].exp();
            (
                vec!["Bottom", "Top", "k", "n"],
                vec![p[0], p[1], k, n],
                format!(
                    "Y = {} + ({} - {}) / [1 + (k/X)^n], k={}, n={}",
                    fmt(p[0]),
                    fmt(p[1]),
                    fmt(p[0]),
                    fmt(k),
                    fmt(n)
                ),
                vec![("ED50 (k)".to_string(), fmt(k)), ("Hill 系数 n".to_string(), fmt(n))],
            )
        }
        CurveKind::Exp3 => (
            vec!["a", "b", "c"],
            vec![p[0], p[1], p[2]],
            format!("Y = {}·e^({}·X) + {}", fmt(p[0]), fmt(p[1]), fmt(p[2])),
            vec![],
        ),
    };
    let params: Vec<(String, f64)> = names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.to_string(), vals[i]))
        .collect();
    (params, eq, extra)
}

/// 参数规范化：把等价的两种表示统一成一种，保证输出可跨软件对比。
///
/// 对幂形式 `1/(1+(X/C)^B)` 与指数形式 `1/(1+e^(-B(X-C)))`，恒有等价变换
///
/// ```text
/// (A, D,  B)  <-->  (D, A, -B)      曲线完全重合（已验证到 1e-16）
/// ```
///
/// 即同一条曲线既能用“A=下平台、B<0”表示，也能用“A=上平台、B>0”表示。
/// LM 从哪个起点收敛是任意的，若不规范化，同一份数据可能这次输出 B=+0.99、
/// 下次输出 B=-0.99，且 A/D 互相对调 —— 与 ELISA Calc / GraphPad 等软件的
/// 约定（**A = 零剂量端平台，D = 无穷剂量端平台，B > 0**）相反，无法直接对比。
///
/// 这里统一规范为 **B > 0**，使 A/D/B 的符号与量级和主流软件一致。
fn canonicalize(kind: CurveKind, p: &mut [f64]) {
    match kind {
        CurveKind::Logistic4 | CurveKind::Logistic5 | CurveKind::LogisticExp => {
            if p[3] < 0.0 {
                p.swap(0, 1); // A <-> D
                p[3] = -p[3]; // B -> -B
            }
        }
        // Hill 的 n = exp(p[3]) > 0、k = exp(p[2]) > 0，不存在符号歧义
        // Exp3 无对称性，均无需处理
        _ => {}
    }
}

/// 非线性模型的多起点初值：覆盖不同的平台方向、拐点位置与斜率量级，取 LM 收敛后 SSE 最小者
fn initial_guesses(kind: CurveKind, pts: &[(f64, f64)]) -> Vec<Vec<f64>> {
    let xs: Vec<f64> = pts.iter().map(|p| p.0).collect();
    let ys: Vec<f64> = pts.iter().map(|p| p.1).collect();
    let n = pts.len();
    let ymin = ys.iter().cloned().fold(f64::MAX, f64::min);
    let ymax = ys.iter().cloned().fold(f64::MIN, f64::max);
    let ymean = mean(&ys);
    let xr = (fitting::quantile(&xs, 1.0) - fitting::quantile(&xs, 0.0)).abs().max(1e-9);

    let take = (n / 3).max(1);
    let lo_mean = mean(&ys[..take]);
    let hi_mean = mean(&ys[n.saturating_sub(take)..]);
    let increasing = hi_mean >= lo_mean;

    let mut pos_x: Vec<f64> = xs.iter().cloned().filter(|x| *x > 0.0).collect();
    // 数据可能全是 X=0 起（含零点），此时不要求候选值为正
    let allow_zero = pos_x.len() < xs.len() / 2;
    if pos_x.is_empty() {
        pos_x = xs.clone();
    }
    let mut cands_c: Vec<f64> = (0..3)
        .map(|i| fitting::quantile(&pos_x, 0.25 + 0.25 * i as f64))
        .filter(|v| if allow_zero { *v >= 0.0 } else { *v > 0.0 })
        .collect();
    cands_c.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    cands_c.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    if cands_c.is_empty() {
        cands_c.push(1.0);
    }

    let mut starts: Vec<Vec<f64>> = Vec::new();
    match kind {
        CurveKind::Logistic4 | CurveKind::Logistic5 => {
            // A = 低浓度端平台，D = 高浓度端平台
            let (a0, d0) = if increasing { (ymin, ymax) } else { (ymax, ymin) };
            let sign = if increasing { -1.0 } else { 1.0 };
            for &c in &cands_c {
                for &mag in &[0.5f64, 1.0, 2.0, 4.0] {
                    let b0 = sign * mag;
                    match kind {
                        CurveKind::Logistic4 => starts.push(vec![a0, d0, c.ln(), b0]),
                        _ => {
                            for &g in &[0.5f64, 1.0, 2.0] {
                                starts.push(vec![a0, d0, c.ln(), b0, g.ln()]);
                            }
                        }
                    }
                    // 反向也试一次，避免方向判断失误
                    match kind {
                        CurveKind::Logistic4 => starts.push(vec![d0, a0, c.ln(), -b0]),
                        _ => {
                            for &g in &[0.5f64, 1.0, 2.0] {
                                starts.push(vec![d0, a0, c.ln(), -b0, g.ln()]);
                            }
                        }
                    }
                }
            }
        }
        CurveKind::LogisticExp => {
            // A = 高 X 端平台，D = 低 X 端平台
            let (a0, d0) = if increasing { (ymax, ymin) } else { (ymin, ymax) };
            let sign = if increasing { 1.0 } else { -1.0 };
            let c_pos: Vec<f64> = (0..3)
                .map(|i| fitting::quantile(&xs, 0.3 + 0.2 * i as f64))
                .collect();
            for &c in &c_pos {
                for &mag in &[0.5f64, 1.0, 2.0, 4.0] {
                    let b0 = sign * mag / xr;
                    starts.push(vec![a0, d0, c, b0]);
                    starts.push(vec![d0, a0, c, -b0]);
                }
            }
        }
        CurveKind::Hill => {
            let (bottom0, top0) = if increasing { (ymin, ymax) } else { (ymax, ymin) };
            for &k in &cands_c {
                for &n0 in &[0.5f64, 1.0, 2.0, 4.0] {
                    starts.push(vec![bottom0, top0, k.ln(), n0.ln()]);
                    starts.push(vec![top0, bottom0, k.ln(), n0.ln()]);
                }
            }
        }
        CurveKind::Exp3 => {
            let amp = (ymax - ymin).abs().max(1e-6);
            for &bs in &[-2.0f64, -0.5, 0.5, 2.0] {
                let b0 = bs / xr;
                for &c0 in &[ymin, 0.0, ymean] {
                    for &s in &[1.0f64, -1.0] {
                        starts.push(vec![s * amp, b0, c0]);
                    }
                }
            }
        }
    }
    if starts.is_empty() {
        starts.push(vec![0.0; 4]);
    }
    starts
}

/// 生成绘图用的拟合曲线（X 跨数量级时按对数等距采样）
/// 按需要的点数生成拟合曲线（用于高分辨率导出）。
///
/// 界面绘图只用 260 点就够了，但导出 4K/8K 时曲线横跨数千物理像素，
/// 260 点会出现明显的折线感。这里按目标点数重新采样：
/// - 线性区间：等距采样；
/// - 跨度大且要求 X>0 的模型：对数等距（避免低浓度段点稀疏）。
///
/// `count` 由调用方按输出宽度决定（通常取画布宽度的 1.5～2 倍，上限 20000）。
pub fn dense_curve(ev: &Evaluator, pts: &[(f64, f64)], model: Model, count: usize) -> Vec<(f64, f64)> {
    let count = count.clamp(2, 20000);
    let xs: Vec<f64> = pts.iter().map(|p| p.0).collect();
    let mut xmin = fitting::quantile(&xs, 0.0);
    let mut xmax = fitting::quantile(&xs, 1.0);
    if !xmin.is_finite() || !xmax.is_finite() {
        return Vec::new();
    }
    if (xmax - xmin).abs() < 1e-12 {
        xmin -= 1.0;
        xmax += 1.0;
    }
    let log_space = model.requires_positive_x() && xmin > 0.0 && xmax / xmin > 20.0;
    (0..count)
        .filter_map(|i| {
            let t = i as f64 / (count as f64 - 1.0);
            let x = if log_space {
                (xmin.ln() + (xmax.ln() - xmin.ln()) * t).exp()
            } else {
                xmin + (xmax - xmin) * t
            };
            ev.eval(x).map(|y| (x, y))
        })
        .collect()
}

fn make_curve(ev: &Evaluator, pts: &[(f64, f64)], model: Model) -> Vec<(f64, f64)> {
    let xs: Vec<f64> = pts.iter().map(|p| p.0).collect();
    let mut xmin = fitting::quantile(&xs, 0.0);
    let mut xmax = fitting::quantile(&xs, 1.0);
    if !xmin.is_finite() || !xmax.is_finite() {
        return Vec::new();
    }
    if (xmax - xmin).abs() < 1e-12 {
        xmin -= 1.0;
        xmax += 1.0;
    }
    let count = 260usize;
    let log_space = model.requires_positive_x() && xmin > 0.0 && xmax / xmin > 20.0;
    (0..count)
        .filter_map(|i| {
            let t = i as f64 / (count as f64 - 1.0);
            let x = if log_space {
                (xmin.ln() + (xmax.ln() - xmin.ln()) * t).exp()
            } else {
                xmin + (xmax - xmin) * t
            };
            ev.eval(x).map(|y| (x, y))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_fit_ok() {
        let pts: Vec<(f64, f64)> = (0..8).map(|i| (i as f64, 1.0 + 2.5 * i as f64)).collect();
        let r = fit(Model::Linear, &pts).unwrap();
        assert!((r.r2 - 1.0).abs() < 1e-10);
        assert!((r.params[1].1 - 2.5).abs() < 1e-8);
    }

    #[test]
    fn four_pl_recovers_parameters() {
        // Y = 0.1 + (2.0 - 0.1)/(1 + (X/50)^1.5)
        let xs = [1.0f64, 5.0, 10.0, 25.0, 50.0, 100.0, 200.0, 400.0];
        let pts: Vec<(f64, f64)> = xs
            .iter()
            .map(|x| (*x, 0.1 + (2.0 - 0.1) / (1.0 + (x / 50.0f64).powf(1.5))))
            .collect();
        let r = fit(Model::Logistic1, &pts).unwrap();
        assert!(r.r2 > 0.9999, "r2={}", r.r2);
        let c = r.params[2].1.exp();
        assert!((c - 50.0).abs() < 1.0, "C={}", c);
        assert!((r.params[3].1 - 1.5).abs() < 0.05, "B={}", r.params[3].1);
    }

    #[test]
    fn hill_recovers_ed50() {
        let xs = [1.0f64, 3.0, 10.0, 30.0, 100.0, 300.0, 1000.0];
        let pts: Vec<(f64, f64)> = xs
            .iter()
            .map(|x| {
                let k = 30.0f64;
                let n = 1.2f64;
                let y = 0.05 + 2.0 * x.powf(n) / (k.powf(n) + x.powf(n));
                (*x, y)
            })
            .collect();
        let r = fit(Model::Hill, &pts).unwrap();
        let k = r.params[2].1.exp();
        assert!((k - 30.0).abs() / 30.0 < 0.1, "k={}", k);
    }

    #[test]
    fn logit_log_ed50() {
        // 以百分比形式给出抑制率：p = 100/(1+(X/IC50)^-b) → logit(p) 对 log10(X) 为线性
        let ic50 = 40.0f64;
        let b = -1.8f64; // logit 对 log10 X 的斜率
        let xs = [2.0f64, 5.0, 12.0, 30.0, 80.0, 200.0];
        let pts: Vec<(f64, f64)> = xs
            .iter()
            .map(|x| {
                let u = b * (x.log10() - ic50.log10());
                let p = 100.0 / (1.0 + (-u).exp());
                (*x, p)
            })
            .collect();
        let r = fit(Model::LogitLog, &pts).unwrap();
        assert!(r.r2 > 0.999, "r2={}", r.r2);
        let got: f64 = r
            .extra
            .iter()
            .find(|(k, _)| k.starts_with("ED50"))
            .unwrap()
            .1
            .parse()
            .unwrap();
        assert!((got - ic50).abs() / ic50 < 0.02, "ED50={}", got);
    }

    #[test]
    fn spline_is_exact() {
        let pts = vec![(0.0, 0.5), (1.0, 1.2), (2.0, 0.8), (3.5, 2.1)];
        let r = fit(Model::CubicSpline, &pts).unwrap();
        assert!(r.sse < 1e-18, "sse={}", r.sse);
    }

    /// 用给定真参数生成无噪声曲线数据
    fn synth(kind: CurveKind, p: &[f64], xs: &[f64]) -> Vec<(f64, f64)> {
        xs.iter().map(|&x| (x, curve_value(kind, x, p))).collect()
    }

    #[test]
    fn nonlinear_params_roundtrip() {
        // 真参数 -> 造数据 -> 拟合 -> 还原，应回到真参数（内部 ln 参数化必须正确还原）
        let cases: Vec<(CurveKind, Vec<f64>, Vec<f64>, &str)> = vec![
            (
                CurveKind::Logistic4,
                vec![1.8, 0.1, 25f64.ln(), 1.0],
                vec![0.0, 1.0, 3.0, 6.0, 12.0, 25.0, 50.0, 100.0, 200.0],
                "Logistic4",
            ),
            (
                CurveKind::Logistic5,
                vec![2.0, 0.15, 20f64.ln(), 1.2, 1.8f64.ln()],
                vec![0.0, 1.0, 3.0, 6.0, 12.0, 20.0, 40.0, 80.0, 160.0, 320.0],
                "Logistic5",
            ),
            (
                CurveKind::LogisticExp,
                vec![2.0, 0.2, 30.0, 0.08],
                vec![0.0, 5.0, 10.0, 20.0, 30.0, 40.0, 60.0, 80.0, 100.0],
                "LogisticExp",
            ),
            (
                CurveKind::Hill,
                vec![0.15, 1.9, 20f64.ln(), 1.5f64.ln()],
                vec![0.0, 1.0, 3.0, 6.0, 12.0, 20.0, 40.0, 80.0, 160.0],
                "Hill",
            ),
            (
                CurveKind::Exp3,
                vec![1.5, -0.05, 0.3],
                vec![0.0, 5.0, 10.0, 20.0, 30.0, 40.0, 60.0, 80.0],
                "Exp3",
            ),
        ];
        for (kind, p_true, xs, name) in cases {
            let pts = synth(kind, &p_true, &xs);
            let starts = initial_guesses(kind, &pts);
            let (mut p, sse) =
                fitting::lm_fit(&pts, |x, p| curve_value(kind, x, p), &starts, 300).unwrap();
            canonicalize(kind, &mut p);
            assert!(sse < 1e-14, "{}: SSE = {} 未收敛到精确解", name, sse);

            let (_, got) = describe_curve(kind, &p);
            let (_, want) = describe_curve(kind, &p_true);
            for (g, w) in got.iter().zip(want.iter()) {
                let rel = (g - w).abs() / w.abs().max(1e-9);
                assert!(rel < 1e-4, "{}: 还原 {} vs 真值 {} (相对误差 {})", name, g, w, rel);
            }
        }
    }

    #[test]
    fn logistic_params_are_canonical_b_positive() {
        // (A,D,B) 与 (D,A,-B) 是同一条曲线，必须规范为 B>0，
        // 否则与 ELISA Calc / GraphPad 的约定相反，无法跨软件对比
        let pts = elisa_case();
        let r = fit(Model::Logistic1, &pts).unwrap();
        let get = |k: &str| r.params.iter().find(|(n, _)| n == k).unwrap().1;
        let (a, d, b) = (get("A"), get("D"), get("B"));

        assert!(b > 0.0, "B = {} 应为正（规范形式）", b);
        // A 为零剂量端平台（高值），D 为无穷剂量端平台（低值）
        assert!(a > d, "A = {} 应大于 D = {}（递减型：A 为零剂量端平台）", a, d);
        // 与参考软件 A=1.66585 / D=0.10146 / B=+0.96750 同序同号
        assert!((a - 1.746).abs() < 0.1, "A = {}", a);
        assert!((d - 0.1147).abs() < 0.05, "D = {}", d);
        assert!((b - 0.995).abs() < 0.15, "B = {}", b);
    }

    #[test]
    fn canonical_transform_preserves_curve() {
        // 验证 (A,D,B) <-> (D,A,-B) 确实等价
        let p = vec![0.114711, 1.746051, 8.940322f64.ln(), -0.994827];
        let mut q = p.clone();
        canonicalize(CurveKind::Logistic4, &mut q);
        assert!(q[3] > 0.0);
        for x in [0.0, 1.0, 10.0, 25.0, 100.0, 500.0] {
            let (a, b) = (curve_value(CurveKind::Logistic4, x, &p), curve_value(CurveKind::Logistic4, x, &q));
            assert!((a - b).abs() < 1e-9, "x={}: {} vs {}", x, a, b);
        }
    }

    #[test]
    fn logit_log_scale_is_detected() {
        // Y 为比例(0~1) 与 百分比(0~100) 都应被正确识别
        let (a_t, b_t) = (2.0f64, -1.5f64);
        let xs = [1.0, 3.0, 10.0, 30.0, 100.0, 300.0, 1000.0];
        for scale in [1.0f64, 100.0] {
            let pts: Vec<(f64, f64)> = xs
                .iter()
                .map(|&x| (x, scale / (1.0 + (-(a_t + b_t * x.log10())).exp())))
                .collect();
            let (ev, _, _, _) = fit_logit_log(&pts).unwrap();
            if let Evaluator::LogitLog { scale: s, .. } = ev {
                assert!((s - scale).abs() < 1e-9, "标度判为 {}，应为 {}", s, scale);
            } else {
                panic!("应为 LogitLog 求值器");
            }
        }
        // OD 吸光度数据（0.2~1.8）不应被误判为百分比
        let pts: Vec<(f64, f64)> = vec![
            (3.13, 1.265),
            (6.25, 1.136),
            (12.5, 0.803),
            (25.0, 0.527),
            (50.0, 0.343),
            (100.0, 0.244),
            (200.0, 0.209),
        ];
        let (ev, _, _, _) = fit_logit_log(&pts).unwrap();
        if let Evaluator::LogitLog { scale: s, .. } = ev {
            assert!((s - 1.0).abs() < 1e-9, "OD 数据标度判为 {}，应为 1", s);
        }
    }

    #[test]
    fn axis_ticks_fixed_step() {
        // X 轴 50 一格
        let t = axis_ticks(0.0, 200.0, 50.0, 40);
        assert_eq!(t, vec![0.0, 50.0, 100.0, 150.0, 200.0]);
        // Y 轴 0.5 一格
        let t = axis_ticks(0.2, 2.3, 0.5, 40);
        assert_eq!(t, vec![0.5, 1.0, 1.5, 2.0]);
        // 格数过多时自动放大步长
        let t = axis_ticks(0.0, 5000.0, 50.0, 40);
        assert!(t.len() <= 40, "{}", t.len());
        assert!(t.iter().all(|v| (v % 50.0).abs() < 1e-9), "{:?}", t);
        // 含零点的 X（不再被剔除）
        let t = axis_ticks(-10.0, 140.0, 50.0, 40);
        assert_eq!(t, vec![0.0, 50.0, 100.0]);
    }

    #[test]
    fn zero_x_is_kept_for_hill() {
        // Hill 模型在 X=0 处有定义（Y = Bottom），不应被剔除
        let pts: Vec<(f64, f64)> = vec![
            (0.0, 0.05),
            (10.0, 0.7),
            (30.0, 1.05),
            (100.0, 1.8),
            (300.0, 1.95),
            (1000.0, 2.0),
        ];
        let r = fit(Model::Hill, &pts).unwrap();
        assert_eq!(r.n, 6, "X=0 的点应保留，实际 n={}", r.n);
        assert!(r.evaluator.eval(0.0).is_some());
    }

    #[test]
    fn zero_x_is_kept_for_logistic() {
        let pts: Vec<(f64, f64)> = vec![
            (0.0, 0.1),
            (5.0, 0.25),
            (25.0, 1.0),
            (100.0, 1.8),
            (400.0, 1.98),
        ];
        let r = fit(Model::Logistic1, &pts).unwrap();
        assert_eq!(r.n, 5, "X=0 的点应保留，实际 n={}", r.n);
    }

    /// 用户给出的实测数据（来源：ELISA Calc 对照用例）
    fn elisa_case() -> Vec<(f64, f64)> {
        vec![
            (200.0, 0.209),
            (100.0, 0.244),
            (50.0, 0.343),
            (25.0, 0.527),
            (12.5, 0.803),
            (6.25, 1.136),
            (3.13, 1.265),
            (0.0, 1.756),
        ]
    }

    #[test]
    fn logistic4_params_are_not_log_scaled() {
        // 内部用 ln(C) 参数化，参数表必须输出 exp 还原后的真实 C
        let r = fit(Model::Logistic1, &elisa_case()).unwrap();
        let get = |k: &str| r.params.iter().find(|(n, _)| n == k).unwrap().1;
        let (a, d, c, b) = (get("A"), get("D"), get("C"), get("B"));

        // C 应落在数据 X 量级内（8~10），绝不是 ln(C)≈2.19
        assert!(c > 5.0 && c < 15.0, "C = {} 不是真实值（疑似 ln 未还原）", c);
        // 与 Python/scipy 全局最优解一致（SSE = 0.00855）
        assert!((c - 8.94).abs() < 0.6, "C = {} 偏离全局最优 8.94", c);
        assert!((a - 1.746).abs() < 0.1, "A = {}", a);
        assert!((d - 0.1147).abs() < 0.05, "D = {}", d);
        assert!((b - 0.995).abs() < 0.15, "B = {}", b);
        // 拟合质量：R² ≈ 0.996
        assert!(r.r2 > 0.99, "R² = {} 偏低", r.r2);
        assert!(r.sse < 0.02, "SSE = {} 偏大", r.sse);
    }

    #[test]
    fn logistic4_equation_matches_params() {
        // 方程字符串里的 C 与参数表里的 C 必须是同一个值
        let r = fit(Model::Logistic1, &elisa_case()).unwrap();
        let c = r.params.iter().find(|(n, _)| n == "C").unwrap().1;
        let s = fmt(c);
        assert!(
            r.equation.contains(&s),
            "方程 {} 不含参数表里的 C = {}",
            r.equation,
            s
        );
    }

    #[test]
    fn hill_params_are_not_log_scaled() {
        let r = fit(Model::Hill, &elisa_case()).unwrap();
        let get = |k: &str| r.params.iter().find(|(n, _)| n == k).unwrap().1;
        let (k, n) = (get("k"), get("n"));
        assert!(k > 1.0, "k = {} 疑似 ln 未还原", k);
        assert!(n > 0.2, "n = {} 疑似 ln 未还原", n);
    }

    #[test]
    fn invert_linear_roundtrip() {
        let pts: Vec<(f64, f64)> = (0..8).map(|i| (i as f64, 1.0 + 2.5 * i as f64)).collect();
        let r = fit(Model::Linear, &pts).unwrap();
        let x = solve_x(&r, 11.0).pop().unwrap();
        assert!((x - 4.0).abs() < 1e-6, "x={}", x);
    }

    #[test]
    fn invert_four_pl_roundtrip() {
        let xs = [1.0f64, 5.0, 10.0, 25.0, 50.0, 100.0, 200.0, 400.0];
        let pts: Vec<(f64, f64)> = xs
            .iter()
            .map(|x| (*x, 0.1 + (2.0 - 0.1) / (1.0 + (x / 50.0f64).powf(1.5))))
            .collect();
        let r = fit(Model::Logistic1, &pts).unwrap();
        let roots = solve_x(&r, 1.05); // 拐点处 Y = (A+D)/2
        assert_eq!(roots.len(), 1, "{:?}", roots);
        assert!((roots[0] - 50.0).abs() / 50.0 < 0.02, "x={}", roots[0]);
    }

    #[test]
    fn invert_hill_roundtrip() {
        let xs = [1.0f64, 3.0, 10.0, 30.0, 100.0, 300.0, 1000.0];
        let pts: Vec<(f64, f64)> = xs
            .iter()
            .map(|x| {
                let (k, n) = (30.0f64, 1.2f64);
                (*x, 0.05 + 2.0 * x.powf(n) / (k.powf(n) + x.powf(n)))
            })
            .collect();
        let r = fit(Model::Hill, &pts).unwrap();
        let roots = solve_x(&r, 0.05 + 1.0); // 半效点
        assert_eq!(roots.len(), 1, "{:?}", roots);
        assert!((roots[0] - 30.0).abs() / 30.0 < 0.1, "x={}", roots[0]);
    }

    #[test]
    fn scan_finds_two_roots_of_parabola() {
        // y = (x-2)(x-6) = x^2 - 8x + 12，取 y=0 应有 x=2 与 x=6
        let pts: Vec<(f64, f64)> = [-2.0f64, 0.0, 2.0, 4.0, 6.0, 8.0, 10.0]
            .iter()
            .map(|x| (*x, (x - 2.0) * (x - 6.0)))
            .collect();
        let r = fit(Model::Quadratic, &pts).unwrap();
        let roots = solve_x(&r, 0.0);
        assert_eq!(roots.len(), 2, "{:?}", roots);
        assert!((roots[0] - 2.0).abs() < 1e-3, "{:?}", roots);
        assert!((roots[1] - 6.0).abs() < 1e-3, "{:?}", roots);
    }

    #[test]
    fn invert_exponential_roundtrip() {
        let pts: Vec<(f64, f64)> = (0..10)
            .map(|i| {
                let x = i as f64 * 0.4;
                (x, 3.0 * (0.5 * x).exp() + 1.0)
            })
            .collect();
        let r = fit(Model::Exponential, &pts).unwrap();
        let roots = solve_x(&r, 3.0 * (0.5 * 2.0).exp() + 1.0);
        assert_eq!(roots.len(), 1, "{:?}", roots);
        assert!((roots[0] - 2.0).abs() < 1e-3, "{:?}", roots);
    }
}
