// Windows 下以 release 编译时，不弹出黑色控制台窗口（子系统改为 windows）。
// 如果希望 debug 构建也不出现控制台，把下一行改成： #![windows_subsystem = "windows"]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod export;
mod fitting;
mod models;
mod theme;
mod transform;

// Slint 的 Model trait 与本项目 models::Model 枚举同名，这里用别名引入：
// 只要 trait 在作用域内（无论叫什么名字），row_count / row_data 等方法即可调用。
use slint::Model as SlintModel;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

use models::{FitResult, Model};
use transform::Transform;

slint::include_modules!();

/// 绘图区像素尺寸（横向扁矩形；与 ui/main.slint 中画布 Rectangle 的 width/height 保持一致）
const PLOT_W: f64 = 500.0;
const PLOT_H: f64 = 280.0;
/// 坐标轴格距：X 轴 50 一格，Y 轴 0.5 一格
const X_STEP: f64 = 50.0;
const Y_STEP: f64 = 0.5;
/// 默认表格行数
const DEFAULT_ROWS: usize = 20;
/// 导入/增加行后末尾额外保留的空行数，方便继续录入
const TAIL_ROWS: usize = 3;

/// 导出图片时需要、但 FitResult 里没有的元信息
struct Meta {
    name: String,
    model: Model,
    x_label: String,
    y_label: String,
    blank_note: String,
    log_x: bool,
    msgs: Vec<String>,
}

struct State {
    result: Option<FitResult>,
    meta: Option<Meta>,
    /// 最近一次生成的完整结果文本（供“复制”使用）
    text: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            result: None,
            meta: None,
            text: String::new(),
        }
    }
}

// ---------------- 小工具 ----------------

fn col(m: &ModelRc<SharedString>) -> Vec<String> {
    (0..m.row_count())
        .map(|i| m.row_data(i).unwrap_or_default().to_string())
        .collect()
}

fn str_model(v: Vec<String>) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        v.into_iter().map(SharedString::from).collect::<Vec<_>>(),
    ))
}

fn kv_model(v: Vec<(String, String)>) -> ModelRc<KV> {
    ModelRc::new(VecModel::from(
        v.into_iter()
            .map(|(k, v)| KV { k: SharedString::from(k), v: SharedString::from(v) })
            .collect::<Vec<_>>(),
    ))
}

/// 解析本底输入框（支持逗号/分号/空格分隔的多个读数，取均值）
fn parse_blank(s: &str) -> Option<f64> {
    let vals: Vec<f64> = s
        .split(|c: char| c == ',' || c == ';' || c == ' ' || c == '\t' || c == '，' || c == '；')
        .filter_map(|t| t.trim().parse::<f64>().ok())
        .collect();
    if vals.is_empty() {
        None
    } else {
        Some(vals.iter().sum::<f64>() / vals.len() as f64)
    }
}

// ---------------- 表格 ----------------

/// 行号文本数组（"1"、"2"...），UI 直接显示，避免依赖 .slint 里的字符串插值
fn row_numbers(n: usize) -> Vec<String> {
    (1..=n).map(|i| i.to_string()).collect()
}

/// 统一写入表格四列 + 行号，保证所有数组等长
fn set_table(ui: &MainWindow, xs: Vec<String>, ys: Vec<String>, fits: Vec<String>, resids: Vec<String>) {
    let n = xs.len().min(ys.len());
    let xs: Vec<String> = xs.into_iter().take(n).collect();
    let ys: Vec<String> = ys.into_iter().take(n).collect();
    let fits: Vec<String> = fits.into_iter().take(n).chain(std::iter::repeat(String::new()))
        .take(n).collect();
    let resids: Vec<String> = resids.into_iter().take(n).chain(std::iter::repeat(String::new()))
        .take(n).collect();
    ui.set_xs(str_model(xs));
    ui.set_ys(str_model(ys));
    ui.set_fits(str_model(fits));
    ui.set_resids(str_model(resids));
    ui.set_row_no(str_model(row_numbers(n)));
}

/// 重置为 n 个空行
fn reset_table(ui: &MainWindow, n: usize) {
    let empty = vec![String::new(); n];
    set_table(ui, empty.clone(), empty.clone(), empty.clone(), empty);
}

/// 用解析好的两列数据填充表格。
/// 行数自动扩展：保留全部解析行，且总行数不少于 DEFAULT_ROWS，末尾再留 3 个空行方便继续录入，
/// 因此粘贴任意多行都不会丢数据。
fn fill_table(ui: &MainWindow, mut rows: Vec<(String, String)>) {
    let n = rows.len();
    // 总行数至少 DEFAULT_ROWS；再多留 TAIL_ROWS 个空行
    let total = n.max(DEFAULT_ROWS) + TAIL_ROWS;
    while rows.len() < total {
        rows.push((String::new(), String::new()));
    }
    let xs: Vec<String> = rows.iter().map(|r| r.0.clone()).collect();
    let ys: Vec<String> = rows.iter().map(|r| r.1.clone()).collect();
    let empty = vec![String::new(); rows.len()];
    set_table(ui, xs, ys, empty.clone(), empty);
    clear_results(ui);
    ui.set_status(SharedString::from(format!(
        "已导入 {} 行数据（表格共 {} 行，行数已自动扩展）。",
        n,
        rows.len()
    )));
}

fn clear_results(ui: &MainWindow) {
    ui.set_equation(SharedString::from("（尚未拟合）"));
    ui.set_stats(SharedString::new());
    ui.set_notes(SharedString::new());
    ui.set_r2_badge_show(false);
    ui.set_r2_badge_text(SharedString::new());
    ui.set_r2_advice(SharedString::new());
    // 曲线变了，上一次的导出结果提示不再适用
    ui.set_export_saved_show(false);
    ui.set_export_saved_name(SharedString::new());
    ui.set_params(ModelRc::new(VecModel::from(Vec::<KV>::new())));
    ui.set_extras(ModelRc::new(VecModel::from(Vec::<KV>::new())));
    let none: Vec<String> = Vec::new();
    ui.set_res_x(str_model(none.clone()));
    ui.set_res_y(str_model(none.clone()));
    ui.set_res_yh(str_model(none.clone()));
    ui.set_res_res(str_model(none.clone()));
    ui.set_res_no(str_model(none));
    ui.set_has_plot(false);
    ui.set_grid_path(SharedString::new());
    ui.set_curve_path(SharedString::new());
    ui.set_dots(ModelRc::new(VecModel::from(Vec::<Dot>::new())));
    ui.set_x_ticks(ModelRc::new(VecModel::from(Vec::<Tick>::new())));
    ui.set_y_ticks(ModelRc::new(VecModel::from(Vec::<Tick>::new())));
    ui.set_result_text(SharedString::new());
}

