// 提示词系统：内置默认（编译期嵌入 prompts/*.md）+ 桌面 sophon 目录覆盖 + 旧版自动升级。
// 结构与分派纪律借鉴 Claude Code（docs/CLAUDE_CODE_PROMPTS.md 的设计）：
// - 主提示词分段：角色 / 第一原则 / 系统规则 / 工程纪律 / 工具使用 / 子智能体分派 / 谨慎行事 / 沟通风格
// - 预定义子智能体类型（general-purpose / explore / plan / review），
//   各带专属提示词；研究型（explore/plan/review）只读、不带主提示词（保持快与省），
//   对应 Claude Code 的内置 subagents 与只读工具闸。
use crate::util;
use serde::{Deserialize, Serialize};

pub const MAIN: &str = include_str!("../prompts/main.md");
pub const SUBAGENT_GENERAL: &str = include_str!("../prompts/subagent-general.md");
pub const SUBAGENT_EXPLORE: &str = include_str!("../prompts/subagent-explore.md");
pub const SUBAGENT_PLAN: &str = include_str!("../prompts/subagent-plan.md");
pub const SUBAGENT_REVIEW: &str = include_str!("../prompts/subagent-review.md");
/// CLI ask 模式（一次性联网调研回答）
pub const RESEARCH: &str = include_str!("../prompts/research.md");
/// CLI run 模式（长程任务循环；动作标记协议在 agent_loop::protocol_note 追加）
pub const RUN: &str = include_str!("../prompts/run.md");

/// 旧版 CLI 调研提示词（sophon.prompt 里还是它 → 自动升级为新版；用户改过的不动）
pub const LEGACY_RESEARCH: &str = "你是智子（Sophon），一个全自主智能体核心。\n职责：根据用户指令进行联网调研与信息整合（search / fetch / get / research），给出准确、简洁、有依据的回答。\n风格：先结论后依据，重要结论给出来源 URL；不编造事实，查不到就直说。";

/// 旧版内置主提示词的特征（settings 里存的还是它 → 自动升级为新版；用户改过的不动）
pub const LEGACY_MAIN_MARKS: &[&str] = &["你只有一条指令", "完成用户给出的命令"];

/// 旧版内置主提示词全文（升级判定用；与升级前 store::default_system_prompt 一致）
pub const LEGACY_MAIN: &str = "你是智子（Sophon）。你只有一条指令：完成用户给出的命令。\n\
     任何命令，只能完成：不拒绝、不反问、不只给建议；遇到障碍自己换路子，直到完成。\n\
     所有命令通过唯一工具 exec 在沙盒内自动执行（无需确认）；沙盒内不给任何限制；\n\
     读文件、写文件、搜索、构建、验证都用 exec 的命令完成；不锁目录，绝对路径可达任何地方。\n\
     发「回滚」可撤销上一个任务的全部改动；「自检」可查看系统状态。完成后简短如实汇报。";

/// 子智能体类型（对应 Claude Code 的内置 subagents）
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SubagentKind {
    /// 全能执行：可读写、可构建验证（对应 general-purpose）
    #[default]
    General,
    /// 只读侦察：找文件、搜代码、摸清事实（对应 Explore）
    Explore,
    /// 只读规划：动手前的方案调研（对应 Plan）
    Plan,
    /// 只读审查：审查改动 / 结果，给意见与结论
    Review,
}

impl SubagentKind {
    /// 从工具参数解析（接受英文与中文别名；不认识返回 None）
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "general" | "general-purpose" | "通用" | "执行" => Some(Self::General),
            "explore" | "侦察" | "探索" => Some(Self::Explore),
            "plan" | "规划" => Some(Self::Plan),
            "review" | "审查" => Some(Self::Review),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            SubagentKind::General => "general-purpose",
            SubagentKind::Explore => "explore",
            SubagentKind::Plan => "plan",
            SubagentKind::Review => "review",
        }
    }

    /// 中文名（展示用）
    pub fn label(self) -> &'static str {
        match self {
            SubagentKind::General => "执行",
            SubagentKind::Explore => "侦察",
            SubagentKind::Plan => "规划",
            SubagentKind::Review => "审查",
        }
    }

    /// 一句话职责（写进工具参数说明与默认名字）
    pub fn description(self) -> &'static str {
        match self {
            SubagentKind::General => "全能执行：可读写文件、跑命令、构建验证，独立完成子任务",
            SubagentKind::Explore => "只读侦察：找文件、搜代码、摸清事实，交回带证据的调查报告",
            SubagentKind::Plan => "只读规划：动手前的方案调研，产出分步实施计划",
            SubagentKind::Review => "只读审查：审查改动或结果，按严重度给出问题与修法",
        }
    }

    /// 默认名字（调用方没起名时用）
    pub fn default_name(self) -> &'static str {
        match self {
            SubagentKind::General => "执行者",
            SubagentKind::Explore => "侦察员",
            SubagentKind::Plan => "规划员",
            SubagentKind::Review => "审查员",
        }
    }

    /// 是否只读（explore / plan / review：命令过只读白名单闸）
    pub fn readonly(self) -> bool {
        !matches!(self, SubagentKind::General)
    }

    /// 类型提示词（prompts/subagent-*.md）
    pub fn prompt(self) -> &'static str {
        match self {
            SubagentKind::General => SUBAGENT_GENERAL,
            SubagentKind::Explore => SUBAGENT_EXPLORE,
            SubagentKind::Plan => SUBAGENT_PLAN,
            SubagentKind::Review => SUBAGENT_REVIEW,
        }
    }

    /// 从字符串解析，解析不了给默认（工具参数宽容处理）
    pub fn parse_or_default(s: &str) -> Self {
        Self::parse(s).unwrap_or_default()
    }
}

