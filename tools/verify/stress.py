import math, random, sys
sys.path.insert(0,'/data/workspace/verify')
from replicate import *

random.seed(20261015)
rng = random.Random(7)

def gen(kind):
    if kind=='Logistic4':
        A=rng.uniform(0.5,3); D=rng.uniform(0.01,0.5); C=10**rng.uniform(0,2.3); B=rng.uniform(0.4,3)
        return [A,D,math.log(C),B], [0,0.3,1,3,8,20,50,120,300]
    if kind=='Logistic5':
        A=rng.uniform(0.5,3); D=rng.uniform(0.01,0.5); C=10**rng.uniform(0,2.3); B=rng.uniform(0.4,3); G=rng.uniform(0.4,3)
        return [A,D,math.log(C),B,math.log(G)], [0,0.3,1,3,8,20,50,120,300]
    if kind=='LogisticExp':
        A=rng.uniform(0.5,3); D=rng.uniform(0.01,0.5); C=rng.uniform(5,60); B=rng.uniform(0.02,0.4)
        return [A,D,C,B], list(range(0,101,10))
    if kind=='Hill':
        Bo=rng.uniform(0.01,0.5); To=rng.uniform(0.5,3); k=rng.uniform(1,80); n=rng.uniform(0.5,3)
        return [Bo,To,math.log(k),math.log(n)], [0,0.5,1,2,5,10,25,50,100,200]
    if kind=='Exp3':
        a=rng.uniform(-2,2); b=rng.uniform(-0.08,0.08); c=rng.uniform(-1,1)
        return [a,b,c], list(range(0,81,8))

print("="*88)
print("随机化压力测试：每个非线性模型 200 组随机真参数（无噪声）")
print("="*88)
summary={}
for kind in ['Logistic4','Logistic5','LogisticExp','Hill','Exp3']:
    worst=0.0; worstcase=None; nfail=0; nconv=0
    for it in range(40):
        p_true, xs = gen(kind)
        ys=[curve_value(kind,x,p_true) for x in xs]
        pts=list(zip(xs,ys))
        starts=initial_guesses(kind,pts)
        res=lm_fit(pts,kind,starts,150)
        if res is None: nfail+=1; continue
        p,sse=res
        canonicalize(kind,p)
        _,vals=describe_curve(kind,p)
        _,tv=describe_curve(kind,p_true)
        rel=max(abs(v-t)/max(1e-9,abs(t)) for v,t in zip(vals,tv))
        if sse>1e-14: nconv+=1   # 拟合未精确复原
        if rel>worst: worst=rel; worstcase=(p_true,vals,tv)
        if rel>1e-3: nfail+=1
    summary[kind]=(worst,nfail,nconv)
    print(f"  {kind:12s} 最大相对误差={worst:.2e}  超限次数={nfail}/40  SSE偏大次数={nconv}/40")
    if nfail: print(f"     最差样本 真值={[round(t,4) for t in worstcase[2]]} 还原={[round(v,4) for v in worstcase[1]]}")

print("\n"+"="*88)
print("加噪声鲁棒性（相对噪声 2%，每模型 30 组，检查参数是否仍在同一量级）")
print("="*88)
for kind in ['Logistic4','Logistic5','LogisticExp','Hill','Exp3']:
    bad=0
    for it in range(15):
        p_true,xs=gen(kind)
        ys=[curve_value(kind,x,p_true) for x in xs]
        pts=[(x,y*(1+rng.gauss(0,0.02))) for x,y in zip(xs,ys)]
        res=lm_fit(pts,kind,initial_guesses(kind,pts),150)
        if res is None: bad+=1; continue
        p,sse=res; canonicalize(kind,p)
        _,vals=describe_curve(kind,p); _,tv=describe_curve(kind,p_true)
        rel=max(abs(v-t)/max(1e-9,abs(t)) for v,t in zip(vals,tv))
        if rel>0.25: bad+=1
    print(f"  {kind:12s} 参数偏离真值 >25% 的组数: {bad}/15")

print("\n"+"="*88)
print("反算往返：由 Y 反解 X，再代入求值应回到原 Y")
print("="*88)
def invert(kind,p,y):
    if kind=='Logistic4':
        a,d,b=p[0],p[1],p[3]; t=(a-d)/(y-d)-1.0
        return None if t<=0 else math.exp(p[2])*t**(1.0/b)
    if kind=='Logistic5':
        a,d,b=p[0],p[1],p[3]; g=math.exp(p[4]); r=(a-d)/(y-d); t=r**(1.0/g)-1.0
        return None if t<=0 else math.exp(p[2])*t**(1.0/b)
    if kind=='LogisticExp':
        a,d,b=p[0],p[1],p[3]; u=(a-d)/(y-d)-1.0
        return None if u<=0 else p[2]-math.log(u)/b
    if kind=='Hill':
        bo,to=p[0],p[1]; n=math.exp(p[3]); u=(to-bo)/(y-bo)-1.0
        return None if u<=0 else math.exp(p[2])/u**(1.0/n)
    if kind=='Exp3':
        a,b,c=p[0],p[1],p[2]; u=(y-c)/a
        return None if u<=0 else math.log(u)/b

for kind in ['Logistic4','Logistic5','LogisticExp','Hill','Exp3']:
    p_true,xs=gen(kind)
    ys=[curve_value(kind,x,p_true) for x in xs]
    res=lm_fit(list(zip(xs,ys)),kind,initial_guesses(kind,list(zip(xs,ys))),150)
    p,_=res; canonicalize(kind,p)
    errs=[]
    for xt in xs[2:-2]:
        yt=curve_value(kind,xt,p)
        xi=invert(kind,p,yt)
        if xi is None: errs.append(float('inf')); continue
        errs.append(abs(curve_value(kind,xi,p)-yt))
    m=max(errs)
    print(f"  {kind:12s} 反算往返最大偏差={m:.2e}  {'OK' if m<1e-8 else 'FAIL'}")