/// [u8; 4] RGBA → Slint 颜色（徽章配色与 export.rs 共用同一组常量）
fn egui_color(c: [u8; 4]) -> slint::Color {
    slint::Color::from_argb_u8(c[3], c[0], c[1], c[2])
}

// ---------------- 按钮反馈：busy → done → 1s 恢复 ----------------

/// 哪些按钮带加载反馈
#[derive(Clone, Copy)]
enum Btn {
    Fit,
    Export,
    Yx,
    Xy,
    Copy,
}

fn set_busy(ui: &MainWindow, b: Btn, v: bool) {
    match b {
        Btn::Fit => ui.set_fit_busy(v),
        Btn::Export => ui.set_export_busy(v),
        Btn::Yx => ui.set_yx_busy(v),
        Btn::Xy => ui.set_xy_busy(v),
        Btn::Copy => ui.set_copy_busy(v),
    }
}

fn set_done(ui: &MainWindow, b: Btn, v: bool) {
    match b {
        Btn::Fit => ui.set_fit_done(v),
        Btn::Export => ui.set_export_done(v),
        Btn::Yx => ui.set_yx_done(v),
        Btn::Xy => ui.set_xy_done(v),
        Btn::Copy => ui.set_copy_done(v),
    }
}

/// 点击反馈时序：
///   1. 立即置 busy → 按钮用 300ms 弹簧曲线收成圆形、显示旋转指示器；
///   2. 等收缩动画播完（300ms）后执行任务，避免同步耗时操作卡住动画；
///   3. 任务成功 → 置 done（绿色对勾）；失败 → 直接恢复；
///   4. 再过 1s 清掉 done，按钮弹回原状。
fn with_feedback(w: slint::Weak<MainWindow>, b: Btn, task: impl FnOnce() -> bool + 'static) {
    let Some(ui) = w.upgrade() else {
        return;
    };
    set_busy(&ui, b, true);

    let w2 = w.clone();
    slint::Timer::single_shot(std::time::Duration::from_millis(300), move || {
        let Some(ui) = w2.upgrade() else {
            return;
        };
        let ok = task();
        if ok {
            set_done(&ui, b, true);
        }
        set_busy(&ui, b, false);

        let w3 = w2.clone();
        slint::Timer::single_shot(std::time::Duration::from_millis(1000), move || {
            if let Some(ui) = w3.upgrade() {
                set_done(&ui, b, false);
            }
        });
    });
}

// ---------------- 拟合 ----------------

fn do_fit(ui: &MainWindow, st: &Rc<RefCell<State>>) {
    let xs = col(&ui.get_xs());
    let ys = col(&ui.get_ys());
    let model = Model::ALL
        .get(ui.get_model_index().max(0) as usize)
        .copied()
        .unwrap_or(Model::Linear);
    let xtf = Transform::ALL
        .get(ui.get_x_tf_index().max(0) as usize)
        .copied()
        .unwrap_or(Transform::None);
    let ytf = Transform::ALL
        .get(ui.get_y_tf_index().max(0) as usize)
        .copied()
        .unwrap_or(Transform::None);
    let blank = if ui.get_blank_on() {
        parse_blank(&ui.get_blank())
    } else {
        None
    };

    let mut msgs: Vec<String> = Vec::new();
    if ui.get_blank_on() && blank.is_none() {
        msgs.push("本底值为空或无法解析，本次未扣除本底。".to_string());
    }

    // 扣除本底 → 列转换
    let mut rows: Vec<(usize, f64, f64)> = Vec::new();
    let n = xs.len().min(ys.len());
    for i in 0..n {
        let sx = xs[i].trim();
        let sy = ys[i].trim();
        if sx.is_empty() && sy.is_empty() {
            continue;
        }
        if sx.is_empty() || sy.is_empty() {
            msgs.push(format!("第 {} 行 X 或 Y 为空，已跳过。", i + 1));
            continue;
        }
        let x: f64 = match sx.parse::<f64>() {
            Ok(v) => v,
            Err(_) => {
                msgs.push(format!("第 {} 行 X=\"{}\" 不是有效数字，已跳过。", i + 1, sx));
                continue;
            }
        };
        let y: f64 = match sy.parse::<f64>() {
            Ok(v) => v,
            Err(_) => {
                msgs.push(format!("第 {} 行 Y=\"{}\" 不是有效数字，已跳过。", i + 1, sy));
                continue;
            }
        };
        let y0 = y - blank.unwrap_or(0.0);
        let x2 = match xtf.apply(x) {
            Some(v) => v,
            None => {
                msgs.push(format!(
                    "第 {} 行 X={} 无法进行“{}”转换，已剔除。",
                    i + 1,
                    models::fmt(x),
                    xtf.label()
                ));
                continue;
            }
        };
        let y2 = match ytf.apply(y0) {
            Some(v) => v,
            None => {
                msgs.push(format!(
                    "第 {} 行 Y={} 无法进行“{}”转换，已剔除。",
                    i + 1,
                    models::fmt(y0),
                    ytf.label()
                ));
                continue;
            }
        };
        rows.push((i, x2, y2));
    }

    let mut fits = vec![String::new(); n];
    let mut resids = vec![String::new(); n];

    if rows.len() < model.min_points() {
        {
            let mut s = st.borrow_mut();
            s.result = None;
            s.text.clear();
        }
        clear_results(ui);
        ui.set_status(SharedString::from(format!(
            "有效数据点 {} 个，不足：“{}”至少需要 {} 个点。",
            rows.len(),
            model.label(),
            model.min_points()
        )));
        set_table(&ui, xs, ys, fits, resids);
        return;
    }

    let data: Vec<(f64, f64)> = rows.iter().map(|(_, x, y)| (*x, *y)).collect();
    match models::fit(model, &data) {
        Some(r) => {
            for (i, x, y) in &rows {
                if let Some(v) = r.evaluator.eval(*x) {
                    fits[*i] = models::fmt(v);
                    resids[*i] = models::fmt(y - v);
                }
            }
            ui.set_equation(SharedString::from(r.equation.clone()));
            ui.set_params(kv_model(
                r.params
                    .iter()
                    .map(|(k, v)| (k.clone(), models::fmt(*v)))
                    .collect(),
            ));
            ui.set_extras(kv_model(r.extra.clone()));
            ui.set_stats(SharedString::from(format!(
                "n = {}    SSE = {}    RMSE = {}    R² = {:.6}",
                r.n,
                models::fmt(r.sse),
                models::fmt(r.rmse),
                r.r2
            )));
            // R² 可信度标识：插值模型的 R² 恒为 1，不代表拟合优度 → 不显示徽章
            if model.is_interpolation() {
                ui.set_r2_badge_show(false);
                ui.set_r2_advice(SharedString::from(""));
            } else {
                let g = export::r2_grade(r.r2);
                let (_, bg) = export::r2_colors(g);
                ui.set_r2_badge_show(true);
                ui.set_r2_badge_text(SharedString::from(format!(
                    "{}  R² = {:.4}",
                    export::r2_label(g, true),
                    r.r2
                )));
                ui.set_r2_badge_bg(egui_color(bg));
                ui.set_r2_advice(SharedString::from(export::r2_advice(g, true)));
            }
            let mut note_lines = msgs.clone();
            note_lines.extend(r.notes.clone());
            if model.is_interpolation() {
                note_lines.push("该模型为插值（曲线严格通过每个数据点），R² 恒为 1，不用于评价拟合优度。".to_string());
            }
            ui.set_notes(SharedString::from(note_lines.join("  ")));

            ui.set_res_x(str_model(r.table.iter().map(|(x, _, _)| models::fmt(*x)).collect()));
            ui.set_res_y(str_model(r.table.iter().map(|(_, y, _)| models::fmt(*y)).collect()));
            ui.set_res_yh(str_model(r.table.iter().map(|(_, _, y)| models::fmt(*y)).collect()));
            ui.set_res_res(str_model(
                r.table.iter().map(|(_, y, yh)| models::fmt(y - yh)).collect(),
            ));
            ui.set_res_no(str_model(row_numbers(r.table.len())));

            update_plot(ui, &r);

            let text = result_text(ui, model, xtf, ytf, blank, &r, &msgs);
            let status = format!(
                "拟合完成：{} 个数据点，模型 {}，R² = {:.6}",
                r.n,
                model.label(),
                r.r2
            );
            {
                let mut s = st.borrow_mut();
                let blank_note = match blank {
                    Some(v) => format!("已扣除本底 {}", models::fmt(v)),
                    None => "未扣除本底".to_string(),
                };
                s.meta = Some(Meta {
                    name: ui.get_data_name().to_string(),
                    model,
                    x_label: xtf.symbol('X'),
                    y_label: ytf.symbol('Y'),
                    blank_note,
                    log_x: ui.get_log_x(),
                    msgs: msgs.clone(),
                });
                s.result = Some(r);
                s.text = text.clone();
            }
            ui.set_result_text(SharedString::from(text));
            ui.set_status(SharedString::from(status));
        }
        None => {
            {
                let mut s = st.borrow_mut();
                s.result = None;
                s.text.clear();
            }
            clear_results(ui);
            ui.set_status(SharedString::from(format!("拟合失败：{}", model.note())));
        }
    }
    set_table(&ui, xs, ys, fits, resids);
}

