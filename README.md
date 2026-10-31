# ELISACalc 回归/拟合计算程序

用 **Slint 1.18.1** 复刻「ELISA Calc」的桌面拟合工具：左侧选模型 / 数据转换 / 功能入口，右侧录入 X（浓度/剂量）与 Y（反应值），一键拟合、看曲线、看方程、反算浓度、复制结果。

> 本版为纯 GUI 程序（Windows 下 **不弹黑色控制台窗口**）。

## 1. 功能

### 主界面功能入口（对应你给的界面草图）
| 按钮 | 作用 |
|---|---|
| **数据输入(E)** | 回到数据表，录入 X、Y；显示每行的 Y 拟合值与残差 |
| **回归方程(R)** | 查看拟合方程、参数、ED50/IC50、R²/SSE/RMSE、完整残差表与可复制的结果文本 |
| **由Y计算X** | 输入反应值 Y，反算浓度/剂量 X（解析反解优先，非单调模型会列出所有解） |
| **由X计算Y** | 输入 X，预测反应值 Y（超出数据范围会提示“属外推”） |

### 回归/拟合模型（10 种）
| 模型 | 表达式 |
|---|---|
| 直线回归 | Y = a + b·X |
| 二次曲线回归 | Y = a0 + a1·X + a2·X² |
| 三次曲线回归 | Y = a0 + a1·X + a2·X² + a3·X³ |
| Logit-log 直线回归 | ln[p/(1-p)] = a + b·log10(X)，输出 ED50/IC50 |
| Logistic 曲线拟合 1（四参数） | Y = D + (A-D)/[1 + (X/C)^B] |
| Logistic 曲线拟合 2（五参数） | Y = D + (A-D)/[1 + (X/C)^B]^G |
| Logistic 曲线拟合 3（指数四参数） | Y = D + (A-D)/[1 + e^(-B·(X-C))] |
| Hill 曲线拟合 | Y = Bottom + (Top-Bottom)·X^n/(k^n + X^n) |
| 指数曲线拟合 | Y = a·e^(b·X) + c |
| 三次样条插值 | 自然边界分段三次多项式，严格过每个数据点 |

### 数据转换（X 列、Y 列各自独立）
不转换、对数[2 为底]、对数[10 为底]、平方、平方根、指数、倒数。

### 其它
扣除本底（可填多个空白孔读数取均值）、拟合曲线绘制（X 轴可切对数坐标）、逐点拟合值与残差、结果一键复制（arboard）、Excel 两列整块粘贴导入、非法数据自动剔除并提示、帮助窗口。

## 2. 目录结构

```
elisa-calc-rs/
├── Cargo.toml
├── build.rs            # 编译 ui/main.slint
├── ui/main.slint       # 全部界面（Slint 声明式 UI）
├── README.md
└── src/
    ├── main.rs         # Slint 回调 + 业务逻辑 + 绘图路径生成
    ├── models.rs       # 10 种模型、参数命名、多起点初值、反函数求解
    ├── fitting.rs      # Householder QR 最小二乘、Levenberg-Marquardt、自然三次样条
    └── transform.rs    # X/Y 列转换
```

拟合算法与 GUI 完全解耦，`cargo test` 可直接验证。

## 3. 构建与运行

需要 Rust 1.82+（Slint 1.18 的要求）与联网拉取 crate：

```bash
cd elisa-calc-rs
cargo build --release     # 编译（首次会拉取 slint，约几分钟）
cargo run --release       # 直接运行
cargo test                # 运行拟合算法单元测试
```

Windows 双击 `target\release\elisa-calc-rs.exe` 即可，**不会弹出控制台窗口**。
产物是**单个 exe 文件**，可拷贝到任意电脑直接运行（免安装、绿色）。详见下一节。

### 关于“不显示黑色运行框”
`src/main.rs` 第一行：

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
```

- `cargo run --release` / 直接运行 exe：无控制台；
- `cargo run`（debug）：保留控制台，方便看日志；
- 若你希望 debug 也不出现控制台，把上面一行改成 `#![windows_subsystem = "windows"]`。

## 3.5 单文件免安装发布

### 它到底是不是“一个文件”？

是。`cargo build --release` 只产出 `target\release\elisa-calc-rs.exe` 一个文件：

- **`.slint` 界面文件已编译进 exe**：`build.rs` 在编译期把 `ui/main.slint` 翻译成 Rust 代码，运行时不读取任何外部 UI 文件；
- **没有外部资源**：界面未引用 `@image-url` 图片或自定义字体，中文字体直接用系统自带的微软雅黑；
- **不写注册表、不装服务、无安装目录**：纯绿色程序，拷到 U 盘或桌面就能跑。

