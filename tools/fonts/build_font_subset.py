#!/usr/bin/env python3
"""生成内嵌用的 Mi Sans 子集字体。

用法：
    # 默认：从系统字体目录找 Mi Sans
    python3 tools/fonts/build_font_subset.py

    # 指定源字体目录
    python3 tools/fonts/build_font_subset.py --src /path/to/misans

输出：
    ui/assets/MiSans-Regular.ttf    （界面 + 导出图）
    ui/assets/MiSans-Demibold.ttf   （界面粗体 / font-weight:700）

依赖：pip install fonttools brotli

为什么要子集化：
    完整 Mi Sans 单字重约 8MB，两个字重 + 导出副本 ≈ 24MB。
    子集化到常用汉字后单份约 1.6MB，exe 只增大约 5MB，且覆盖 99.9% 日常用字；
    生僻字由 default-font-family 的 fallback 链回退系统字体兜底。

扩字集：
    修改本脚本里的 CHARS 构造（例如加更多 GBK 汉字），重新生成即可。
"""
import argparse
import shutil
import sys
from pathlib import Path

OUT_DIR = Path(__file__).resolve().parents[2] / "ui" / "assets"
WEIGHTS = [("Regular", 400), ("Demibold", 600)]


def build_chars() -> str:
    chars = set()
    # ASCII / Latin-1 补充
    chars |= {chr(c) for c in range(0x20, 0x7F)}
    chars |= {chr(c) for c in range(0xA0, 0x100)}
    # 科技/数学符号
    chars |= set("°±×÷μ—…·¹²³⁰⁴⁵⁶⁷⁸⁹⁺⁻⁼⁽⁾ⁿⁱ₀₁₂₃₄₅₆₇₈₉℃℉‰§¶†‡•′″")
    # 通用标点、上标下标、货币、字母式符号
    for lo, hi in [(0x2000, 0x2070), (0x20A0, 0x20BF), (0x2100, 0x214F)]:
        chars |= {chr(c) for c in range(lo, hi)}
    # CJK 标点、全角
    chars |= {chr(c) for c in range(0x3000, 0x3040)}
    chars |= {chr(c) for c in range(0xFF00, 0xFFF0)}
    # GB2312 全部汉字（6763）
    for b in range(0xB0, 0xF8):
        for b2 in range(0xA1, 0xFF):
            try:
                chars.add(bytes([b, b2]).decode("gb2312"))
            except Exception:
                pass
    # GBK 常见扩展（人名/地名字）
    chars |= set(
        "瞭煊煒燁玥璟軒鑫鈺鍾鎮鏞铉浥潁瀅灝煥玟琬瑄璇睿祎祯祺祾禛竑筠筱緹繽纭绾缨羿翊翎翔翰曜曦"
    )
    return "".join(sorted(chars))


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--src", default=None, help="Mi Sans 源字体所在目录")
    args = ap.parse_args()

    try:
        from fontTools.ttLib import TTFont
        from fontTools import subset
    except ImportError:
        print("需要 fontTools：pip install fonttools brotli", file=sys.stderr)
        return 1

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    chars = build_chars()
    tmp = OUT_DIR / "_chars.txt"
    tmp.write_text(chars, encoding="utf-8")
    print(f"字符集：{len(chars)} 个字符")

    for weight, wclass in WEIGHTS:
        src = None
        if args.src:
            src = Path(args.src) / f"MiSans-{weight}.ttf"
        else:
            for d in [
                Path("/usr/share/fonts/truetype/misans"),
                Path.home() / ".fonts",
                Path("/Library/Fonts"),
            ]:
                p = d / f"MiSans-{weight}.ttf"
                if p.exists():
                    src = p
                    break
        if src is None or not src.exists():
            print(f"找不到 MiSans-{weight}.ttf，用 --src 指定目录", file=sys.stderr)
            return 1

        out = OUT_DIR / f"MiSans-{weight}.ttf"
        # ① 子集化（不要用 --obfuscate-names，否则族名变乱码导致匹配失败）
        subset.main([
            str(src), f"--text-file={tmp}", f"--output-file={out}",
            "--layout-features=*", "--no-hinting",
            "--drop-tables+=DSIG,LTSH,VDMX,hdmx,PCLT",
        ])
        # ② 统一族名 + 字重：让两个字重属于同一 family，
        #    这样 font-weight:700 才会命中 Demibold 而不是合成粗体
        f = TTFont(out)
        f["name"].names = []
        def add(nid, s):
            f["name"].setName(s, nid, 3, 1, 0x409)
            f["name"].setName(s, nid, 1, 0, 0)
        add(1, "MiSans"); add(2, weight)
        add(4, f"MiSans {weight}"); add(6, f"MiSans-{weight}")
        add(16, "MiSans"); add(17, weight)
        if "OS/2" in f:
            f["OS/2"].usWeightClass = wclass
        f.save(out)
        print(f"  {out.name}: {out.stat().st_size/1024/1024:.2f} MB "
              f"(源 {src.stat().st_size/1024/1024:.2f} MB)")

    tmp.unlink()
    print("完成。运行 cargo build --release 生效。")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