fn result_text(
    ui: &MainWindow,
    model: Model,
    xtf: Transform,
    ytf: Transform,
    blank: Option<f64>,
    r: &FitResult,
    msgs: &[String],
) -> String {
    let mut s = String::new();
    s.push_str("ELISACalc 回归/拟合计算结果\n");
    s.push_str(&format!("数据名称：{}\n", ui.get_data_name()));
    s.push_str(&format!("模型：{}\n", model.label()));
    s.push_str(&format!("公式：{}\n", model.formula()));
    s.push_str(&format!(
        "X 转换：{}；Y 转换：{}；扣除本底：{}\n",
        xtf.label(),
        ytf.label(),
        match blank {
            Some(v) => format!("是 ({})", models::fmt(v)),
            None => "否".to_string(),
        }
    ));
    s.push_str(&format!("拟合方程：{}\n", r.equation));
    for (k, v) in &r.params {
        s.push_str(&format!("  {} = {}\n", k, models::fmt(*v)));
    }
    for (k, v) in &r.extra {
        s.push_str(&format!("  {} = {}\n", k, v));
    }
    s.push_str(&format!(
        "统计：n = {}，SSE = {}，RMSE = {}，R² = {:.6}\n",
        r.n,
        models::fmt(r.sse),
        models::fmt(r.rmse),
        r.r2
    ));
    for m in msgs {
        s.push_str(&format!("提示：{}\n", m));
    }
    for m in &r.notes {
        s.push_str(&format!("提示：{}\n", m));
    }
    s.push_str("\nNo.\tX\tY实测\tY拟合\t残差\n");
    for (i, (x, y, yh)) in r.table.iter().enumerate() {
        s.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            i + 1,
            models::fmt(*x),
            models::fmt(*y),
            models::fmt(*yh),
            models::fmt(y - yh)
        ));
    }
    s
}

// ---------------- 反算 ----------------

// ---------------- 由 Y 计算 X / 由 X 计算 Y（批量表格）----------------

/// 反算表格的默认行数
const CALC_ROWS: usize = 20;

/// 读取当前 CalcRow 模型为 (no, input, output, note)
fn read_calc_rows(m: &ModelRc<CalcRow>) -> Vec<(String, String, String, String)> {
    (0..m.row_count())
        .map(|i| {
            let r = m.row_data(i).unwrap();
            (r.no.to_string(), r.input.to_string(), r.output.to_string(), r.note.to_string())
        })
        .collect()
}

/// 由 (input, output, note) 构造 CalcRow 模型；行数以 rows 为准，行号自动生成
fn build_calc_rows(rows: Vec<(String, String, String)>) -> ModelRc<CalcRow> {
    ModelRc::new(VecModel::from(
        rows.into_iter()
            .enumerate()
            .map(|(i, (input, output, note))| CalcRow {
                no: SharedString::from((i + 1).to_string()),
                input: SharedString::from(input),
                output: SharedString::from(output),
                note: SharedString::from(note),
            })
            .collect::<Vec<_>>(),
    ))
}