需要留意的只有“动态依赖”两件事：

| 依赖 | 是否需要处理 |
|---|---|
| Windows 系统 DLL（kernel32 / user32 / gdi32 / d3d11 / dxgi / bcrypt 等） | 否，Win10/11 自带 |
| VC++ 运行库（vcruntime140.dll、msvcp140.dll） | **需要处理**，见下 |

默认情况下 MSVC 工具链会动态链接 VC++ 运行库，目标机若没装过 VC Redist 就会报“找不到 vcruntime140.dll”。本工程已自带 `.cargo/config.toml` 把它们静态链接进去，所以**默认构建的 exe 可以直接丢到没装任何东西的电脑上运行**。

### 构建命令

```bash
# MSVC 工具链（rustup 在 Windows 上的默认选择）
cargo build --release
# → target\release\elisa-calc-rs.exe   单文件，可直接拷走
```

若你用的是 GNU 工具链：

```bash
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
# → target\x86_64-pc-windows-gnu\release\elisa-calc-rs.exe
```

### 验证是否真的“零外部依赖”

方式一（VS 自带）：

```powershell
dumpbin /dependents target\release\elisa-calc-rs.exe
```

方式二：用 [Dependencies](https://github.com/lucasg/Dependencies) 打开 exe 看依赖树。

只要列表里全是 `C:\Windows\System32\...` 的系统 DLL，没有 `vcruntime140.dll` / `msvcp140.dll` / `libgcc_*.dll` / `libstdc++*.dll`，就是真正的单文件。最稳妥的验证：把 exe 单独拷进一个干净的虚拟机跑一次。

### 体积

Slint 默认同时编入 Skia 与 femtovg 渲染后端，release 后 exe 量级在**十几 MB**（不同版本略有出入）。本工程已开启 `lto = true`、`codegen-units = 1`、`strip = true` 做瘦身。

如果确实在意体积，可以只保留一个渲染后端（把 `Cargo.toml` 的 slint 依赖改为关闭默认特性再挑一个，`feature` 名称请以 `cargo add slint@1.18.1 --dry-run` 或 [Slint Cargo 文档](https://docs.rs/slint/1.18.1/slint/)核对为准）：

```toml
slint = { version = "1.18.1", default-features = false,
          features = ["std", "backend-winit", "renderer-femtovg"] }
```

### 其他注意事项

- **杀软误报**：Rust 静态链接的大型 exe 偶尔被 Windows Defender 误判。若被拦截，用 `cargo build --release` 的干净产物 + 代码签名可基本避免。
- **老机器无 GPU**：Slint 会自动回退到软件渲染；如需强制软件渲染，运行前设环境变量 `SLINT_BACKEND=software`。
- **不联网**：程序不访问网络，剪贴板与文件都在本地完成。

### 可选：换界面风格
Slint 内置多套风格，设置环境变量即可（Windows PowerShell）：

```powershell
$env:SLINT_STYLE = "native"; cargo run --release   # 更接近系统原生控件
```
可选值：`fluent`（默认）、`material`、`cupertino`、`cosmic`、`native`、`qt`。

### 剪贴板依赖
“复制”按钮使用 `arboard`。如果不需要，删除 `Cargo.toml` 里的 `arboard = "3.4"`，并把 `src/main.rs` 中 `on_copy_result` 内的剪贴板三行改为只提示即可（回归方程页本来就可以手动 Ctrl+C）。

## 4. 使用要点

1. 【数据输入(E)】里录入 X、Y（支持 `1.5e-3` 这类科学计数法）；也可从 Excel 复制两列 → 点【粘贴 / 导入数据】→ 粘贴 → 【解析填充】。**行数自动扩展**：表格行数 = max(解析行数, 20) + 3，粘贴 100 行也不会丢任何数据。
2. 处理顺序固定为：**扣除本底 → X/Y 列转换 → 拟合**。转换非法的行（如负数取对数、0 取倒数）会被剔除并在结果区提示。
3. Logit-log、Logistic 1/2、Hill 要求 X > 0（内部有对数/幂运算）；三次样条要求 X 严格递增且无重复值。
4. 非线性模型（Logistic / Hill / 指数）采用多起点 Levenberg-Marquardt，自动取残差平方和最小的解，一般无需手动给初值。
5. 【由Y计算X】对二次、三次、样条这类非单调模型会扫描全区间并列出**全部解**；若提示未找到解，说明该 Y 不在曲线值域内。
6. 【复制】把模型、参数、R²、残差表整套文本放进剪贴板，可直接贴回 Excel。

### 两种粘贴导入方式

- **方式一**：复制好 Excel 两列 → 打开【粘贴 / 导入数据】→ 点输入框 → Ctrl+V → 【解析填充】。
- **方式二（推荐）**：复制好后 → 打开【粘贴 / 导入数据】→ 直接点【从剪贴板粘贴】按钮，程序用 `arboard` 主动读取剪贴板，不依赖输入框的快捷键实现 → 【解析填充】。

> 说明：导入弹窗设置了 `close-policy: close-on-click-outside`，只有点击弹窗**外部**才关闭（默认是 `close-on-click`，点弹窗内部即关闭，根本来不及粘贴）。

## 5. 单元测试覆盖

- 直线/三次多项式系数还原、样条严格过点；
- 三参数指数、四参数 Logistic（C、B）、Hill（ED50 k）、Logit-log（ED50/IC50）的参数还原；
- 反算：直线/四参数/Hill/指数的往返一致性，以及二次曲线求根的“双解”验证；
- 各转换的合法/非法输入处理。

## 5.5 界面配色与双主题（深色 / 浅色，跟随系统）

内置两套完整配色，通过 `ui/main.slint` 里的 `global Theme` 提供：所有颜色都是
`dark ? 深色值 : 浅色值` 的派生属性，界面元素只引用 `Theme.xxx`，不写死颜色。

```slint
global Theme {
    in-out property <ThemeMode> mode: ThemeMode.system;   // system / light / dark
    in-out property <bool> system-dark: true;             // 由 Rust 检测后写入
    out property <bool> dark: root.mode == ThemeMode.system
            ? root.system-dark                            // 跟随系统
            : (root.mode == ThemeMode.dark);              // 手动强制
    out property <color> win-bg: dark ? #16191f : #eff2f6;
    // ...共 30+ 个令牌
}
```

### “跟随系统”是怎么实现的

不依赖 `Palette.color-scheme = ColorScheme.unknown`（它只在部分 Slint 风格下生效），
而是**由 Rust 主动查询系统设置**：

| 平台 | 检测方式 |
|---|---|
| Windows | `reg query HKCU\...\Themes\Personalize /v AppsUseLightTheme`（`0` = 深色）；调用时带 `CREATE_NO_WINDOW`，不会闪黑框 |
| macOS | `defaults read -g AppleInterfaceStyle`，含 `Dark` 即深色 |
| Linux | `gsettings get org.gnome.desktop.interface gtk-theme` |

程序启动时应用一次，之后用 `slint::Timer` **每 1.5 秒轮询**；检测到变化就调用
`apply-system-theme(bool)` 回调切换主题，所以在系统设置里改深浅色，界面约 1.5 秒内自动跟随。
（检测失败时保持上一次结果，不会来回跳。）

- **左侧面板「界面主题」下拉**：跟随系统 / 浅色 / 深色。手动选浅色或深色时，会同时把
  `Palette.color-scheme` 同步给原生控件（`LineEdit`/`TextEdit`），避免原生控件与自绘面板配色不一致；
- **自绘控件**（按钮、复选框、下拉框）的前景色/背景色/悬停色/禁用色全部走 `Theme` 令牌，两套主题下都保证对比度；
- **绘图区**配色也随主题切换（网格线、曲线、散点、坐标轴文字）；
- 想微调配色，只改 `global Theme` 里对应令牌的两组色值即可。

### 下拉框（现代样式）

`DropDown` 为自绘组件，风格与整体界面统一：

- 圆角 6px 边框，展开时边框变强调色、箭头翻转为向上；
- 弹出面板：圆角 8px + 1px 边框 + **投影**（`drop-shadow-*`）浮层效果；
- 选项行 32px：悬停高亮、选中项左侧 3px 强调色竖条 + 右侧对勾、文字变强调色；
- **箭头与对勾用 `Path` 自绘**，不使用 `▾`/`✓` 等字符 —— 避免系统字体缺字形时显示成小方块。

> 提示：若换风格（环境变量 `SLINT_STYLE=fluent/material/cupertino/cosmic/native`），
> 自绘部分不受影响，原生控件的观感会随风格变化。

## 5.8 批量反算：由 Y 算 X / 由 X 算 Y（表格化）

左侧【由Y计算X】【由X计算Y】两个按钮对应的页面，是**两张独立的数据表格**（默认各 20 行）：

| 列 | 说明 |
|---|---|
| No. | 行号 |
| 输入列 | Y（或 X），可编辑 |
| 结果列 | X（或 Y），只读 |
| 备注 | 提示信息，如「超出数据范围，属外推」「共 2 个解：…」「未找到解」 |

**功能按钮**

- **批量计算**：一次性算完所有填写了输入的行；
- **从剪贴板粘贴**：直接从 Excel 复制一列 Y 或 X 值粘进来。**行数自动扩展**（粘 100 个就给 103 行，不会丢数据）。若剪贴板是两列（Tab 或逗号分隔），自动取第一列；
- **复制结果**：把结果列复制成「**每个值一行**」的纯文本 —— 粘贴到 Excel 会自动排成一列；
- **+ 增加 5 行 / 清空**。

**多解的处理**：非单调模型（二次、三次、样条）由 Y 反算可能有多解。结果列给出第一个解（便于直接复制成数值列），**备注列列出全部解**；若 Y 不在曲线值域内，备注提示「未找到解」。

> 说明：结果列设为 `read-only`，仅作展示；输入列用 `edited` 回调回写到 Rust 端的 model，
> 保证 Slint 的 `for` 循环能正确刷新。

## 5.10 绘图区与导出图片

### 界面绘图区

- 画布 **500×280**（横向扁矩形，比早期的正方形更省地方；`ui/main.slint` 画布 `Rectangle` 与 `src/main.rs` 的 `PLOT_W/PLOT_H` 必须一致）；
- **实测点标记为正方形**（`border-radius: 0`），拟合曲线为红色折线；
- **导出相关控件在画布右侧**的独立面板（250px）：格式下拉 + 【导出曲线图片】按钮 + 说明。

### 坐标轴刻度

- **X 轴 50 一格**，**Y 轴 0.5 一格**（`src/main.rs` 的 `X_STEP`/`Y_STEP`，导出图在 `src/export.rs`）；
- 用 `models::axis_ticks(min, max, step, max_lines)` 按整数索引生成，避免浮点累加误差；
- 数据跨度过大导致格数超过 40 时，自动把步长放大 10 倍；
- **X=0 的点不再被剔除**：只有 Logit-log 依赖 `log10(X)`、X=0 无定义才剔除；Logistic(1/2/3)、Hill、指数在 X=0 处均有定义，零点照常参与拟合（测试 `zero_x_is_kept_for_hill` / `zero_x_is_kept_for_logistic`）。

### 导出图片（支持 HD / 2K / 4K / 8K）

侧边面板新增【分辨率】下拉：

| 选项 | 图片宽度 | 典型输出（10 行信息） | 内存占用 |
|---|---|---|---|
| HD | 1920 px | 1920×2000（3.8 MP） | ~15 MB |
| 2K | 2560 px | 2560×2667（6.8 MP） | ~26 MB |
| **4K（默认）** | 3840 px | 3840×4001（15.4 MP） | ~59 MB |
| 8K | 7680 px | 7680×8001（61.5 MP） | ~235 MB |

**关键点是“矢量级重绘”，不是位图拉伸**：所有图元（文字、线段、方块、曲线）
都按坐标程序化绘制，版面常量定义在「基准坐标系」（宽 860）里，实际输出时整体乘
`scale = 目标宽度 / 860`。因此 4K/8K 下线条依旧是 1 物理像素级锐利，文字由
`ab_glyph` 按放大后的字号重新栅格化。

```rust
let s = target_w as f64 / BASE_W;      // 4K → 4.465
let px = |v: f64| v * s;               // 长度缩放
let fz = |v: f32| v * s as f32;        // 字号缩放
```

- 换行（wrap）在**基准坐标系**里测量完成，保证不同分辨率下断行位置一致；
- 有像素总量保护（上限 96 MP），极端情况会自动降低倍率而不是耗尽内存；
- 8K 编码（尤其 PNG）可能需要数秒，状态栏会先提示“正在生成…”。

其余：内容含标题、模型公式、曲线图、**图例**、**拟合方程**、参数、ED50/IC50、
`n/SSE/RMSE/R²`、数据转换与本底说明、导出时间；格式 **JPEG（质量 95）/ PNG（无损）**，
保存在 exe 所在目录，状态栏显示完整路径；中文依赖系统字体，缺失时回退英文。

## 5.12 无边框窗口与自绘窗口控制

`no-frame: true` 隐藏原生标题栏，改用自绘的三个 mac 风格圆形按钮。
**核心原则：框架是 Slint（底层 winit），一切用 Slint 官方 Window API。**

已在 docs.rs 1.18.1 上逐条确认存在：

```rust
window.set_minimized(true);                    // 最小化
window.set_maximized(!window.is_maximized());  // 最大化 / 还原切换
window.is_maximized();                          // 查询状态
slint::quit_event_loop(); window.hide();        // 关闭
```

> Slint 的 `Window` **没有** `drag_window()`，所以拖动必须自己算（见下）。
> 另外 Slint 也**没有**边缘缩放 API。

顺序与符号（左→右，**常显**，不依赖 hover）：

| 顺序 | 颜色 | 功能 | 符号 |
|---|---|---|---|
| ① | 黄 `#febc2e` | 最小化 | `−`（横线长 10px / 圆圈 15px） |
| ② | 绿 `#28c840` | 最大化 / 还原 | `□` |
| ③ | 红 `#ff5f57` | 关闭 | `×` |

**标题行布局**：用 `Rectangle` 容器 + 绝对定位（不用 `HorizontalLayout`）：

- 拖动区 `TouchArea` 铺满整行（34px 高）→ 整行任意位置都能拖，只有右上角
  那 61px（3×15 + 2×8）归按钮；
- 按钮组 `x: parent.width - 61px; y: (parent.height - 15px)/2` →
  **圆圈中线 == 标题行中线**（水平对齐），且贴右上角；
- 声明顺序决定层级：标题 `Text`（不接收鼠标事件）→ 拖动区 → 按钮组（最上层，
  拦截自己范围内的点击，不会误触发拖动）。

### 拖动：为什么必须用「屏幕位移」而不是「窗口内位移」

这是上一版闪烁的真因。

Slint 的 `TouchArea` 只能给出**鼠标在窗口内的坐标** `M`。若用
`ΔM = M − M0` 移动窗口，由于 `M = S − W`（S 为鼠标真实屏幕位置、W 为窗口位置），
于是 `ΔM` 本身依赖 `W` → `W` 与 `ΔM` 互相拉扯 → 表现为拖动时持续抖动/闪烁。

**关键洞察**：鼠标的真实屏幕位置 `S = W + M × scale` 是**恒定**的
（移动窗口不会改变鼠标在屏幕上的位置）。所以用屏幕位移即可消除反馈循环：

```
按下：记录 W0，以及 S0 = W0 + M0 × scale
移动：计算 S = W + M × scale（W 为当前窗口位置）
      取 ΔS = S − S0
      新窗口位置 W = W0 + ΔS
```

`ΔS` 与 `W` 无关 → 无循环 → 平滑不闪，**且跨平台**（不需要 Win32）。

### 曾踩过的坑（勿重复）

| 坑 | 后果 |
|---|---|
| 用 `WM_SYSCOMMAND` 实现 min/max/close | 三个按钮**全部失效**：`sys_command()` 只按“是否找到句柄”返回，消息实际未生效却提前 `return`，把可用的 Slint 路径跳过了 |
| 拖动用 `ΔM`（窗口内位移） | 拖动时持续闪烁 |

一次性动作（min/max/close）**永远用 Slint API**，不碰 Win32。

### 已知限制

- 无边框窗口**不能拖拽边缘缩放**（Slint 无对应 API，原生边框已被移除）；
- Windows 下无边框窗口默认没有投影（可另用 `DwmExtendFrameIntoClientArea` 补，本项目未启用以保持零 FFI）；
- 双击标题栏可切换最大化；拖动标题栏任意空白处可移动窗口。

### 导出清晰度

影响观感的三个因素，都已处理：

| 因素 | 处理 |
|---|---|
| 分辨率 | 支持 HD / 2K / **4K（默认）** / 8K，整体等比重绘（非位图拉伸） |
| 曲线折线感 | **按输出宽度重新采样曲线**：界面只用 260 点，导出 4K 时曲线横跨数千像素会出现折线，这里按「绘图区物理宽度 × 2」重采样（上限 20000 点） |
| 压缩伪影 | JPEG 质量由 95 提到 **98**，减少文字与曲线边缘的振铃伪影；也可直接选 PNG（无损） |

线段描边用圆盘序列，步长按「半径的一半」推进，保证相邻圆盘充分重叠（否则
高分辨率下会出现串珠状断续），边缘做 1 物理像素羽化。

## 5.13 交互动效

### 按钮：点击反馈（收缩 → 旋转加载 → 成功对勾 → 恢复）

时序与实现：

| 阶段 | 时长 | 实现 |
|---|---|---|
| 收缩成圆形 | 300ms | `animate width, height, x, y, border-radius { duration: 300ms; easing: spring(0.45); }` |
| 旋转加载 | 直至完成 | 12 个点按相位依次亮暗形成旋转彗尾 |
| 成功对勾 | — | 绿色实心 + 白色 ✓ |
| 恢复原状 | 完成后 1s | Rust 侧 `Timer::single_shot` 清除 done |

- **弹簧曲线**用 Slint 内置的 `spring(bounce)`（`bounce ∈ [-1,1]`），
  取 0.45 —— 释放时有轻微超调回弹，符合物理手感；
- **按压反馈**：`sq = pressed ? 0.96 : 1.0` 作用在宽高上（居中补偿 x/y），
  配合 **`inner-shadow-*`**（Slint 1.18 的原生内阴影）制造按下去的深度感，
  释放时由同一条 spring 曲线回弹超调；
- **旋转指示器**：Rectangle **没有旋转属性**，所以不能用 `rotation-angle`。
  改为「12 个点 + 相位推进」：每个点亮度 = `1 - 相位滞后`，随时间依次点亮，
  视觉上就是旋转彗尾。时间轴用 `animation-tick()`（不需要 Rust 定时器）；
- 指示器放在 `if root.busy:` 内 —— **不忙时不创建**，避免常驻重绘开销。

Rust 侧调度（`with_feedback`）：

```rust
set_busy(true);
Timer::single_shot(300ms, || {           // 等收缩动画播完再干活
    let ok = task();                     // 同步耗时任务
    if ok { set_done(true); }            // 成功 → 对勾
    set_busy(false);
    Timer::single_shot(1s, || set_done(false));  // 1 秒后恢复
});
```

> 说明：拟合、8K 导出等任务是同步执行的，执行期间界面会短暂无响应
> （收缩动画已播完，不影响观感）。若需要完全不卡顿，可把这些任务移到后台线程。

带反馈的按钮：【回归/拟合】【复制】【批量计算 X】【批量计算 Y】【导出曲线图片】。

### 按压动画：覆盖所有可点击控件

| 控件 | 压缩比 | 额外反馈 |
|---|---|---|
| `MyButton`（全部按钮） | 0.96 | 内阴影 + spring 超调回弹 |
| `DropDown`（下拉框头部） | 0.97 | 内阴影 + spring 回弹 |
| `MyCheckBox`（复选框方框） | 0.88 | 内阴影 + spring 回弹 |

统一做法：控件本体 `background: transparent`，可见形状交给内部 `body`
承担，通过 `body.width/height = 尺寸 × sq` 并补偿 `x/y` 居中实现缩放；
`animate` 用 `spring(0.5)`，释放时自然产生轻微超调。

### 下拉弹窗：状态同步与点击次数（四个坑）

**坑 1：箭头只翻一次**（最初）
`PopupWindow` 没有 closed 回调，关闭由 Slint 执行，我们无从得知。

**坑 2：`no-auto-close` 会让点击全部失效**
`PopupWindow` 是一层覆盖全窗口的输入层；设成 `no-auto-close` 时，点在弹窗外的
事件被这一层吃掉（既不关闭、也不下传）→ 只能点选项才能关。

**坑 3：不能用“超大弹窗”当遮罩**
Slint 会把 PopupWindow 约束到窗口范围内并把原点对齐窗口左上，面板（位于弹窗内的
大偏移处）因此被推到窗口外 → **弹窗内容完全不显示**。

**坑 4：窗口级遮罩会吞掉一次点击**（本次修复的核心）
在 `MainWindow` 上放全屏遮罩来复位状态，结果点击外部时：弹窗关闭（Slint），
但那次点击命中了遮罩而非目标控件，于是还要再点一次 —— 表现为
“第一下打开、第二下关闭、第三下才点到别的控件”，甚至遮罩因状态残留而长期挡住界面。

**最终方案**：删掉窗口级遮罩，改用**全局单例 + 各交互入口复位**。

```slint
global DdState { in-out property <int> open-id: -1; }   // -1 = 无展开

// DropDown 组件内
in property <int> dd-id: -1;
property <bool> open: DdState.open-id == root.dd-id && root.dd-id >= 0;
// 头部：toggle；选项：置 -1
// MyButton / MyCheckBox / 各输入框 edited：置 -1
// 另在 root-box **最底层**放一个全屏 TouchArea（点击真正的空白时命中）：置 -1
```

要点：

- 弹窗关闭仍交给 Slint（`close-policy: close-on-click`，默认值）；
  **关键**：该点击会**下传**给下方元素，所以点一次就能同时“关闭弹窗 + 触发目标控件”；
- 之所以用 `global` 而不是层层回调：任何组件（含 `MyButton` 这种通用组件）
  都能直接读写 `DdState.open-id`，无需把回调透传下去；
- 底层全屏 `TouchArea` 作为**第一个**子元素 → 只在点击真正空白处命中，
  不会挡住任何控件，用于兜底复位（点空白关闭弹窗的情况）。

> 残留限制：Slint 的 PopupWindow 无法实现“点击外部时同时关闭并把该次点击
> 透传给外部控件”之外的行为；本方案已把总点击次数降到最少（1 打开、2 操作目标）。

### 图标一律用 Path 绘制，不用字符

字符类“图标”（`✓` `▾` `×`）在部分字体里**没有对应字形**，会渲染成小方块。
现已全部改为 `Path` 绘制：

| 位置 | 原来 | 现在 |
|---|---|---|
| 复选框对勾 | `Text { text: "✓" }` | `Path { commands: "M 3 13 L 9.5 19.5 L 21 6" }` |
| 下拉箭头 | `▾` | `Path`（展开时翻转） |
| 删除行叉号 | `Text { text: "×" }` | `MyButton { x-icon: true }` → `Path` |
| 成功对勾 | — | `Path`（白色 ✓） |

`MyButton` 新增 `x-icon: bool` 属性，启用后隐藏文字、改用 Path 画叉。


### 卡片：流动渐变描边 + 呼吸弥散背光

`FancyCard` 组件（用于反算页两张卡、帮助弹窗内容区）：

- **描边**：一条 3 倍宽的渐变带在卡片内水平循环平移，中间被内容层遮住，
  只露出 2px 边缘 —— 因为 Rectangle 无法旋转，用「渐变平移」实现等效的
  **旋转渐变描边**效果；渐变首尾同色，循环无缝。
- **背光**：卡片底部一个圆角矩形，用 `drop-shadow-blur` 做**动态模糊**，
  模糊半径与 y 偏移随 `animation-tick()` 呼吸（三角波 `0→1→0`，不依赖 sin）。

两者都由 `animation-tick()` 驱动，无需 Rust 定时器；
组件提供 `fx: false` 可关闭动画（省电 / 低性能机器）。

## 5.14 R² 上标渲染与可信度标识

### 上标 ² 显示为小方块的问题

`²`（U+00B2）在 simhei / 微软雅黑 等常见中文字体里**没有字形**，直接渲染
会得到小方块（缺字形回退）。解决办法：**不依赖字体，自己画** —— 在
`Canvas::text` / `text_w` 中识别 Unicode 上标/下标字符，改用
**0.62 倍字号 + 基线上移 0.40 倍**绘制。

```rust
fn sup_base(ch: char) -> Option<char> {
    match ch {
        '\u{00B2}' => Some('2'),   // ²
        '\u{00B3}' => Some('3'),   // ³
        // …覆盖 ⁰¹²³⁴⁵⁶⁷⁸⁹⁺⁻⁼⁽⁾ⁿⁱ
        _ => None,
    }
}
```

排版侧同步处理：`text_w()` 对上标按缩小后的字号计算宽度，因此换行（wrap）
位置也正确。

### R² 可信度分级

按免疫分析（ELISA）常规质控习惯划分，界面与导出图**同步显示**：

| R² | 等级 | 徽章 | 颜色 | 含义 |
|---|---|---|---|---|
| ≥ 0.99 | High | 可信度高 | 绿 `#108a50` | 拟合优度良好，可用于反算 |
| 0.95 ~ 0.99 | Mid | 可信度中等 | 橙 `#c4740c` | 建议检查离群点或换模型 |
| < 0.95 | Low | 可信度低 | 红 `#be2d2d` | 不建议用于反算 |

- **界面**：统计行右侧显示圆角徽章（含 R² 数值），淡入出现；下方一行同色提示语；
- **导出图**：统计行右侧画同色徽章，信息区追加一条建议语，颜色与徽章一致；
- **插值模型例外**：三次样条的 R² 恒为 1，不代表拟合优度，此时**不显示徽章**，
  只保留原提示「该模型为插值…R² 恒为 1」。

徽章配色集中在 `export.rs`（`BADGE_*_BG` 常量），界面通过
`r2_colors()` 取同一组值，保证两处一致。

新增单元测试：`r2_grade_thresholds`、`r2_badge_colors_are_distinct`、
`superscript_chars_map_to_base`、`superscript_narrows_measured_width`、
`text_with_superscript_does_not_panic`。

## 5.15 圆角窗口

`Window` 元素**不支持** `border-radius`，所以圆角要靠「透明窗口 + 圆角容器裁剪」：

```slint
export component MainWindow inherits Window {
    background: transparent;      // 四角透出桌面

    root-box := Rectangle {
        x: 0; y: 0; width: 100%; height: 100%;
        border-radius: 12px;
        clip: true;               // 关键：裁掉四角
        background: Theme.win-bg;
        border-width: 1px;
        border-color: Theme.border;

        VerticalLayout { x: 0; y: 0; width: 100%; height: 100%; padding: 10px; ... }
        // …下拉遮罩也放在这里
    }
}
```

两个必须注意的点：

1. **`Rectangle` 不是布局容器**，子元素不会自动填充 —— 内层的
   `VerticalLayout` 必须显式写 `width: 100%; height: 100%`，否则会退化到
   preferred size、整个布局塌掉；
2. 遮罩 `dd-shield` 也要放进 `root-box` 内，才能被圆角裁剪。

> 若所在平台/后端不支持窗口透明（四角发黑），把 `Window.background` 改回
> `Theme.win-bg` 即可退化成方角窗口，内部 UI 不受影响。

## 5.16 外侧框线对齐

原先「视图区」有外框而下方「绘图区」没有，底部一大块内容裸着，视觉上不齐。
现在统一为同一套外框（`1px` / `Theme.border` / 圆角 `4px`）：

| 区域 | 外框 |
|---|---|
| 左：功能入口与控制 | 有 |
| 右上：数据 / 方程 / 反算 | 有 |
| 右下：绘图区 + 侧边导出面板 | **新增** |

同时去掉了画布与侧边面板遗留的 `y: 2px` 偏移（那是早期为躲边框加的 hack），
改由外层 `HorizontalLayout` 统一居中，顶边严格对齐。

## 5.17 导出面板的提示文案

原则：**静态说明越少越好，结果反馈要明确**。

- 删除了原来的长句「含图例、拟合方程、参数与 R²；按分辨率重新绘制，
  4K/8K 依然锐利。图片保存在程序所在目录。」—— 这些内容在帮助里已有，
  堆在面板底部只是噪音；
- 保留「坐标：X 轴 50 一格，Y 轴 0.5 一格」（与当前刻度设置强相关，有用）；
- **导出成功后**在按钮下方出现一个着重标注的块：

```slint
if root.export-saved-show: Rectangle {
    background: Theme.ok-soft;      // 柔和绿底
    border-color: Theme.ok-solid;   // 绿色边框
    // ✓ 已保存到程序所在目录
    // <文件名>
}
```

用带底色 + 强调色边框 + `Path` 对勾，比一行灰字显眼得多；
文件名从 `path.file_name()` 取，只显示文件名（完整路径仍在状态栏）。

Rust 侧：导出成功设置 `export-saved-show/name`；**导出失败或清除数据/重新拟合时
主动清掉**，避免上一次的成功提示误导用户。

## 5.18 界面字体：内嵌 Mi Sans（不依赖系统安装）

字体文件放在 `ui/assets/`，**编译期打进二进制**，运行时不读任何字体文件：

```
ui/assets/MiSans-Regular.ttf    1.6 MB   → 界面 + 导出图
ui/assets/MiSans-Demibold.ttf   1.6 MB   → 界面 font-weight:700
```

### 三个接入点

| 位置 | 做法 |
|---|---|
| 界面 | `ui/main.slint` 顶部 `import "assets/MiSans-Regular.ttf";` 与 Demibold 同上，Slint 编译器内嵌 |
| 界面族名 | `default-font-family: "MiSans, Mi Sans, Microsoft YaHei, …"` |
| 导出图 | `src/export.rs`：`const EMBEDDED_MISANS: &[u8] = include_bytes!("../ui/assets/MiSans-Regular.ttf");` |

`build.rs` 里加了 `cargo:rerun-if-changed=ui/assets/*.ttf`，改字体会自动重编译。

### 为什么要子集化

完整 Mi Sans 单字重 **7.7 MB**；两个界面字重 + 导出副本 ≈ 24 MB。
子集化后单份 **1.6 MB**，exe 只增大约 **5 MB**。

字集：ASCII + Latin-1 + 常用科技符号 + CJK 标点/全角 + **GB2312 全部汉字
（6763）** + 少量 GBK 常见字，共 **7436 字形**。
生僻字由 `default-font-family` 的 fallback 链回退系统字体兜底 → **不会变方块**。

需要扩字集：改 `tools/fonts/build_font_subset.py` 里的字符集后重跑即可。

### 两个关键坑

1. **不要用 `--obfuscate-names`**：pyftsubset 该选项会把字体名混淆成乱码
   （实测变为 `.\x7f`），导致 `default-font-family: "MiSans"` 匹配不上、字体不生效。
2. **两个字重必须统一族名**：Mi Sans 各字重原生的 family 是独立的
   （`MiSans Demibold`、`MiSans Heavy` …）。必须把 name 表改成
   `family="MiSans"` + `usWeightClass=400/600`，否则 `font-weight: 700`
   只会由引擎**合成**粗体，中文字形观感明显变差。

### 许可

Mi Sans 由小米发布。**请自行确认其使用条款符合你的分发/商用场景。**

