//! Lightweight feature extraction for the heuristic strategy.

/// Flags / signals extracted from a user turn.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TurnFeatures {
    pub char_len: usize,
    pub approx_tokens: u64,
    pub has_code_fence: bool,
    pub has_path_or_file: bool,
    pub chinese_ratio: f32,
    pub is_trivial: bool,
    pub is_complaint: bool,
    pub flag_debug: bool,
    pub flag_architecture: bool,
    pub flag_high_risk: bool,
    pub flag_planning: bool,
    pub flag_long_form: bool,
}

const COMPLAINT_EN: &[&str] = &[
    "wrong",
    "incorrect",
    "that's not",
    "that is not",
    "does not work",
    "doesn't work",
    "not what i asked",
    "still broken",
    "try again",
    "you failed",
    "useless",
    "hallucinat",
];

const COMPLAINT_ZH: &[&str] = &[
    "不对",
    "错了",
    "不行",
    "没用",
    "不是我说的",
    "重新",
    "再试",
    "失败",
    "幻觉",
];

const DEBUG_KW: &[&str] = &[
    "debug",
    "stacktrace",
    "stack trace",
    "backtrace",
    "segfault",
    "panic",
    "exception",
    "traceback",
    "报错",
    "崩溃",
    "调试",
];

const ARCH_KW: &[&str] = &[
    "architecture",
    "refactor",
    "migrate",
    "design system",
    "microservice",
    "分布式",
    "架构",
    "重构",
    "迁移",
];

const RISK_KW: &[&str] = &[
    "security",
    "vulnerability",
    "cve-",
    "auth",
    "production outage",
    "数据泄露",
    "安全",
    "权限",
];

const PLAN_KW: &[&str] = &[
    "plan",
    "roadmap",
    "step by step",
    "implement",
    "design a",
    "方案",
    "计划",
    "实现",
    "逐步",
];

const TRIVIAL: &[&str] = &[
    "ok",
    "okay",
    "thanks",
    "thank you",
    "thx",
    "got it",
    "lgtm",
    "好的",
    "谢谢",
    "嗯",
    "行",
];

pub fn extract_features(text: &str) -> TurnFeatures {
    let trimmed = text.trim();
    let lower = trimmed.to_ascii_lowercase();
    let char_len = trimmed.chars().count();
    let approx_tokens = ((char_len as f32) / 3.5).ceil() as u64;

    let chinese = trimmed
        .chars()
        .filter(|c| ('\u{4e00}'..='\u{9fff}').contains(c))
        .count();
    let chinese_ratio = if char_len == 0 {
        0.0
    } else {
        chinese as f32 / char_len as f32
    };

    let has_code_fence = trimmed.contains("```")
        || trimmed.contains("fn ")
        || trimmed.contains("def ")
        || trimmed.contains("class ");
    let has_path_or_file = trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains(".rs")
        || trimmed.contains(".py")
        || trimmed.contains(".ts")
        || trimmed.contains(".toml");

    let is_trivial = char_len <= 24
        && TRIVIAL
            .iter()
            .any(|t| lower == *t || lower.trim_end_matches(['!', '.', '。']) == *t);

    let is_complaint = COMPLAINT_EN.iter().any(|k| lower.contains(k))
        || COMPLAINT_ZH.iter().any(|k| trimmed.contains(k));

    TurnFeatures {
        char_len,
        approx_tokens,
        has_code_fence,
        has_path_or_file,
        chinese_ratio,
        is_trivial,
        is_complaint,
        flag_debug: DEBUG_KW.iter().any(|k| lower.contains(k) || trimmed.contains(k)),
        flag_architecture: ARCH_KW.iter().any(|k| lower.contains(k) || trimmed.contains(k)),
        flag_high_risk: RISK_KW.iter().any(|k| lower.contains(k) || trimmed.contains(k)),
        flag_planning: PLAN_KW.iter().any(|k| lower.contains(k) || trimmed.contains(k)),
        flag_long_form: char_len >= 1200 || approx_tokens >= 400,
    }
}