/// 由 Y 计算 X：批量
/// 返回是否至少算出了一个有效结果（用于按钮的“成功”反馈）
fn do_yx_calc(ui: &MainWindow, st: &Rc<RefCell<State>>) -> bool {
    let m = ui.get_yx_rows();
    let rows = read_calc_rows(&m);
    let s = st.borrow();
    let out: Vec<(String, String, String)> = rows
        .iter()
        .map(|(_, input, _, _)| {
            let t = input.trim();
            if t.is_empty() {
                return (input.clone(), String::new(), String::new());
            }
            let y: f64 = match t.parse() {
                Ok(v) => v,
                Err(_) => return (input.clone(), String::new(), "Y 不是有效数字".to_string()),
            };
            match &s.result {
                None => (
                    input.clone(),
                    String::new(),
                    "请先做回归/拟合".to_string(),
                ),
                Some(r) => {
                    let roots = models::solve_x(r, y);
                    if roots.is_empty() {
                        (
                            input.clone(),
                            String::new(),
                            format!(
                                "未找到解（Y 可能不在 [{}, {}] 值域内）",
                                models::fmt(r.domain.0),
                                models::fmt(r.domain.1)
                            ),
                        )
                    } else if roots.len() == 1 {
                        (input.clone(), models::fmt(roots[0]), String::new())
                    } else {
                        let list: Vec<String> = roots.iter().map(|x| models::fmt(*x)).collect();
                        (
                            input.clone(),
                            models::fmt(roots[0]),
                            format!("共 {} 个解：{}", list.len(), list.join("，")),
                        )
                    }
                }
            }
        })
        .collect();
    drop(s);
    let n = out.iter().filter(|r| !r.1.trim().is_empty()).count();
    ui.set_yx_rows(build_calc_rows(out));
    if n == 0 {
        ui.set_status(SharedString::from("未算出结果：请检查 Y 列是否已填写且已完成拟合。"));
        return false;
    }
    ui.set_status(SharedString::from(format!(
        "已批量由 Y 计算 X（{} 行）。点『复制结果』可把 X 列粘到 Excel（自动成一列）。",
        n
    )));
    true
}

/// 由 X 计算 Y：批量
/// 返回是否至少算出了一个有效结果（用于按钮的“成功”反馈）
fn do_xy_calc(ui: &MainWindow, st: &Rc<RefCell<State>>) -> bool {
    let m = ui.get_xy_rows();
    let rows = read_calc_rows(&m);
    let s = st.borrow();
    let out: Vec<(String, String, String)> = rows
        .iter()
        .map(|(_, input, _, _)| {
            let t = input.trim();
            if t.is_empty() {
                return (input.clone(), String::new(), String::new());
            }
            let x: f64 = match t.parse() {
                Ok(v) => v,
                Err(_) => return (input.clone(), String::new(), "X 不是有效数字".to_string()),
            };
            match &s.result {
                None => (
                    input.clone(),
                    String::new(),
                    "请先做回归/拟合".to_string(),
                ),
                Some(r) => match r.evaluator.eval(x) {
                    Some(v) => {
                        let note = if x < r.domain.0 || x > r.domain.1 {
                            "超出数据范围，属外推".to_string()
                        } else {
                            String::new()
                        };
                        (input.clone(), models::fmt(v), note)
                    }
                    None => (
                        input.clone(),
                        String::new(),
                        "该 X 无法求值（可能不在定义域内，如 X ≤ 0）".to_string(),
                    ),
                },
            }
        })
        .collect();
    drop(s);
    let n = out.iter().filter(|r| !r.1.trim().is_empty()).count();
    ui.set_xy_rows(build_calc_rows(out));
    if n == 0 {
        ui.set_status(SharedString::from("未算出结果：请检查 X 列是否已填写且已完成拟合。"));
        return false;
    }
    ui.set_status(SharedString::from(format!(
        "已批量由 X 计算 Y（{} 行）。点『复制结果』可把 Y 列粘到 Excel（自动成一列）。",
        n
    )));
    true
}

/// 从剪贴板粘贴一列数值到反算表格（行数自动扩展）。返回新的表格模型。
fn paste_calc_rows(
    ui: &MainWindow,
    cur: &ModelRc<CalcRow>,
    kind: &str,
) -> Option<ModelRc<CalcRow>> {
    match arboard::Clipboard::new().and_then(|mut cb| cb.get_text()) {
        Ok(t) if !t.trim().is_empty() => {
            // 支持一列（每行一个）或两列（制表符/逗号分隔，取第一列）
            let vals: Vec<String> = t
                .lines()
                .map(|l| {
                    let l = l.trim();
                    if l.contains('\t') {
                        l.split('\t').next().unwrap_or("").trim().to_string()
                    } else if l.contains(',') {
                        l.split(',').next().unwrap_or("").trim().to_string()
                    } else {
                        l.to_string()
                    }
                })
                .filter(|v| !v.is_empty())
                .collect();
            if vals.is_empty() {
                ui.set_status(SharedString::from("剪贴板中没有可用的数值。"));
                return None;
            }
            let keep = cur.row_count().max(CALC_ROWS);
            let total = vals.len().max(keep) + 3;
            let mut rows: Vec<(String, String, String)> =
                vals.into_iter().map(|v| (v, String::new(), String::new())).collect();
            while rows.len() < total {
                rows.push((String::new(), String::new(), String::new()));
            }
            let filled = rows.iter().filter(|r| !r.0.trim().is_empty()).count();
            let total = rows.len();
            ui.set_status(SharedString::from(format!(
                "已从剪贴板导入 {} 个{}值（表格共 {} 行，行数已自动扩展），点『批量计算』。",
                filled,
                kind,
                total
            )));
            return Some(build_calc_rows(rows));
        }
        Ok(_) => ui.set_status(SharedString::from("剪贴板内容为空。")),
        Err(e) => ui.set_status(SharedString::from(format!("读取剪贴板失败：{}", e))),
    }
    None
}

/// 复制结果列：每个值一行 → 粘贴到 Excel 自动成一列
/// 返回是否真的复制了内容
fn copy_calc_results(ui: &MainWindow, m: &ModelRc<CalcRow>, label: &str) -> bool {
    let rows = read_calc_rows(m);
    // 只复制“有结果”的行：每个值一行 —— 粘到 Excel 会自动排成一列
    let vals: Vec<&str> = rows
        .iter()
        .filter(|r| !r.1.trim().is_empty())
        .map(|r| r.1.as_str())
        .collect();
    if vals.is_empty() {
        ui.set_status(SharedString::from(format!("{} 列暂无结果，请先批量计算。", label)));
        return false;
    }
    let text = vals.join("\n");
    match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(text.clone())) {
        Ok(_) => {
            ui.set_status(SharedString::from(format!(
                "已复制 {} 个{}值到剪贴板（每行一个，粘贴到 Excel 会自动排成一列）。",
                vals.len(),
                label
            )));
            true
        }
        Err(e) => {
            ui.set_status(SharedString::from(format!("写入剪贴板失败：{}", e)));
            false
        }
    }
}

