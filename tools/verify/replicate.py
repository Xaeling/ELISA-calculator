"""逐行复现 elisa-calc-rs 的拟合内核，做「真参数 -> 造数据 -> 拟合 -> 还原」往返验证。"""
import math, numpy as np

# ---------- 复现 models.rs::clamp_exp / curve_value ----------
def clamp_exp(e):
    if not math.isfinite(e):
        return math.inf if e > 0 else 0.0
    if e > 700: return math.inf
    if e < -700: return 0.0
    return math.exp(e)

def curve_value(kind, x, p):
    if kind == 'Logistic4':
        a, d, b = p[0], p[1], p[3]
        if x <= 0.0: return a if b > 0.0 else d
        e = clamp_exp(b * (math.log(x) - p[2]))
        return d + (a - d) / (1.0 + e)
    if kind == 'Logistic5':
        a, d, b = p[0], p[1], p[3]
        g = clamp_exp(p[4])
        if x <= 0.0: return a if b > 0.0 else d
        e = clamp_exp(b * (math.log(x) - p[2]))
        t = clamp_exp(g * math.log(1.0 + e))
        return d + (a - d) / t
    if kind == 'LogisticExp':
        a, d, c, b = p[0], p[1], p[2], p[3]
        e = clamp_exp(-b * (x - c))
        return d + (a - d) / (1.0 + e)
    if kind == 'Hill':
        bottom, top = p[0], p[1]
        n = clamp_exp(p[3])
        if x <= 0.0: return bottom
        e = clamp_exp(n * (p[2] - math.log(x)))
        return bottom + (top - bottom) / (1.0 + e)
    if kind == 'Exp3':
        return p[0] * clamp_exp(p[1] * x) + p[2]
    raise ValueError(kind)

# ---------- 复现 models.rs::describe_curve 的参数还原（关键检查点）----------
def describe_curve(kind, p):
    if kind == 'Logistic4':
        c = math.exp(p[2]); return ['A','D','C','B'], [p[0], p[1], c, p[3]]
    if kind == 'Logistic5':
        c = math.exp(p[2]); g = math.exp(p[4])
        return ['A','D','C','B','G'], [p[0], p[1], c, p[3], g]
    if kind == 'LogisticExp':
        return ['A','D','C','B'], [p[0], p[1], p[2], p[3]]
    if kind == 'Hill':
        k = math.exp(p[2]); n = math.exp(p[3])
        return ['Bottom','Top','k','n'], [p[0], p[1], k, n]
    if kind == 'Exp3':
        return ['a','b','c'], [p[0], p[1], p[2]]
    raise ValueError(kind)

# ---------- 复现 fitting.rs::quantile ----------
def quantile(v, q):
    s = sorted(v); q = min(max(q, 0.0), 1.0)
    pos = q * (len(s) - 1); lo = math.floor(pos); hi = min(math.ceil(pos), len(s) - 1)
    return s[lo] + (s[hi] - s[lo]) * (pos - lo)

