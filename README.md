# Hermes —— Friday 桌面助理（Rust 版）

按 `docs/` 里的设计（Friday Qt 版架构、ASSISTANT/REMOTE/TERMINAL/STYLE 等规格），
用 **Rust + egui/eframe** 重写的桌面端程序。全部本地运行、不联网。

## 功能（对齐 docs，不漏项）

- **会话与聊天**：侧边栏按工作目录分组、搜索、置顶/改名/删除；对话页 Markdown 渲染、
  流式输出、工具卡（运行中/成功/失败/待确认/已停止/已拒绝/排队 七态色）、思考块、
  消息统计、已停止、复制、fork、朗读；输入栏发送/停止/工作中排队（interjection）、
  附件、模型/权限/思考/工作目录胶囊、「下一轮生效」指示。
- **Agent 主循环**：回合线程模型（主线程改界面可见状态、工作线程顺序书写）、
  停止状态机（is_stopping 只认第一次）、审批（approve/deny）、插话、恢复、重试、
  每轮验证检查点、工作中换模型（下一轮生效）。
- **工具集**：run_shell（带审批/超时/取消）、read_file、write_file、edit_file、
  find_files（glob，支持 `*` `?` `{a,b}`）、search_files（正则）、search_keywords、
  read_terminal、run_in_terminal、update_task_state、delegate_task、run_parallel_tasks、
  message_agent、stop_agent、wait_agents、read_agent_log。
- **子 Agent 系统**：团队状态、Agent 树面板**实时**展示每个子智能体的运行/任务/日志/结果，
  支持并行、后台、嵌套、停止、互发消息。
- **内置终端**：标签页、ConPTY + VT 模拟（颜色/光标/滚动缓冲 5000 行）、Agent 可读写。
- **助手页**：时间线、状态板、记忆、话题摘要（本地规则生成）。
- **MCP 客户端**：连接本机 stdio MCP 服务，取其工具（不联网）。
- **大脑（引擎）**：默认内置离线引擎（规则路由，零配置可用）；可指向本机跑的
  OpenAI 兼容服务（Ollama / llama.cpp / LM Studio，**地址限 127.0.0.1 / localhost**，
  构造时校验，拒绝任何云端地址）。
- **密钥库**：API Key 只存名字、明文加密落盘（docs §5.3 secrets.json）。

## 构建

需要 Rust（Windows GNU 工具链）。Windows 上编译 `parking_lot_core` 需要 MinGW binutils
（`dlltool`/`as`/`ld`），Rust 自带的 self-contained 缺 `as.exe`，所以用 MSYS2 的：

```powershell
.\build.ps1            # debug
.\build.ps1 --release  # release
```

或手动：

```powershell
$env:PATH = "C:\msys64\mingw64\bin;$env:PATH"
cargo build --release
```

## 运行

```powershell
.\target\release\hermes-app.exe
```

数据默认在 `%APPDATA%\Hermes`（可用 `FRIDAY_HOME` / `FRIDAY_DATA_DIR` 覆盖）。

## 测试

```powershell
$env:PATH = "C:\msys64\mingw64\bin;$env:PATH"
cargo test -p hermes-core
```

覆盖：glob 匹配、UUID/ISO8601、取消令牌层级、消息树、会话库（增删改查/搜索/持久化）、
离线路由（找文件/读文件/运行命令/派子 Agent/问答）、本机地址校验（拒绝云端）、
工具实际执行（run_shell/read/write/edit/find/search）、终端 VT 模拟（文本/颜色）、
Agent 树（运行/日志/完成）、助手（时间线/记忆/话题摘要）、密钥库。

## 结构

```
crates/hermes-core/  无界面核心
  src/models.rs      数据模型（与 docs §5.3 JSON 同构）
  src/store.rs       会话库/设置/密钥库/引擎库/AppCore
  src/engine/        ChatEngine 接口 + 内置离线引擎 + 本机 OpenAI 兼容引擎
  src/agent/         主循环（runner）+ 子 Agent 系统（team）
  src/tools/         工具接口 + 全部内置工具
  src/terminal/      ConPTY + VT 模拟器 + 管理器
  src/assistant/     助手运行时（时间线/状态板/记忆/话题摘要）
  src/mcp/           MCP 客户端（本机 stdio）
  src/util/          路径/JSON/取消令牌/主线程命令队列
crates/hermes-app/   eframe 界面
  src/main.rs        入口、布局、字体
  src/sidebar.rs     侧边栏（会话分组）
  src/chat.rs        对话页（消息流 + 输入栏）
  src/blocks.rs      消息块渲染（工具卡/思考/文本）
  src/agent_tree.rs  Agent 树面板（子智能体实时动作）
  src/terminal_view.rs 内置终端面板
  src/assistant_view.rs 助手页
  src/settings_view.rs 设置页
  src/markdown.rs    极简 Markdown 渲染
  src/theme.rs       原生观感主题（浅色/深色跟随系统）
```

## 约束

- 完全本地资源，不调用任何云端 API、不联网依赖（引擎地址限本机，已测试强制）。
- 桌面端实时、完整展示所有执行细节（含子智能体动作）——Agent 树面板 + 工具卡 + 终端。