// ---------------- 绘图 ----------------

fn update_plot(ui: &MainWindow, r: &FitResult) {
    let want_log = ui.get_log_x();
    let use_log = want_log && r.table.iter().all(|(x, _, _)| *x > 0.0);
    let fx = |x: f64| if use_log { x.log10() } else { x };
    let ok = |x: &f64, y: &f64| x.is_finite() && y.is_finite() && (!use_log || *x > 0.0);

    let mut vx: Vec<f64> = Vec::new();
    let mut vy: Vec<f64> = Vec::new();
    for (x, y, _) in &r.table {
        if ok(x, y) {
            vx.push(fx(*x));
            vy.push(*y);
        }
    }
    for (x, y) in &r.curve {
        if ok(x, y) {
            vx.push(fx(*x));
            vy.push(*y);
        }
    }
    if vx.len() < 2 {
        ui.set_has_plot(false);
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
    let xr = xmax - xmin;
    let yr = ymax - ymin;
    xmin -= 0.05 * xr;
    xmax += 0.05 * xr;
    ymin -= 0.05 * yr;
    ymax += 0.05 * yr;

    let (l, rr, t, b) = (58.0, PLOT_W - 14.0, 12.0, PLOT_H - 18.0);
    let sx = |v: f64| l + (v - xmin) / (xmax - xmin) * (rr - l);
    let sy = |v: f64| b - (v - ymin) / (ymax - ymin) * (b - t);

    // 刻度：X 轴 50 一格，Y 轴 0.5 一格（格数过多时自动放大为 10 倍步长）
    let xtick = models::axis_ticks(xmin, xmax, X_STEP, 40);
    let ytick = models::axis_ticks(ymin, ymax, Y_STEP, 40);

    // 网格 + 边框
    let mut grid = String::new();
    for v in &xtick {
        grid.push_str(&format!("M {:.1} {:.1} L {:.1} {:.1} ", sx(*v), t, sx(*v), b));
    }
    for v in &ytick {
        grid.push_str(&format!("M {:.1} {:.1} L {:.1} {:.1} ", l, sy(*v), rr, sy(*v)));
    }
    grid.push_str(&format!(
        "M {:.1} {:.1} L {:.1} {:.1} L {:.1} {:.1} L {:.1} {:.1} Z",
        l, t, rr, t, rr, b, l, b
    ));

    // 拟合曲线
    let mut curve = String::new();
    let mut first = true;
    for (x, y) in &r.curve {
        if !ok(x, y) {
            continue;
        }
        let py = sy(*y).clamp(t, b);
        curve.push_str(&format!(
            "{} {:.1} {:.1} ",
            if first { "M" } else { "L" },
            sx(fx(*x)),
            py
        ));
        first = false;
    }

    // 实测点
    let mut dots: Vec<Dot> = Vec::new();
    for (x, y, _) in &r.table {
        if !ok(x, y) {
            continue;
        }
        dots.push(Dot { px: sx(fx(*x)) as f32, py: sy(*y) as f32 });
    }

    // 刻度标签
    let mut xt: Vec<Tick> = Vec::new();
    let mut yt: Vec<Tick> = Vec::new();
    for v in &xtick {
        let label = if use_log {
            models::fmt(10f64.powf(*v))
        } else {
            models::fmt(*v)
        };
        xt.push(Tick { pos: sx(*v) as f32, label: SharedString::from(label) });
    }
    for v in &ytick {
        yt.push(Tick { pos: sy(*v) as f32, label: SharedString::from(models::fmt(*v)) });
    }

    ui.set_grid_path(SharedString::from(grid));
    ui.set_curve_path(SharedString::from(curve));
    ui.set_dots(ModelRc::new(VecModel::from(dots)));
    ui.set_x_ticks(ModelRc::new(VecModel::from(xt)));
    ui.set_y_ticks(ModelRc::new(VecModel::from(yt)));
    ui.set_has_plot(true);
}

// ---------------- 导出图片 ----------------

/// 返回是否导出成功
fn do_export(ui: &MainWindow, st: &Rc<RefCell<State>>) -> bool {
    let fmt = if ui.get_export_fmt_index() > 0 {
        export::ImgFormat::Png
    } else {
        export::ImgFormat::Jpeg
    };
    // 分辨率：由目标宽度换算成倍率（版面基准宽度 860）
    let ri = ui.get_export_res_index().max(0) as usize;
    let (target_w, res_name) = if ri < export::RES_PRESETS.len() {
        export::RES_PRESETS[ri]
    } else {
        export::RES_PRESETS[2]
    };
    let scale = target_w as f64 / 860.0;

    // 曲线采样密度随分辨率提升：界面绘图只用 260 点，导出 4K 时曲线横跨
    // 数千物理像素，260 点会出现明显折线感。这里按「绘图区物理宽度的 2 倍」
    // 重新采样（上限 20000 点），保证放大后依旧平滑。
    let plot_px = (700.0 * scale).round() as usize;
    let dense = (plot_px * 2).clamp(260, 20000);

    let spec = {
        let s = st.borrow();
        let (r, meta) = match (&s.result, &s.meta) {
            (Some(r), Some(m)) => (r, m),
            _ => {
                drop(s);
                ui.set_status(SharedString::from("请先点击【回归/拟合】得到曲线，再导出图片。"));
                return false;
            }
        };
        let mut notes: Vec<String> = Vec::new();
        notes.push(meta.blank_note.clone());
        notes.push(format!("数据转换：X → {}，Y → {}", meta.x_label, meta.y_label));
        notes.extend(meta.msgs.iter().cloned());
        notes.extend(r.notes.iter().cloned());

        export::ExportSpec {
            name: if meta.name.trim().is_empty() {
                "标准曲线".to_string()
            } else {
                meta.name.clone()
            },
            model: meta.model,
            equation: r.equation.clone(),
            params: r
                .params
                .iter()
                .map(|(k, v)| (k.clone(), models::fmt(*v)))
                .collect(),
            extras: r.extra.clone(),
            stats: format!(
                "n = {}    SSE = {}    RMSE = {}    R² = {:.6}",
                r.n,
                models::fmt(r.sse),
                models::fmt(r.rmse),
                r.r2
            ),
            x_label: meta.x_label.clone(),
            y_label: meta.y_label.clone(),
            notes,
            pts: r.table.iter().map(|(x, y, _)| (*x, *y)).collect(),
            curve: models::dense_curve(
                &r.evaluator,
                &r.table.iter().map(|(x, y, _)| (*x, *y)).collect::<Vec<_>>(),
                meta.model,
                dense,
            ),
            log_x: meta.log_x,
            scale,
            // 插值模型的 R² 恒为 1，不能代表拟合优度 → 不画徽章
            r2: if meta.model.is_interpolation() { None } else { Some(r.r2) },
        }
    };

    ui.set_status(SharedString::from(format!(
        "正在生成 {} 图片（{}），分辨率较高时请稍候…",
        fmt.label(),
        res_name
    )));

    match export::export_plot(&spec, fmt) {
        Ok(path) => {
            // 文件名单独提出来：界面上“已保存到程序所在目录”下方着重显示
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string());
            ui.set_export_saved_name(SharedString::from(name));
            ui.set_export_saved_show(true);
            ui.set_status(SharedString::from(format!(
                "已导出 {} 图片（{}）：{}",
                fmt.label(),
                res_name,
                path.display()
            )));
            true
        }
        Err(e) => {
            // 失败时清掉上一次的成功提示，避免误导
            ui.set_export_saved_show(false);
            ui.set_export_saved_name(SharedString::new());
            ui.set_status(SharedString::from(format!("导出失败：{}", e)));
            false
        }
    }
}