# ---------- 复现 models.rs::initial_guesses ----------
def initial_guesses(kind, pts):
    xs = [p[0] for p in pts]; ys = [p[1] for p in pts]; n = len(pts)
    ymin, ymax = min(ys), max(ys); ymean = sum(ys)/n
    xr = max(abs(quantile(xs,1.0) - quantile(xs,0.0)), 1e-9)
    take = max(n//3, 1)
    lo_mean = sum(ys[:take])/take
    hi_mean = sum(ys[n-take:])/take
    increasing = hi_mean >= lo_mean
    pos_x = [x for x in xs if x > 0.0]
    allow_zero = len(pos_x) < len(xs)//2
    if not pos_x: pos_x = xs[:]
    raw_c = [quantile(pos_x, 0.25 + 0.25*i) for i in range(3)]
    if allow_zero:
        cands_c = sorted(set(round(v, 12) for v in raw_c if v >= 0.0))
    else:
        cands_c = sorted(set(round(v, 12) for v in raw_c if v > 0.0))
    if not cands_c: cands_c = [1.0]
    starts = []
    if kind in ('Logistic4','Logistic5'):
        a0, d0 = (ymin, ymax) if increasing else (ymax, ymin)
        sign = -1.0 if increasing else 1.0
        for c in cands_c:
            for mag in (0.5,1.0,2.0,4.0):
                b0 = sign*mag
                if kind=='Logistic4':
                    starts.append([a0,d0,math.log(c) if c>0 else 0.0,b0])
                    starts.append([d0,a0,math.log(c) if c>0 else 0.0,-b0])
                else:
                    for g in (0.5,1.0,2.0):
                        lc = math.log(c) if c>0 else 0.0
                        starts.append([a0,d0,lc,b0,math.log(g)])
                        starts.append([d0,a0,lc,-b0,math.log(g)])
    elif kind=='LogisticExp':
        a0, d0 = (ymax, ymin) if increasing else (ymin, ymax)
        sign = 1.0 if increasing else -1.0
        c_pos = [quantile(xs, 0.3+0.2*i) for i in range(3)]
        for c in c_pos:
            for mag in (0.5,1.0,2.0,4.0):
                b0 = sign*mag/xr
                starts.append([a0,d0,c,b0]); starts.append([d0,a0,c,-b0])
    elif kind=='Hill':
        bottom0, top0 = (ymin, ymax) if increasing else (ymax, ymin)
        for k in cands_c:
            for n0 in (0.5,1.0,2.0,4.0):
                lk = math.log(k) if k>0 else 0.0
                starts.append([bottom0,top0,lk,math.log(n0)])
                starts.append([top0,bottom0,lk,math.log(n0)])
    elif kind=='Exp3':
        amp = max(abs(ymax-ymin), 1e-6)
        for bs in (-2.0,-0.5,0.5,2.0):
            b0 = bs/xr
            for c0 in (ymin, 0.0, ymean):
                for s in (1.0,-1.0):
                    starts.append([s*amp, b0, c0])
    return starts

# ---------- 复现 models.rs::canonicalize ----------
def canonicalize(kind, p):
    if kind in ('Logistic4','Logistic5','LogisticExp'):
        if p[3] < 0.0:
            p[0], p[1] = p[1], p[0]
            p[3] = -p[3]
    return p

# ---------- 复现 fitting.rs::lm_single / lm_fit ----------
def sse_of(pts, kind, p):
    s = 0.0
    for x,y in pts:
        v = curve_value(kind,x,p)
        if not math.isfinite(v): return None
        s += (y-v)**2
    return s

def lm_single(pts, kind, p0, max_iter=300):
    n = len(p0)
    if n==0 or len(pts)<=n: return None
    p = list(p0)
    sse = sse_of(pts,kind,p)
    if sse is None: return None
    lam = 1e-3
    for _ in range(max_iter):
        JTJ = np.zeros((n,n)); JTr = np.zeros(n); bad=False
        for x,y in pts:
            base = curve_value(kind,x,p)
            if not math.isfinite(base): bad=True; break
            j = np.zeros(n)
            for k in range(n):
                h = 1e-6*(1.0+abs(p[k]))
                pp = list(p); pp[k]+=h
                pm = list(p); pm[k]-=h
                d = (curve_value(kind,x,pp)-curve_value(kind,x,pm))/(2*h)
                if not math.isfinite(d): bad=True; break
                j[k]=d
            if bad: break
            r = y-base
            JTr += j*r; JTJ += np.outer(j,j)
        if bad: return None
        accepted=False
        for _trial in range(40):
            A = JTJ.copy()
            for i in range(n): A[i,i] += lam*(1.0+JTJ[i,i])
            try: delta = np.linalg.solve(A, JTr.copy())
            except np.linalg.LinAlgError:
                lam*=10.0
                if lam>1e14: break
                continue
            pn = [p[i]+delta[i] for i in range(n)]
            s2 = sse_of(pts,kind,pn)
            if s2 is not None and s2 <= sse:
                conv = (sse-s2) <= 1e-12*(1.0+sse)
                p, sse = pn, s2
                lam = max(lam/10.0, 1e-12); accepted=True
                if conv: return (p, sse)
                break
            lam *= 10.0
            if lam > 1e14: break
        if not accepted: break
    return (p, sse)

def lm_fit(pts, kind, starts, max_iter=300):
    best=None
    for p0 in starts:
        r = lm_single(pts,kind,p0,max_iter)
        if r and (best is None or r[1] < best[1]): best=r
    return best


# ---------- 复现 models.rs::fit_logit_log（修复后：双向尝试取 SSE 更优）----------
def fit_logit_log(pts):
    best=None
    for scale in (1.0, 100.0):
        usable=[(x,y) for x,y in pts if x>0.0 and y>0.0 and y<scale]
        if len(usable)<2: continue
        tx=[math.log10(x) for x,_ in usable]
        ty=[math.log((y/scale)/(1.0-y/scale)) for _,y in usable]
        c=list(np.polyfit(tx,ty,1)[::-1])
        a,b=c[0],c[1]
        sse=0.0; ok=True
        for x,y in usable:
            u=a+b*math.log10(x)
            yh=scale/(1.0+clamp_exp(-u))
            if not math.isfinite(yh): ok=False; break
            sse+=(y-yh)**2
        if not ok: continue
        if best is None or sse<best[3]: best=(scale,a,b,sse)
    return best
