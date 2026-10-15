// 自我学习（SOPHON.md §5）：
// 经验库 lessons.jsonl——问题 → 尝试 → 结果 → 教训；交付后写入、新任务前检索；
// 错误模式库——同类错误第 2 次直接给已知修法。
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// 一条经验 / 教训
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Lesson {
    pub ts: String,
    pub problem: String,
    pub attempts: Vec<String>,
    pub result: String,
    pub lesson: String,
    pub tags: Vec<String>,
}

/// 错误模式：同一报错 → 已知修法
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ErrorPattern {
    /// 错误签名（归一化后的报错关键行）
    pub signature: String,
    pub fix: String,
    pub hits: u32,
    pub updated_at: String,
}

/// 经验库（数据目录下 lessons.jsonl + error_patterns.json）
#[derive(Default)]
pub struct LessonStore {
    pub lessons: Vec<Lesson>,
    pub error_patterns: HashMap<String, ErrorPattern>,
    file: Option<PathBuf>,
    patterns_file: Option<PathBuf>,
}

impl LessonStore {
    pub fn load(data_dir: &std::path::Path) -> Self {
        let file = data_dir.join("lessons.jsonl");
        let patterns_file = data_dir.join("error_patterns.json");
        let mut store = LessonStore {
            lessons: Vec::new(),
            error_patterns: HashMap::new(),
            file: Some(file.clone()),
            patterns_file: Some(patterns_file.clone()),
        };
        if let Ok(text) = std::fs::read_to_string(&file) {
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Ok(l) = serde_json::from_str::<Lesson>(line) {
                    store.lessons.push(l);
                }
            }
        }
        if let Ok(v) = std::fs::read_to_string(&patterns_file) {
            if let Ok(map) = serde_json::from_value::<HashMap<String, ErrorPattern>>(serde_json::from_str(&v).unwrap_or_default()) {
                store.error_patterns = map;
            }
        }
        store
    }

    /// 测试用（不落盘）
    pub fn in_memory() -> Self {
        LessonStore::default()
    }

    pub fn record(&mut self, lesson: Lesson) {
        // 相同 problem+lesson 去重（只更新时间）
        if let Some(old) = self
            .lessons
            .iter_mut()
            .find(|l| l.problem == lesson.problem && l.lesson == lesson.lesson)
        {
            old.ts = lesson.ts;
            old.result = lesson.result;
        } else {
            self.lessons.push(lesson);
        }
        self.save();
    }

    /// 按关键词检索最相关的教训（命中 tags / problem / lesson 计分）
    pub fn search(&self, query: &str, top_k: usize) -> Vec<&Lesson> {
        let words: Vec<String> = query
            .split(|c: char| c.is_whitespace() || c == ',' || c == '，' || c == '、')
            .map(|w| w.trim().to_lowercase())
            .filter(|w| !w.is_empty())
            .collect();
        if words.is_empty() {
            return Vec::new();
        }
        let mut scored: Vec<(usize, &Lesson)> = self
            .lessons
            .iter()
            .map(|l| {
                let hay = format!(
                    "{} {} {} {}",
                    l.problem.to_lowercase(),
                    l.lesson.to_lowercase(),
                    l.tags.join(" ").to_lowercase(),
                    l.result.to_lowercase()
                );
                let score = words.iter().filter(|w| hay.contains(w.as_str())).count();
                (score, l)
            })
            .filter(|(s, _)| *s > 0)
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.into_iter().take(top_k).map(|(_, l)| l).collect()
    }

    /// 记录一次工具错误；返回已知修法（第 2 次起有）
    pub fn record_error(&mut self, error_text: &str, fix_hint: Option<String>) -> Option<String> {
        let sig = error_signature(error_text);
        let (known, give) = {
            let entry = self.error_patterns.entry(sig.clone()).or_insert_with(|| ErrorPattern {
                signature: sig,
                fix: String::new(),
                hits: 0,
                updated_at: crate::models::now_iso(),
            });
            entry.hits += 1;
            entry.updated_at = crate::models::now_iso();
            if entry.fix.is_empty() {
                if let Some(f) = fix_hint {
                    entry.fix = f;
                }
            }
            (if entry.fix.is_empty() { None } else { Some(entry.fix.clone()) }, entry.hits >= 2)
        };
        self.save();
        // 第 2 次起才给修法（第一次还在学）
        if give {
            known
        } else {
            None
        }
    }

    fn save(&self) {
        let (Some(file), Some(patterns_file)) = (&self.file, &self.patterns_file) else {
            return;
        };
        if let Ok(mut f) = std::fs::File::create(file) {
            use std::io::Write;
            for l in &self.lessons {
                if let Ok(line) = serde_json::to_string(l) {
                    let _ = writeln!(f, "{}", line);
                }
            }
        }
        if let Ok(v) = serde_json::to_value(&self.error_patterns) {
            let _ = std::fs::write(patterns_file, serde_json::to_string_pretty(&v).unwrap_or_default());
        }
    }
}

