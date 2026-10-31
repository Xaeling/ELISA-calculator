//! X / Y 数据列的数学转换（对应界面上的“不转换 / 对数[2为底] / 对数[10为底] / 平方 / 平方根 / 指数 / 倒数”）

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Transform {
    /// 不转换
    None,
    /// log2(x)
    Log2,
    /// log10(x)
    Log10,
    /// x^2
    Square,
    /// sqrt(x)
    Sqrt,
    /// e^x
    Exp,
    /// 1/x
    Reciprocal,
}

impl Transform {
    pub const ALL: [Transform; 7] = [
        Transform::None,
        Transform::Log2,
        Transform::Log10,
        Transform::Square,
        Transform::Sqrt,
        Transform::Exp,
        Transform::Reciprocal,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Transform::None => "不转换",
            Transform::Log2 => "对数[2为底]",
            Transform::Log10 => "对数[10为底]",
            Transform::Square => "平方",
            Transform::Sqrt => "平方根",
            Transform::Exp => "指数",
            Transform::Reciprocal => "倒数",
        }
    }

    /// 用于生成结果说明的符号式
    pub fn symbol(&self, var: char) -> String {
        let v = var.to_string();
        match self {
            Transform::None => v,
            Transform::Log2 => format!("log2({})", v),
            Transform::Log10 => format!("log10({})", v),
            Transform::Square => format!("{}^2", v),
            Transform::Sqrt => format!("sqrt({})", v),
            Transform::Exp => format!("exp({})", v),
            Transform::Reciprocal => format!("1/{}", v),
        }
    }

    /// 转换单个数值。非法输入（如对负数取对数、对 0 取倒数、结果溢出）返回 None，调用方据此剔除该行。
    pub fn apply(&self, v: f64) -> Option<f64> {
        let out = match self {
            Transform::None => v,
            Transform::Log2 => {
                if v <= 0.0 || !v.is_finite() {
                    return None;
                }
                v.log2()
            }
            Transform::Log10 => {
                if v <= 0.0 || !v.is_finite() {
                    return None;
                }
                v.log10()
            }
            Transform::Square => v * v,
            Transform::Sqrt => {
                if v < 0.0 || !v.is_finite() {
                    return None;
                }
                v.sqrt()
            }
            Transform::Exp => v.exp(),
            Transform::Reciprocal => {
                if v == 0.0 || !v.is_finite() {
                    return None;
                }
                1.0 / v
            }
        };
        if out.is_finite() {
            Some(out)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_only_for_positive() {
        assert_eq!(Transform::Log10.apply(100.0), Some(2.0));
        assert_eq!(Transform::Log10.apply(0.0), None);
        assert_eq!(Transform::Log10.apply(-3.0), None);
        assert_eq!(Transform::Log2.apply(8.0), Some(3.0));
    }

    #[test]
    fn others() {
        assert_eq!(Transform::Square.apply(3.0), Some(9.0));
        assert_eq!(Transform::Sqrt.apply(9.0), Some(3.0));
        assert_eq!(Transform::Sqrt.apply(-1.0), None);
        assert_eq!(Transform::Exp.apply(0.0), Some(1.0));
        assert_eq!(Transform::Reciprocal.apply(4.0), Some(0.25));
        assert_eq!(Transform::Reciprocal.apply(0.0), None);
    }
}
