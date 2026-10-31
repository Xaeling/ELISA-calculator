//! 数值拟合底层：Householder QR 最小二乘、Levenberg-Marquardt 非线性拟合、自然三次样条插值

pub fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.iter().sum::<f64>() / v.len() as f64
}

/// 一般线性方程组求解（部分选主元高斯消元）。系数矩阵会被就地修改。
pub fn solve_linear_system(a: &mut Vec<Vec<f64>>, b: &mut Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    if n == 0 || a.len() != n {
        return None;
    }
    for col in 0..n {
        let mut pivot = col;
        for r in (col + 1)..n {
            if a[r][col].abs() > a[pivot][col].abs() {
                pivot = r;
            }
        }
        if a[pivot][col].abs() < 1e-14 {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        let pv = a[col][col];
        for r in (col + 1)..n {
            let factor = a[r][col] / pv;
            if factor == 0.0 {
                continue;
            }
            for c in col..n {
                a[r][c] -= factor * a[col][c];
            }
            b[r] -= factor * b[col];
        }
    }
    let mut x = vec![0.0; n];
    for r in (0..n).rev() {
        let mut s = b[r];
        for c in (r + 1)..n {
            s -= a[r][c] * x[c];
        }
        x[r] = s / a[r][r];
    }
    if x.iter().all(|v| v.is_finite()) {
        Some(x)
    } else {
        None
    }
}

/// 线性最小二乘：min ||A x - b||，使用 Householder QR（比正规方程数值更稳定）
pub fn lstsq(a: &[Vec<f64>], b: &[f64]) -> Option<Vec<f64>> {
    let m = b.len();
    if m == 0 {
        return None;
    }
    let n = a.first().map(|r| r.len()).unwrap_or(0);
    if n == 0 || n > m || a.len() != m {
        return None;
    }
    let mut a = a.to_vec();
    let mut b = b.to_vec();

    for k in 0..n {
        let mut norm = 0.0;
        for i in k..m {
            norm += a[i][k] * a[i][k];
        }
        norm = norm.sqrt();
        if norm < 1e-300 {
            continue;
        }
        let alpha = if a[k][k] >= 0.0 { -norm } else { norm };
        let mut v = vec![0.0; m];
        for i in k..m {
            v[i] = a[i][k];
        }
        v[k] -= alpha;
        let mut vnorm2 = 0.0;
        for i in k..m {
            vnorm2 += v[i] * v[i];
        }
        if vnorm2 < 1e-300 {
            continue;
        }
        for j in k..n {
            let mut s = 0.0;
            for i in k..m {
                s += v[i] * a[i][j];
            }
            s *= 2.0 / vnorm2;
            for i in k..m {
                a[i][j] -= s * v[i];
            }
        }
        let mut s = 0.0;
        for i in k..m {
            s += v[i] * b[i];
        }
        s *= 2.0 / vnorm2;
        for i in k..m {
            b[i] -= s * v[i];
        }
    }

    // 回代：取前 n 行上三角部分
    let mut x = vec![0.0; n];
    for r in (0..n).rev() {
        if a[r][r].abs() < 1e-300 {
            return None;
        }
        let mut s = b[r];
        for c in (r + 1)..n {
            s -= a[r][c] * x[c];
        }
        x[r] = s / a[r][r];
    }
    if x.iter().all(|v| v.is_finite()) {
        Some(x)
    } else {
        None
    }
}

/// 多项式拟合，返回系数 [a0, a1, ..., a_degree]，即 y = a0 + a1*x + ...
pub fn poly_fit(xs: &[f64], ys: &[f64], degree: usize) -> Option<Vec<f64>> {
    let n = xs.len();
    if n <= degree || ys.len() != n {
        return None;
    }
    let mut a = vec![vec![0.0; degree + 1]; n];
    for i in 0..n {
        let mut v = 1.0;
        for j in 0..=degree {
            a[i][j] = v;
            v *= xs[i];
        }
    }
    lstsq(&a, ys)
}

/// 多项式求值（Horner）
pub fn poly_eval(c: &[f64], x: f64) -> f64 {
    c.iter().rev().fold(0.0, |acc, &ci| acc * x + ci)
}

#[derive(Clone, Copy, Debug)]
pub struct FitQuality {
    /// 残差平方和
    pub sse: f64,
    /// 决定系数
    pub r2: f64,
    /// 残差标准差（RMSE）
    pub rmse: f64,
    /// 参与拟合的点数
    pub n: usize,
}

/// 按预测函数计算拟合质量
pub fn quality(pts: &[(f64, f64)], predict: impl Fn(f64) -> Option<f64>) -> Option<FitQuality> {
    let n = pts.len();
    if n == 0 {
        return None;
    }
    let ybar = mean(&pts.iter().map(|p| p.1).collect::<Vec<f64>>());
    let mut sse = 0.0;
    let mut sst = 0.0;
    for (x, y) in pts {
        let yh = predict(*x)?;
        if !yh.is_finite() {
            return None;
        }
        sse += (y - yh) * (y - yh);
        sst += (y - ybar) * (y - ybar);
    }
    if !sse.is_finite() {
        return None;
    }
    let r2 = if sst > 0.0 {
        1.0 - sse / sst
    } else if sse == 0.0 {
        1.0
    } else {
        0.0
    };
    let dof = n.saturating_sub(1).max(1) as f64;
    Some(FitQuality {
        sse,
        r2,
        rmse: (sse / dof).sqrt(),
        n,
    })
}

/// 残差平方和；出现 NaN/Inf 时返回 None
pub fn sse_of<F>(pts: &[(f64, f64)], f: &F, p: &[f64]) -> Option<f64>
where
    F: Fn(f64, &[f64]) -> f64,
{
    let mut s = 0.0;
    for (x, y) in pts {
        let yh = f(*x, p);
        if !yh.is_finite() {
            return None;
        }
        let r = y - yh;
        s += r * r;
    }
    if s.is_finite() {
        Some(s)
    } else {
        None
    }
}

/// Levenberg-Marquardt（数值雅可比）。
/// `starts` 为多组初值，取残差平方和最小者；返回 (参数, SSE)。
pub fn lm_fit<F>(pts: &[(f64, f64)], f: F, starts: &[Vec<f64>], max_iter: usize) -> Option<(Vec<f64>, f64)>
where
    F: Fn(f64, &[f64]) -> f64,
{
    let mut best: Option<(Vec<f64>, f64)> = None;
    for p0 in starts {
        if let Some((p, sse)) = lm_single(pts, &f, p0, max_iter) {
            let better = match &best {
                Some((_, s)) => sse < *s,
                None => true,
            };
            if better {
                best = Some((p, sse));
            }
        }
    }
    best
}

fn lm_single<F>(pts: &[(f64, f64)], f: &F, p0: &[f64], max_iter: usize) -> Option<(Vec<f64>, f64)>
where
    F: Fn(f64, &[f64]) -> f64,
{
    let n = p0.len();
    if n == 0 || pts.len() <= n {
        return None;
    }
    let mut p = p0.to_vec();
    let mut sse = sse_of(pts, f, &p)?;
    let mut lambda = 1e-3;

    for _ in 0..max_iter {
        // 数值雅可比 + 法方程
        let mut jtj = vec![vec![0.0; n]; n];
        let mut jtr = vec![0.0; n];
        for (x, y) in pts {
            let base = f(*x, &p);
            if !base.is_finite() {
                return None;
            }
            let mut j = vec![0.0; n];
            for k in 0..n {
                let h = 1e-6 * (1.0 + p[k].abs());
                let mut pp = p.clone();
                pp[k] += h;
                let mut pm = p.clone();
                pm[k] -= h;
                let d = (f(*x, &pp) - f(*x, &pm)) / (2.0 * h);
                if !d.is_finite() {
                    return None;
                }
                j[k] = d;
            }
            let r = y - base;
            for i in 0..n {
                jtr[i] += j[i] * r;
                for k in 0..n {
                    jtj[i][k] += j[i] * j[k];
                }
            }
        }

        let mut accepted = false;
        for _trial in 0..40 {
            let mut a = jtj.clone();
            for i in 0..n {
                a[i][i] += lambda * (1.0 + jtj[i][i]);
            }
            let mut b = jtr.clone();
            if let Some(delta) = solve_linear_system(&mut a, &mut b) {
                let pn: Vec<f64> = (0..n).map(|i| p[i] + delta[i]).collect();
                if let Some(s2) = sse_of(pts, f, &pn) {
                    if s2 <= sse {
                        let converged = (sse - s2) <= 1e-12 * (1.0 + sse);
                        p = pn;
                        sse = s2;
                        lambda = (lambda / 10.0).max(1e-12);
                        accepted = true;
                        if converged {
                            return Some((p, sse));
                        }
                        break;
                    }
                }
            }
            lambda *= 10.0;
            if lambda > 1e14 {
                break;
            }
        }
        if !accepted {
            break;
        }
    }
    Some((p, sse))
}

/// 自然三次样条（Natural Cubic Spline）
#[derive(Clone, Debug)]
pub struct Spline {
    pub xs: Vec<f64>,
    pub ys: Vec<f64>,
    /// 各节点处的二阶导数（自然边界：两端为 0）
    pub m: Vec<f64>,
}

/// 构造自然三次样条。要求 x 严格递增（内部会排序），点数 >= 3；否则返回 None。
pub fn natural_cubic_spline(mut pts: Vec<(f64, f64)>) -> Option<Spline> {
    pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let n = pts.len();
    if n < 3 {
        return None;
    }
    for i in 1..n {
        if pts[i].0 - pts[i - 1].0 <= 0.0 {
            return None; // x 重复，无法插值
        }
    }
    let xs: Vec<f64> = pts.iter().map(|p| p.0).collect();
    let ys: Vec<f64> = pts.iter().map(|p| p.1).collect();
    let h: Vec<f64> = (0..n - 1).map(|i| xs[i + 1] - xs[i]).collect();

    let mut m = vec![0.0; n];
    let inner = n - 2; // 内部未知量个数
    if inner > 0 {
        let mut lower = vec![0.0; inner];
        let mut diag = vec![0.0; inner];
        let mut upper = vec![0.0; inner];
        let mut rhs = vec![0.0; inner];
        for i in 0..inner {
            let k = i + 1;
            lower[i] = h[k - 1];
            diag[i] = 2.0 * (h[k - 1] + h[k]);
            upper[i] = h[k];
            rhs[i] = 6.0 * ((ys[k + 1] - ys[k]) / h[k] - (ys[k] - ys[k - 1]) / h[k - 1]);
        }
        // Thomas 算法
        for i in 1..inner {
            if diag[i - 1].abs() < 1e-300 {
                return None;
            }
            let w = lower[i] / diag[i - 1];
            diag[i] -= w * upper[i - 1];
            rhs[i] -= w * rhs[i - 1];
        }
        let mut sol = vec![0.0; inner];
        if diag[inner - 1].abs() < 1e-300 {
            return None;
        }
        sol[inner - 1] = rhs[inner - 1] / diag[inner - 1];
        for i in (0..inner - 1).rev() {
            if diag[i].abs() < 1e-300 {
                return None;
            }
            sol[i] = (rhs[i] - upper[i] * sol[i + 1]) / diag[i];
        }
        for i in 0..inner {
            m[i + 1] = sol[i];
        }
    }
    Some(Spline { xs, ys, m })
}

/// 样条求值。区间外按端点段线性外推。
pub fn spline_eval(sp: &Spline, x: f64) -> f64 {
    let n = sp.xs.len();
    if n < 2 {
        return f64::NAN;
    }
    let i = sp
        .xs
        .partition_point(|&v| v <= x)
        .saturating_sub(1)
        .min(n - 2);
    let h = sp.xs[i + 1] - sp.xs[i];
    let dx = x - sp.xs[i];
    let b = (sp.ys[i + 1] - sp.ys[i]) / h - h * (2.0 * sp.m[i] + sp.m[i + 1]) / 6.0;
    sp.ys[i] + b * dx + sp.m[i] / 2.0 * dx * dx + (sp.m[i + 1] - sp.m[i]) / (6.0 * h) * dx * dx * dx
}

/// 取一组数的分位数（线性插值），q ∈ [0,1]
pub fn quantile(v: &[f64], q: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let q = q.clamp(0.0, 1.0);
    let pos = q * (s.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = (pos.ceil() as usize).min(s.len() - 1);
    let frac = pos - lo as f64;
    s[lo] + (s[hi] - s[lo]) * frac
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_recovers_slope() {
        let xs: Vec<f64> = (0..10).map(|i| i as f64).collect();
        let ys: Vec<f64> = xs.iter().map(|x| 2.0 + 3.0 * x).collect();
        let c = poly_fit(&xs, &ys, 1).unwrap();
        assert!((c[0] - 2.0).abs() < 1e-8);
        assert!((c[1] - 3.0).abs() < 1e-8);
    }

    #[test]
    fn cubic_recovers_coeffs() {
        let xs: Vec<f64> = (0..12).map(|i| i as f64 * 0.7 - 4.0).collect();
        let ys: Vec<f64> = xs.iter().map(|x| 1.0 - 2.0 * x + 0.5 * x * x + 0.25 * x * x * x).collect();
        let c = poly_fit(&xs, &ys, 3).unwrap();
        assert!((c[0] - 1.0).abs() < 1e-6, "{:?}", c);
        assert!((c[1] + 2.0).abs() < 1e-6, "{:?}", c);
        assert!((c[2] - 0.5).abs() < 1e-6, "{:?}", c);
        assert!((c[3] - 0.25).abs() < 1e-6, "{:?}", c);
    }

    #[test]
    fn spline_passes_through_points() {
        let pts = vec![(0.0, 0.0), (1.0, 2.0), (2.5, -1.0), (4.0, 3.0), (5.0, 1.0)];
        let sp = natural_cubic_spline(pts.clone()).unwrap();
        for (x, y) in pts {
            assert!((spline_eval(&sp, x) - y).abs() < 1e-9);
        }
    }

    #[test]
    fn lm_recovers_exponential() {
        // y = 3*exp(0.4x) + 1
        let f = |x: f64, p: &[f64]| p[0] * (p[1] * x).exp() + p[2];
        let pts: Vec<(f64, f64)> = (0..12)
            .map(|i| {
                let x = i as f64 * 0.5;
                (x, 3.0 * (0.4 * x).exp() + 1.0)
            })
            .collect();
        let (p, sse) = lm_fit(&pts, f, &[vec![2.0, 0.2, 0.5]], 200).unwrap();
        assert!(sse < 1e-8, "sse={} p={:?}", sse, p);
        assert!((p[0] - 3.0).abs() < 1e-3 && (p[1] - 0.4).abs() < 1e-3 && (p[2] - 1.0).abs() < 1e-3);
    }
}
