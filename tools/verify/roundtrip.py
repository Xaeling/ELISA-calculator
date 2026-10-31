import math, sys
sys.path.insert(0,'/data/workspace/verify')
from replicate import *

def fmt(v, d=4): return f"{v:.{d}f}"

# 真值： (kind, 内部参数向量 p_true, X 取值, 说明)
# 注意 p_true 是「内部参数化」的：C/k/n/G 以 ln 形式给出
CASES = [
    ("直线回归", None, None, None),
]

def poly_case(deg, coef, xs):
    ys = [sum(c*x**i for i,c in enumerate(coef)) for x in xs]
    return xs, ys

print("=" * 92)
print("全模型往返验证：真参数 -> 造数据(无噪声) -> 用本程序的流程拟合 -> 还原参数 -> 与真值比对")
print("=" * 92)

fails = []

# ---------- 1. 多项式：直接解，验证系数还原 ----------
def poly_fit_np(xs, ys, deg):
    return list(np.polyfit(xs, ys, deg)[::-1])  # 升幂

for deg, coef, xs, name in [
    (1, [2.0, 3.5], [0,1,2,3,4,5], "直线 Y=2+3.5X"),
    (2, [1.0, -2.0, 0.75], [0,1,2,3,4,5,6], "二次 Y=1-2X+0.75X²"),
    (3, [0.5, 1.0, -0.4, 0.08], [0,1,2,3,4,5,6,7], "三次 Y=0.5+X-0.4X²+0.08X³"),
]:
    ys = [sum(c*x**i for i,c in enumerate(coef)) for x in xs]
    got = poly_fit_np(xs, ys, deg)
    err = max(abs(g-c) for g,c in zip(got, coef))
    ok = err < 1e-6
    print(f"[{'OK ' if ok else 'FAIL'}] {name:26s} 真值={[fmt(c) for c in coef]} 还原={[fmt(g) for g in got]} 最大误差={err:.2e}")
    if not ok: fails.append(name)

# ---------- 2. 非线性：内部 ln 参数化 -> describe_curve 还原 ----------
NONLIN = [
    ("Logistic1(四参数)", 'Logistic4', [1.8, 0.1, math.log(25.0), 1.0],
     [0,1,3,6,12,25,50,100,200], "A=1.8 D=0.1 C=25 B=1.0"),
    ("Logistic1(递减)", 'Logistic4', [0.1, 1.8, math.log(30.0), 1.0],
     [0,2,5,10,20,40,80,160], "A=0.1 D=1.8 C=30 B=1.0 (X→0 起递增)"),
    ("Logistic2(五参数)", 'Logistic5', [2.0, 0.15, math.log(20.0), 1.2, math.log(1.8)],
     [0,1,3,6,12,20,40,80,160,320], "A=2.0 D=0.15 C=20 B=1.2 G=1.8"),
    ("Logistic3(指数四参)", 'LogisticExp', [2.0, 0.2, 30.0, 0.08],
     [0,5,10,20,30,40,60,80,100], "A=2.0 D=0.2 C=30 B=0.08"),
    ("Hill", 'Hill', [0.15, 1.9, math.log(20.0), math.log(1.5)],
     [0,1,3,6,12,20,40,80,160], "Bottom=0.15 Top=1.9 k=20 n=1.5"),
    ("指数(三参数)", 'Exp3', [1.5, -0.05, 0.3],
     [0,5,10,20,30,40,60,80], "a=1.5 b=-0.05 c=0.3"),
]

for name, kind, p_true, xs, desc in NONLIN:
    ys = [curve_value(kind, x, p_true) for x in xs]
    pts = list(zip(xs, ys))
    starts = initial_guesses(kind, pts)
    res = lm_fit(pts, kind, starts)
    if res is None:
        print(f"[FAIL] {name:26s} LM 未收敛")
        fails.append(name); continue
    p_hat, sse = res
    canonicalize(kind, p_hat)          # <- 本程序的规范化步骤
    names, vals = describe_curve(kind, p_hat)
    # 真值对应的「展示值」
    _, true_vals = describe_curve(kind, p_true)
    errs = [abs(v-t) for v,t in zip(vals, true_vals)]
    rel = [e/max(1e-9, abs(t)) for e,t in zip(errs, true_vals)]
    ok = (sse < 1e-12) and max(rel) < 1e-4
    print(f"[{'OK ' if ok else 'FAIL'}] {name:26s} SSE={sse:.2e}")
    print(f"        真值: {desc}")
    print(f"        还原: " + "  ".join(f"{n}={fmt(v)}" for n,v in zip(names, vals)))
    if not ok:
        print(f"        期望: " + "  ".join(f"{n}={fmt(t)}" for n,t in zip(names, true_vals)))
        print(f"        相对误差: {[f'{r:.2e}' for r in rel]}")
        fails.append(name)

# ---------- 3. Logit-log ----------
print("-" * 92)
a_t, b_t = 2.0, -1.5
xs = [1,3,10,30,100,300,1000]
ok_all = True
for scale_t in (1.0, 100.0):
    ys = [scale_t/(1.0+math.exp(-(a_t+b_t*math.log10(x)))) for x in xs]
    r = fit_logit_log(list(zip(xs, ys)))
    scale, a, b, _ = r
    ed50 = 10**(-a/b)
    ok = abs(scale-scale_t)<1e-9 and abs(a-a_t)<1e-6 and abs(b-b_t)<1e-6
    ok_all = ok_all and ok
    print(f"[{'OK ' if ok else 'FAIL'}] Logit-log(真scale={scale_t:5.1f})  判定scale={scale:5.1f}  a={fmt(a)}(真{a_t})  b={fmt(b)}(真{b_t})  ED50={fmt(ed50)}")
# OD 数据不应误判
pts_od = [(x,y) for x,y in zip([3.13,6.25,12.5,25.0,50.0,100.0,200.0],[1.265,1.136,0.803,0.527,0.343,0.244,0.209])]
sc,_,_,_ = fit_logit_log(pts_od)
ok_od = abs(sc-1.0)<1e-9
print(f"[{'OK ' if ok_od else 'FAIL'}] Logit-log(OD数据0.2~1.8)  判定scale={sc:.1f} (应为1，旧规则误判为100)")
if not ok_all or not ok_od: fails.append("LogitLog")

# ---------- 4. 用户的实测数据（含 X=0）----------
print("-" * 92)
xs = [0.0, 3.13, 6.25, 12.5, 25.0, 50.0, 100.0, 200.0]
ys = [1.756, 1.265, 1.136, 0.803, 0.527, 0.343, 0.244, 0.209]
pts = list(zip(xs, ys))
kind='Logistic4'
starts = initial_guesses(kind, pts)
res = lm_fit(pts, kind, starts)
p_hat, sse = res
canonicalize(kind, p_hat)
names, vals = describe_curve(kind, p_hat)
d = dict(zip(names, vals))
print(f"[    ] 实测用例(含X=0) SSE={sse:.6f}")
print(f"        还原: " + "  ".join(f"{n}={fmt(v,6)}" for n,v in zip(names, vals)))
c_ok = 5.0 < d['C'] < 15.0
print(f"[{'OK ' if c_ok else 'FAIL'}] C 为真实值而非 ln(C)  -> C={fmt(d['C'],6)}  (若是 {fmt(math.log(d['C']),6)} 说明又漏还原了)")
if not c_ok: fails.append("实测C")

print("=" * 92)
print("失败项:", fails if fails else "无 —— 全部通过")