/// 错误签名：取报错的第一行非空内容、截断、去掉数字（让同类错误归一）
pub fn error_signature(error_text: &str) -> String {
    let first = error_text
        .lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .unwrap_or("unknown");
    // 去掉路径与数字，保留结构
    let mut sig = String::new();
    let mut prev_digit = false;
    for c in first.chars() {
        if c.is_ascii_digit() {
            if !prev_digit {
                sig.push('#');
            }
            prev_digit = true;
        } else {
            prev_digit = false;
            if c == '\\' || c == '/' {
                sig.push(' ');
            } else {
                sig.push(c);
            }
        }
    }
    let sig: String = sig.split_whitespace().collect::<Vec<_>>().join(" ");
    sig.chars().take(120).collect()
}

/// 为新任务构造「相关经验」提示段（拼进上下文）
pub fn lessons_prompt(lessons: &[&Lesson]) -> Option<String> {
    if lessons.is_empty() {
        return None;
    }
    let mut out = String::from("以下是从经验库里检索到的相关教训，动手前先参考：\n");
    for l in lessons {
        out.push_str(&format!(
            "- 问题：{} → 教训：{}\n",
            l.problem, l.lesson
        ));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lesson(problem: &str, lesson_text: &str, tags: &[&str]) -> Lesson {
        Lesson {
            ts: crate::models::now_iso(),
            problem: problem.into(),
            attempts: vec![],
            result: "成功".into(),
            lesson: lesson_text.into(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn record_and_search() {
        let mut s = LessonStore::in_memory();
        s.record(lesson("cargo build 失败：找不到 msyh.ttc", "用 C:\\Windows\\Fonts\\msyh.ttc 绝对路径", &["字体", "png"]));
        s.record(lesson("PowerShell 里 grep 不存在", "用 Select-String 或 cmd findstr", &["shell"]));
        assert_eq!(s.lessons.len(), 2);
        let hits = s.search("字体 png", 3);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].lesson.contains("msyh"));
        assert!(s.search("shell", 3).len() == 1);
        assert!(s.search("不相关词汇xyz", 3).is_empty());
        // 去重
        s.record(lesson("cargo build 失败：找不到 msyh.ttc", "用 C:\\Windows\\Fonts\\msyh.ttc 绝对路径", &["字体"]));
        assert_eq!(s.lessons.len(), 2);
    }

    #[test]
    fn error_pattern_second_time_gives_fix() {
        let mut s = LessonStore::in_memory();
        let err = "error[E0382]: borrow of moved value: `x`\n  --> src/main.rs:10";
        assert!(s.record_error(err, Some("先 clone 再移动".into())).is_none());
        let fix = s.record_error(err, None);
        assert_eq!(fix.as_deref(), Some("先 clone 再移动"));
        // 第三次也有
        assert!(s.record_error(err, None).is_some());
    }

    #[test]
    fn signature_normalizes() {
        let a = error_signature("error[E0382]: cannot find value `x` in this scope (1)");
        let b = error_signature("error[E0382]: cannot find value `x` in this scope (22)");
        assert_eq!(a, b);
        // 不同结构的报错签名不同
        let c = error_signature("error[E0308]: mismatched types");
        assert_ne!(a, c);
    }
}