// ---------------- 帮助文本 ----------------

fn help_text() -> String {
    let mut s = String::new();
    s.push_str("【回归/拟合模型】\n");
    for m in Model::ALL {
        s.push_str(&format!("· {}\n    {}\n    {}\n", m.label(), m.formula(), m.note()));
    }
    s.push_str("\n【数据转换】（X 列与 Y 列各自独立设置）\n");
    for t in Transform::ALL {
        s.push_str(&format!("· {}：X → {}，Y → {}\n", t.label(), t.symbol('X'), t.symbol('Y')));
    }
    s.push_str("\n【操作步骤】\n");
    s.push_str("1. 点【数据输入(E)】，在表格中录入 X（浓度/剂量）与 Y（反应值），或用【粘贴 / 导入数据】从 Excel 整块粘贴；\n");
    s.push_str("2. 选择模型与 X/Y 转换方式，必要时勾选【扣除本底】并填入空白孔读数；\n");
    s.push_str("3. 点【回归/拟合】，下方显示拟合曲线；点【回归方程(R)】查看参数、R² 与残差表；\n");
    s.push_str("4. 点【由Y计算X】反算浓度（非单调模型会列出全部解），点【由X计算Y】预测反应值；\n");
    s.push_str("5. 点【复制】把完整结果（含残差表）放进剪贴板，也可在【回归方程(R)】页手动 Ctrl+C。\n");
    s.push_str("\n说明：处理顺序为 扣除本底 → X/Y 列转换 → 拟合；转换非法的行（如负数取对数、0 取倒数）会被自动剔除并在结果区提示。\n");
    s
}

// ---------------- 入口 ----------------

