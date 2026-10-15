// 工具层：工具接口、上下文、注册表。
// 对应 docs/ARCHITECTURE.md 的 ToolRegistry / ToolEvent / AgentTools。
use crate::agent::team::AgentTeamState;
use crate::engine::{ChatEngine, ToolDescriptor};
use crate::models::*;
use crate::store::SecretStore;
use crate::terminal::TerminalManager;
use crate::util::{CancelToken, MainThread};
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

pub mod builtin;
pub use builtin::*;

/// 工具执行上下文（工作线程持有；共享状态用各自的锁，不长期持全局锁）
pub struct ToolContext {
    pub working_directory: String,
    pub conversation_id: String,
    pub terminal_manager: Arc<Mutex<TerminalManager>>,
    pub team: Arc<Mutex<AgentTeamState>>,
    pub task_states: Arc<Mutex<HashMap<String, TaskState>>>,
    pub cancel: CancelToken,
    pub main_thread: MainThread,
    /// 子 Agent 嵌套深度（主 Agent 为 0）
    pub depth: u32,
    pub engine: Arc<dyn ChatEngine>,
    pub secrets: Arc<Mutex<SecretStore>>,
    pub system_prompt: String,
    /// 沙盒：命令必须在沙盒内跑；None = 直跑
    pub sandbox: Option<std::sync::Arc<crate::sandbox::Sandbox>>,
}

#[derive(Debug)]
pub enum ToolOutput {
    Ok(String),
    Err(String),
}

impl ToolOutput {
    pub fn is_ok(&self) -> bool {
        matches!(self, ToolOutput::Ok(_))
    }
    pub fn into_text(self) -> String {
        match self {
            ToolOutput::Ok(s) => s,
            ToolOutput::Err(s) => s,
        }
    }
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn title(&self) -> &str;
    fn description(&self) -> &str;
    /// 参数的 JSON Schema
    fn parameters(&self) -> Value;
    /// 是否高危（需要审批）
    fn needs_approval(&self, _args: &Value) -> bool {
        false
    }
    /// 是否写操作（ReadOnly 模式下拒绝）
    fn is_write(&self) -> bool {
        false
    }
    fn execute(&self, args: &Value, ctx: &mut ToolContext) -> ToolOutput;

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: self.name().to_string(),
            title: self.title().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
        }
    }
}

pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn builtin() -> Self {
        let tools: Vec<Box<dyn Tool>> = vec![
            // 仅保留 exec：文件/搜索/构建等一切操作都用命令在沙盒内完成
            Box::new(builtin::RunShell),
        ];
        ToolRegistry { tools }
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.iter().find(|t| t.name() == name).map(|t| t.as_ref())
    }

    pub fn descriptors(&self) -> Vec<ToolDescriptor> {
        self.tools.iter().map(|t| t.descriptor()).collect()
    }

    pub fn names(&self) -> Vec<String> {
        self.tools.iter().map(|t| t.name().to_string()).collect()
    }

    pub fn execute(&self, name: &str, args: &Value, ctx: &mut ToolContext) -> ToolOutput {
        match self.get(name) {
            Some(tool) => tool.execute(args, ctx),
            None => ToolOutput::Err(format!("未知工具：{}", name)),
        }
    }
}

/// 简单的 glob 匹配（* 任意、? 单字符）
pub fn glob_match(pattern: &str, name: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = name.chars().collect();
    glob_rec(&pat, 0, &txt, 0)
}

fn glob_rec(pat: &[char], mut pi: usize, txt: &[char], mut ti: usize) -> bool {
    while pi < pat.len() {
        match pat[pi] {
            '*' => {
                // 跳过连续的 *
                while pi < pat.len() && pat[pi] == '*' {
                    pi += 1;
                }
                if pi == pat.len() {
                    return true;
                }
                while ti <= txt.len() {
                    if glob_rec(pat, pi, txt, ti) {
                        return true;
                    }
                    ti += 1;
                }
                return false;
            }
            '?' => {
                if ti >= txt.len() {
                    return false;
                }
                ti += 1;
                pi += 1;
            }
            '{' => {
                // 花括号展开：{a,b,c}
                let mut depth = 1;
                let mut end = pi + 1;
                while end < pat.len() {
                    match pat[end] {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    end += 1;
                }
                if end >= pat.len() {
                    return false; // 没有匹配的 }
                }
                let inner = &pat[pi + 1..end];
                let alternatives = split_alternatives(inner);
                let rest = &pat[end + 1..];
                for alt in alternatives {
                    let mut combined = alt.clone();
                    combined.extend_from_slice(rest);
                    if glob_rec(&combined, 0, txt, ti) {
                        return true;
                    }
                }
                return false;
            }
            c => {
                if ti >= txt.len() || txt[ti] != c {
                    return false;
                }
                ti += 1;
                pi += 1;
            }
        }
    }
    ti == txt.len()
}

/// 花括号内按顶层逗号分割
fn split_alternatives(inner: &[char]) -> Vec<Vec<char>> {
    let mut result = Vec::new();
    let mut current = Vec::new();
    let mut depth = 0;
    for &c in inner {
        match c {
            '{' => {
                depth += 1;
                current.push(c);
            }
            '}' => {
                depth -= 1;
                current.push(c);
            }
            ',' if depth == 0 => {
                result.push(current.clone());
                current.clear();
            }
            _ => current.push(c),
        }
    }
    result.push(current);
    result
}

/// 把相对路径解析为基于工作目录的绝对路径
pub fn resolve_path(working_directory: &str, path: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::path::Path::new(working_directory).join(p)
    }
}