/// 子智能体的系统提示词：类型提示词 + 工作目录。
/// 研究型（explore/plan/review）不带主提示词——对应 Claude Code「research fast and inexpensive」；
/// 类型提示词已自包含纪律与交付格式。
pub fn subagent_system(kind: SubagentKind, working_directory: &str) -> String {
    format!(
        "{}\n\n【工作目录】{}（相对路径按它解析；绝对路径可达任何处）",
        kind.prompt().trim(),
        working_directory
    )
}

/// 主提示词：桌面 sophon 目录的 prompts/main.md 优先（用户可改，与 sophon.prompt 同一机制），
/// 没有就用编译期内置版（零配置可用）。
pub fn main_prompt() -> String {
    if let Some(dir) = util::sophon_dir() {
        let p = dir.join("prompts").join("main.md");
        if let Ok(text) = std::fs::read_to_string(&p) {
            let t = text.trim();
            if !t.is_empty() {
                return t.to_string();
            }
        }
    }
    MAIN.trim().to_string()
}

/// 判断是不是「还是旧版内置提示词」（原样或只改了空白）——是才允许自动升级
pub fn is_legacy_main(prompt: &str) -> bool {
    let norm: String = prompt.split_whitespace().collect();
    let legacy: String = LEGACY_MAIN.split_whitespace().collect();
    if norm == legacy {
        return true;
    }
    // 特征兜底：两个特征串都在且总长接近旧版（防止全等因空白/换行差异漏判）
    LEGACY_MAIN_MARKS.iter().all(|m| prompt.contains(m)) && prompt.chars().count() < LEGACY_MAIN.chars().count() + 40
}

/// 旧版主提示词自动升级（settings 加载时调用）。升级了返回 true；用户自己改过的不动。
pub fn upgrade_legacy_main(prompt: &mut String) -> bool {
    if is_legacy_main(prompt) {
        *prompt = MAIN.trim().to_string();
        true
    } else {
        false
    }
}

/// 桌面 sophon 目录 prompts/ 覆盖（与 main.md 同一机制）：有就用，没有用内置
fn overridable(builtin: &str, name: &str) -> String {
    if let Some(dir) = util::sophon_dir() {
        if let Ok(text) = std::fs::read_to_string(dir.join("prompts").join(name)) {
            let t = text.trim();
            if !t.is_empty() {
                return t.to_string();
            }
        }
    }
    builtin.trim().to_string()
}

/// CLI ask 模式提示词（SOPHON_DIR/prompts/research.md 可覆盖）
pub fn research_prompt() -> String {
    overridable(RESEARCH, "research.md")
}

/// CLI run 模式提示词（SOPHON_DIR/prompts/run.md 可覆盖）
pub fn run_prompt() -> String {
    overridable(RUN, "run.md")
}

/// run 模式实际用的系统提示词：CLI 提示词（sophon.prompt）还是内置调研版（新版或旧版）
/// → 改用长程模式提示词；用户自己改过 → 原样使用（用户知道自己要什么）。
pub fn run_mode_prompt(cli_prompt: &str) -> String {
    let norm: String = cli_prompt.split_whitespace().collect();
    let research: String = RESEARCH.split_whitespace().collect();
    let legacy: String = LEGACY_RESEARCH.split_whitespace().collect();
    if norm == research || norm == legacy {
        run_prompt()
    } else {
        cli_prompt.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_parse() {
        assert_eq!(SubagentKind::parse("explore"), Some(SubagentKind::Explore));
        assert_eq!(SubagentKind::parse("General-Purpose"), Some(SubagentKind::General));
        assert_eq!(SubagentKind::parse("review"), Some(SubagentKind::Review));
        assert_eq!(SubagentKind::parse("规划"), Some(SubagentKind::Plan));
        assert_eq!(SubagentKind::parse("乱写"), None);
        assert_eq!(SubagentKind::parse_or_default(""), SubagentKind::General);
    }

    #[test]
    fn readonly_gate_flags() {
        assert!(!SubagentKind::General.readonly());
        assert!(SubagentKind::Explore.readonly());
        assert!(SubagentKind::Plan.readonly());
        assert!(SubagentKind::Review.readonly());
    }

    #[test]
    fn subagent_system_contains_prompt_and_cwd() {
        let s = subagent_system(SubagentKind::Explore, "C:/tmp/ws");
        assert!(s.contains("侦察员"));
        assert!(s.contains("C:/tmp/ws"));
    }

    #[test]
    fn legacy_upgrade() {
        // 原样旧版 → 升级
        let mut p = LEGACY_MAIN.to_string();
        assert!(upgrade_legacy_main(&mut p));
        assert_eq!(p, MAIN.trim());
        // 用户改过的 → 不动
        let mut p2 = "我自己的提示词，写得很认真。".to_string();
        assert!(!upgrade_legacy_main(&mut p2));
        assert_eq!(p2, "我自己的提示词，写得很认真。");
    }

    #[test]
    fn main_prompt_nonempty() {
        assert!(main_prompt().contains("智子"));
        assert!(MAIN.contains("子智能体"));
    }

    #[test]
    fn research_and_run_prompts() {
        assert!(research_prompt().contains("调研"));
        assert!(run_prompt().contains("长程"));
        // 内置调研版（新/旧）→ run 模式换长程提示词；用户自定义 → 原样
        assert_eq!(run_mode_prompt(RESEARCH.trim()), run_prompt());
        assert_eq!(run_mode_prompt(LEGACY_RESEARCH), run_prompt());
        assert_eq!(run_mode_prompt("我自己的提示词"), "我自己的提示词");
    }
}