fn main() {
    let ui = MainWindow::new().unwrap();

    ui.set_model_items(str_model(
        Model::ALL.iter().map(|m| m.label().to_string()).collect(),
    ));
    ui.set_model_formulas(str_model(
        Model::ALL.iter().map(|m| m.formula().to_string()).collect(),
    ));
    ui.set_tf_items(str_model(
        Transform::ALL.iter().map(|t| t.label().to_string()).collect(),
    ));
    ui.set_formula_text(SharedString::from(Model::ALL[0].formula()));
    ui.set_export_formats(str_model(vec![
        "JPEG (.jpg)".to_string(),
        "PNG (.png)".to_string(),
    ]));
    ui.set_export_fmt_index(0);
    ui.set_help_text(SharedString::from(help_text()));
    reset_table(&ui, DEFAULT_ROWS);

    let st = Rc::new(RefCell::new(State::default()));

    // 回归/拟合（带加载反馈）
    let w = ui.as_weak();
    let s = st.clone();
    ui.on_fit(move || {
        let wa = w.clone();
        let wb = w.clone();
        let s = s.clone();
        with_feedback(wa, Btn::Fit, move || {
            let Some(ui) = wb.upgrade() else {
                return false;
            };
            do_fit(&ui, &s);
            s.borrow().result.is_some()
        });
    });

    // 清除
    let w = ui.as_weak();
    let s = st.clone();
    ui.on_clear_all(move || {
        let ui = w.unwrap();
        reset_table(&ui, DEFAULT_ROWS);
        clear_results(&ui);
        let mut st = s.borrow_mut();
        st.result = None;
        st.text.clear();
        drop(st);
        ui.set_status(SharedString::from("已清除数据与结果。"));
    });

    // 复制
    let w = ui.as_weak();
    let s = st.clone();
    ui.on_copy_result(move || {
        let wa = w.clone();
        let wb = w.clone();
        let s = s.clone();
        with_feedback(wa, Btn::Copy, move || {
            let Some(ui) = wb.upgrade() else {
                return false;
            };
            let txt = {
                let st = s.borrow();
                if st.text.is_empty() {
                    ui.get_result_text().to_string()
                } else {
                    st.text.clone()
                }
            };
            if txt.is_empty() {
                ui.set_status(SharedString::from("尚无结果可复制，请先拟合。"));
                return false;
            }
            let ok = match arboard::Clipboard::new() {
                Ok(mut cb) => cb.set_text(txt).is_ok(),
                Err(_) => false,
            };
            ui.set_status(SharedString::from(if ok {
                "结果已复制到剪贴板。".to_string()
            } else {
                "复制失败：请到【回归方程(R)】页选中文本手动 Ctrl+C。".to_string()
            }));
            ok
        });
    });

    // 退出
    ui.on_quit(|| {
        let _ = slint::quit_event_loop();
    });

    // 增加行
    let w = ui.as_weak();
    ui.on_add_rows(move || {
        let ui = w.unwrap();
        let mut xs = col(&ui.get_xs());
        let mut ys = col(&ui.get_ys());
        for _ in 0..5 {
            xs.push(String::new());
            ys.push(String::new());
        }
        let empty = vec![String::new(); xs.len()];
        set_table(&ui, xs, ys, empty.clone(), empty);
        ui.set_status(SharedString::from("已增加 5 行。"));
    });

    // 删除行
    let w = ui.as_weak();
    let s = st.clone();
    ui.on_remove_row(move |i: i32| {
        let ui = w.unwrap();
        let i = i as usize;
        let mut xs = col(&ui.get_xs());
        let mut ys = col(&ui.get_ys());
        if xs.len() <= 1 || i >= xs.len() {
            return;
        }
        xs.remove(i);
        ys.remove(i);
        let empty = vec![String::new(); xs.len()];
        set_table(&ui, xs, ys, empty.clone(), empty);
        clear_results(&ui);
        let mut st = s.borrow_mut();
        st.result = None;
        st.text.clear();
        drop(st);
        ui.set_status(SharedString::from("已删除该行，请重新拟合。"));
    });

    // 粘贴导入
    let w = ui.as_weak();
    let s = st.clone();
    ui.on_parse_paste(move || {
        let ui = w.unwrap();
        let text = ui.get_paste_text().to_string();
        let mut rows: Vec<(String, String)> = Vec::new();
        for line in text.lines() {
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            let parts: Vec<&str> = if t.contains('\t') {
                t.split('\t').collect()
            } else if t.contains(',') {
                t.split(',').collect()
            } else if t.contains(';') {
                t.split(';').collect()
            } else {
                t.split_whitespace().collect()
            };
            let x = parts.first().map(|v| v.trim().to_string()).unwrap_or_default();
            let y = parts.get(1).map(|v| v.trim().to_string()).unwrap_or_default();
            if x.is_empty() && y.is_empty() {
                continue;
            }
            rows.push((x, y));
        }
        if rows.is_empty() {
            ui.set_status(SharedString::from("未能解析出两列数据，请检查粘贴内容。"));
            return;
        }
        fill_table(&ui, rows);
        let mut st = s.borrow_mut();
        st.result = None;
        st.text.clear();
    });

    // 从剪贴板读取文本填入导入框（不依赖输入框的 Ctrl+V 快捷键实现）
    let w = ui.as_weak();
    ui.on_read_clipboard(move || {
        let ui = w.unwrap();
        match arboard::Clipboard::new().and_then(|mut cb| cb.get_text()) {
            Ok(t) if !t.trim().is_empty() => {
                let lines = t.lines().count();
                ui.set_paste_text(SharedString::from(t));
                ui.set_status(SharedString::from(format!(
                    "已从剪贴板读取 {} 行，确认无误后点『解析填充』。",
                    lines
                )));
            }
            Ok(_) => {
                ui.set_status(SharedString::from("剪贴板内容为空。"));
            }
            Err(e) => {
                ui.set_status(SharedString::from(format!("读取剪贴板失败：{}", e)));
            }
        }
    });

    // 导出曲线图片
    let w = ui.as_weak();
    let s = st.clone();
    ui.on_export_plot(move || {
        let wa = w.clone();
        let wb = w.clone();
        let s = s.clone();
        with_feedback(wa, Btn::Export, move || {
            let Some(ui) = wb.upgrade() else {
                return false;
            };
            do_export(&ui, &s)
        });
    });

    // 导出分辨率选项（与 export::RES_PRESETS 保持同一来源）
    ui.set_export_res(str_model(
        export::RES_PRESETS.iter().map(|(_, n)| (*n).to_string()).collect(),
    ));

    // ---------------- 由 Y 计算 X（批量表格）----------------
    let w = ui.as_weak();
    let s2 = st.clone();
    ui.on_yx_calc(move || {
        let wa = w.clone();
        let wb = w.clone();
        let s = s2.clone();
        with_feedback(wa, Btn::Yx, move || {
            let Some(ui) = wb.upgrade() else {
                return false;
            };
            do_yx_calc(&ui, &s)
        });
    });

    let w = ui.as_weak();
    ui.on_yx_paste(move || {
        let ui = w.unwrap();
        let cur = ui.get_yx_rows();
        if let Some(m) = paste_calc_rows(&ui, &cur, "Y") {
            ui.set_yx_rows(m);
        }
    });

    let w = ui.as_weak();
    ui.on_yx_copy(move || {
        let ui = w.unwrap();
        let m = ui.get_yx_rows();
        copy_calc_results(&ui, &m, "X");
    });

    let w = ui.as_weak();
    ui.on_yx_add_row(move || {
        let ui = w.unwrap();
        let mut rows = read_calc_rows(&ui.get_yx_rows());
        for _ in 0..5 {
            rows.push((String::new(), String::new(), String::new(), String::new()));
        }
        ui.set_yx_rows(build_calc_rows(
            rows.into_iter().map(|(_, i, o, n)| (i, o, n)).collect(),
        ));
    });

    let w = ui.as_weak();
    ui.on_yx_clear(move || {
        let ui = w.unwrap();
        ui.set_yx_rows(build_calc_rows(
            (0..CALC_ROWS).map(|_| (String::new(), String::new(), String::new())).collect(),
        ));
        ui.set_status(SharedString::from("已清空『由 Y 计算 X』表格。"));
    });

    let w = ui.as_weak();
    ui.on_yx_set_input(move |i, v| {
        let ui = w.unwrap();
        let m = ui.get_yx_rows();
        let idx = i as usize;
        if idx < m.row_count() {
            if let Some(mut r) = m.row_data(idx) {
                r.input = SharedString::from(v.to_string());
                m.set_row_data(idx, r);
            }
        }
    });

    // ---------------- 由 X 计算 Y（批量表格）----------------
    let w = ui.as_weak();
    let s3 = st.clone();
    ui.on_xy_calc(move || {
        let wa = w.clone();
        let wb = w.clone();
        let s = s3.clone();
        with_feedback(wa, Btn::Xy, move || {
            let Some(ui) = wb.upgrade() else {
                return false;
            };
            do_xy_calc(&ui, &s)
        });
    });

    let w = ui.as_weak();
    ui.on_xy_paste(move || {
        let ui = w.unwrap();
        let cur = ui.get_xy_rows();
        if let Some(m) = paste_calc_rows(&ui, &cur, "X") {
            ui.set_xy_rows(m);
        }
    });

    let w = ui.as_weak();
    ui.on_xy_copy(move || {
        let ui = w.unwrap();
        let m = ui.get_xy_rows();
        copy_calc_results(&ui, &m, "Y");
    });

    let w = ui.as_weak();
    ui.on_xy_add_row(move || {
        let ui = w.unwrap();
        let mut rows = read_calc_rows(&ui.get_xy_rows());
        for _ in 0..5 {
            rows.push((String::new(), String::new(), String::new(), String::new()));
        }
        ui.set_xy_rows(build_calc_rows(
            rows.into_iter().map(|(_, i, o, n)| (i, o, n)).collect(),
        ));
    });

    let w = ui.as_weak();
    ui.on_xy_clear(move || {
        let ui = w.unwrap();
        ui.set_xy_rows(build_calc_rows(
            (0..CALC_ROWS).map(|_| (String::new(), String::new(), String::new())).collect(),
        ));
        ui.set_status(SharedString::from("已清空『由 X 计算 Y』表格。"));
    });

    let w = ui.as_weak();
    ui.on_xy_set_input(move |i, v| {
        let ui = w.unwrap();
        let m = ui.get_xy_rows();
        let idx = i as usize;
        if idx < m.row_count() {
            if let Some(mut r) = m.row_data(idx) {
                r.input = SharedString::from(v.to_string());
                m.set_row_data(idx, r);
            }
        }
    });

    // 反算表格初始化（默认 20 行空表）—— 必须在 show 之前，否则首帧为空
    ui.set_yx_rows(build_calc_rows(
        (0..CALC_ROWS).map(|_| (String::new(), String::new(), String::new())).collect(),
    ));
    ui.set_xy_rows(build_calc_rows(
        (0..CALC_ROWS).map(|_| (String::new(), String::new(), String::new())).collect(),
    ));

    // ---------------- 自绘窗口控制（no-frame 下替代原生标题栏）----------------
    //
    // 框架是 Slint（底层 winit）。Slint 1.18.1 的 `Window` 已封装好这些能力，
    // 直接在 Rust 侧调用官方 API 即可，**不要**再用 Win32 消息：
    //   · set_minimized(true)            最小化
    //   · set_maximized(!is_maximized()) 最大化 / 还原切换
    //   · quit_event_loop() + hide()     关闭
    // （上一版改用 WM_SYSCOMMAND 后三个按钮全部失效：sys_command() 只按“是否
    //   找到句柄”返回，消息实际未生效却提前 return，把可用的 Slint 路径跳过了。）

    // 最小化
    let w = ui.as_weak();
    ui.on_win_minimize(move || {
        let ui = w.unwrap();
        ui.window().set_minimized(true);
    });

    // 最大化 / 还原切换
    let w = ui.as_weak();
    ui.on_win_maximize(move || {
        let ui = w.unwrap();
        let win = ui.window();
        win.set_maximized(!win.is_maximized());
    });

    // 关闭：退出事件循环 → 隐藏窗口 → 兜底结束进程
    let w = ui.as_weak();
    ui.on_win_close(move || {
        let ui = w.unwrap();
        let _ = slint::quit_event_loop();
        let _ = ui.hide();
        // 兜底：个别后端下 quit_event_loop 可能不生效，这里确保进程退出
        std::process::exit(0);
    });

    // ---------------- 拖动标题栏（关键：用“屏幕位移”，不是“窗口内位移”）----------------
    //
    // 为什么之前会闪烁：Slint 的 TouchArea 只能给出**鼠标在窗口内的坐标** M。
    // 若用 ΔM = M - M0 去移动窗口，则 M 本身又依赖窗口位置 W（M = S - W），
    // 于是 ΔM = (W0 + D - W) - M0 会随 W 变化 → W 与 ΔM 互相拉扯 → 抖动。
    //
    // 正确做法：鼠标的**真实屏幕位置** S = W + M×scale 是恒定的（窗口移动不会
    // 改变鼠标在屏幕上的位置）。所以：
    //   按下时记录 W0 与 S0 = W0 + M0×scale
    //   移动时算  S  = W  + M ×scale，取 ΔS = S - S0
    //   新窗口位置 W  = W0 + ΔS
    // ΔS 与 W 无关，因此不存在反馈循环 —— 平滑、不闪烁，且跨平台（无需 Win32）。
    //
    // 注：Slint 1.18.1 的 Window 没有 drag_window()，所以必须这样算。

    let drag: Rc<RefCell<Option<(f64, f64, f64, f64)>>> = Rc::new(RefCell::new(None));

    let w = ui.as_weak();
    let d1 = drag.clone();
    ui.on_win_drag_start(move |sx, sy| {
        let ui = w.unwrap();
        let win = ui.window();
        let scale = win.scale_factor().max(0.01) as f64;
        let p = win.position();
        // 鼠标屏幕位置（物理像素）= 窗口位置 + 窗口内坐标×缩放
        let s0x = p.x as f64 + sx as f64 * scale;
        let s0y = p.y as f64 + sy as f64 * scale;
        *d1.borrow_mut() = Some((p.x as f64, p.y as f64, s0x, s0y));
    });

    let w = ui.as_weak();
    let d2 = drag.clone();
    ui.on_win_drag_move(move |cx, cy| {
        let Some((w0x, w0y, s0x, s0y)) = *d2.borrow() else {
            return; // 未处于拖动状态（例如只是鼠标划过）
        };
        let ui = w.unwrap();
        let win = ui.window();
        let scale = win.scale_factor().max(0.01) as f64;
        let p = win.position();
        let sx = p.x as f64 + cx as f64 * scale;
        let sy = p.y as f64 + cy as f64 * scale;
        // 鼠标位移只由屏幕坐标决定 → 不受窗口位置影响
        let dx = sx - s0x;
        let dy = sy - s0y;
        win.set_position(slint::PhysicalPosition::new(
            (w0x + dx) as i32,
            (w0y + dy) as i32,
        ));
    });

    let d3 = drag.clone();
    ui.on_win_drag_end(move || {
        *d3.borrow_mut() = None;
    });

    // ---- 系统主题同步 ----
    // 启动时立即应用一次；之后定时轮询，系统切换明暗时自动跟随。
    // （不依赖 Palette 的 ColorScheme.unknown 回退，该行为在部分风格下不可靠。）
    let mut last_dark: Option<bool> = None;
    if let Some(d) = theme::prefers_dark() {
        ui.invoke_apply_system_theme(d);
        last_dark = Some(d);
    }
    let w_theme = ui.as_weak();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(1500),
        move || {
            let ui = match w_theme.upgrade() {
                Some(u) => u,
                None => return,
            };
            if let Some(d) = theme::prefers_dark() {
                if last_dark.replace(d) != Some(d) {
                    ui.invoke_apply_system_theme(d);
                }
            }
        },
    );
    // 定时器必须在 ui.run() 期间保持存活
    let _keep_timer = timer;

    ui.run().unwrap();
}
