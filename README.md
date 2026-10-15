# 智子（Sophon）—— 全自主智能体核心（Rust）

按 `docs/SOPHON.md` 的定义实现的全自主智能体。**无内置图形 / 终端界面**
（原 egui 桌面端与后补的 TUI 层已于 2026-10-15 按用户要求移除，历史实现见 git 历史）。

当前形态：

- `sophon-core`：无界面核心库——会话、引擎、Agent、工具、终端、MCP、
  自主策略、经验库、自我改进、沙盒 / 压缩 / 回滚 / 自检、联网能力。
- `sophon`：联网研究命令行（`crates/sophon-core/src/bin/sophon.rs`）。

## 功能

- **数据模型与会话库**：JSON 同构规则（UUID 大写、ISO8601 无毫秒、可选字段不写键）；
  会话增删改查 / 搜索 / 持久化 / fork。
- **引擎**：内置离线引擎（规则路由，零配置可用）；本机 OpenAI 兼容引擎
  （Ollama / llama.cpp / LM Studio，地址限 127.0.0.1 / localhost，构造时校验，拒绝云端）。
- **Agent 主循环**：回合线程模型、停止状态机、审批、插话、每轮验证检查点、
  活性看门狗（无输出停 / deadline / 原地打转检测）。
- **子 Agent 系统**：并行 / 后台 / 嵌套、互发消息、团队状态、用量统计。
- **工具集**：run_shell / read_file / write_file / edit_file / find_files / search_files /
  search_keywords / 终端读写 / 任务状态 / 派遣与子 Agent 管理等，+ MCP 动态注册的工具。
- **内置终端**：ConPTY + VT 模拟（16 色 / 真彩 / 光标 / 5000 行回滚），Agent 可读写。
- **全自主策略**（autonomy.rs）：三级自主（autonomous / safe / yolo）、破坏性检测、
  可逆替代（回收站 / 备份 / 临时目录）、错误自愈、交付自检。
- **经验库**（learning.rs）：问题 → 尝试 → 结果 → 教训，新任务先检索。
- **自我改进**（selfimprove.rs）：git 分支改自身代码 → cargo check / cargo test 全绿 →
  合并 → 重新编译；失败自动回滚并记教训。
- **安全与自省**：沙盒隔离（sandbox.rs）、上下文压缩（context_compress.rs）、
  回滚（rollback.rs）、自检（selfcheck.rs）。
- **联网能力**（web.rs）：不调搜索 API——直接抓搜索引擎结果页 + 用本机 Edge/Chrome
  的 headless 模式渲染 JS 页面（零安装）。
- **MCP 客户端**：连接本机 stdio MCP 服务，取其工具。
- **密钥库**：API Key 只以名字流转、明文加密落盘（secrets.json）。

## 构建

需要 Rust（Windows GNU 工具链）。编译 `parking_lot_core` 需要 MinGW binutils，
仓库带了构建脚本自动设 PATH：

```powershell
.\build.ps1            # debug
.\build.ps1 --release  # release
```

或手动：

```powershell
$env:PATH = "C:\msys64\mingw64\bin;$env:PATH"
cargo build --release
```

产物：`target/release/sophon.exe`（核心是库 `sophon_core`，供上层程序链接）。

## sophon 用法

```text
sophon search   <bing|baidu> <query> <count> <outfile>
sophon fetch    <url> <outfile> [max_chars] [timeout_secs] [virtual_ms]
sophon get      <url> <outfile> [max_chars]
sophon research <question> <max_pages> <outfile>
```

搜索结果与页面正文以 Markdown 形式落盘为证据文件（UTF-8）。

## 测试

```powershell
$env:PATH = "C:\msys64\mingw64\bin;$env:PATH"
cargo test -p sophon-core
```

覆盖：glob 匹配、UUID / ISO8601、取消令牌层级、消息树、会话库、离线路由、
本机地址校验（拒绝云端）、工具实际执行、终端 VT 模拟、Agent 树、助手运行时、
密钥库、自主策略 / 沙盒 / 压缩 / 回滚 / 自检。

## 结构

```
crates/sophon-core/   无界面核心
  src/models.rs           数据模型
  src/store/mod.rs        会话库 / 设置 / 密钥库 / 引擎库 / AppCore
  src/engine/             ChatEngine 接口 + 离线引擎 + 本机 OpenAI 兼容引擎
  src/agent/              主循环（runner）+ 子 Agent 系统（team）
  src/tools/              工具接口 + 全部内置工具
  src/terminal/           ConPTY + VT 模拟器 + 管理器
  src/assistant/          助手运行时（时间线 / 状态板 / 记忆 / 话题摘要）
  src/mcp/                MCP 客户端（本机 stdio）
  src/remote/             远程对接
  src/autonomy.rs         自主策略
  src/learning.rs         经验库
  src/selfimprove.rs      自我改进管道
  src/sandbox.rs          沙盒隔离
  src/context_compress.rs 上下文压缩
  src/rollback.rs         回滚
  src/selfcheck.rs        自检
  src/web.rs              联网能力（结果页抓取 + headless 渲染）
  src/bin/webcli.rs       webcli 命令行入口
  src/util/               路径 / JSON / 取消令牌 / 主线程命令队列
```

## 数据位置

默认在用户数据目录（`FRIDAY_HOME` 换根目录；`FRIDAY_DATA_DIR` / `FRIDAY_CACHE_DIR`
分别覆盖数据 / 缓存位置）。

## 约束

- 引擎地址限本机（127.0.0.1 / localhost），不调任何云端 API。
- 联网抓取用本机浏览器 headless，不引第三方下载。
- 界面层已移除；需要界面时可基于 core 的事件流另行实现。
