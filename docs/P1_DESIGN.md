# P1 实施方案（设计，待 #7e0d 合并 P0 后实施）

> 本文件只设计、不改代码。实施顺序按 P1-1 → P1-5。每项改完即 `cargo build` + `cargo test -p hermes-core` + 端到端自查。
> 约束：完全本地、严禁联网/云端；不擅自下载大模型；界面保持 egui/eframe，简洁美观、符合操作习惯。

## 本机已有资源（设计依据，已核实）

| 资源 | 位置 | 用途 |
|---|---|---|
| llama-server.exe | `C:\Program Files\Android\Android Studio\plugins\gemini\resources\llamacpp\` | P0 在用（#7e0d） |
| MiniCPM-V-4.7-1B-Q4_K_M.gguf + mmproj | `C:\Users\ultra\.cache\modelscope\hub\models\OpenBMB\MiniCPM-V-4.7-1B-gguf\` | 视觉模型 → analyze_image |
| whisper.cpp 1.9.1 全套（whisper-cli / whisper-server / whisper-stream…） | `C:\msys64\mingw64\bin\` | transcribe_audio（whisper-server 提供 OpenAI 兼容 `/v1/audio/transcriptions`） |
| Windows SAPI SpVoice（2 音色：中文 Huihui / 英文 Zira） | 系统自带 | speak（TTS），零模型零下载 |
| NVIDIA RTX 2060（Vulkan） | — | 加速 |

**缺口（不擅自下载，如实报告）**：
- 无 whisper 模型文件（`ggml-*.bin`）→ transcribe_audio 需模型才能跑
- 无扩散模型（safetensors/ckpt/onnx 均无）→ generate_image 无法本地完成

---

## P1-1 媒体附件渲染图片

**改 `crates/hermes-app/src/blocks.rs`**（Media 块，现 115–128 行只显示文件名）
- `App` 加纹理缓存：`pub image_cache: HashMap<String, egui::TextureHandle>`（`main.rs` 的 `App` 结构体 + `Default`）
- Media 渲染：`mime` 以 `image/` 开头时
  - 缓存未命中 → 用 `image` crate 解码文件为 `egui::ColorImage`，`ctx.load_texture(path, img, opts)` 存入缓存
  - `ui.image(&tex, [w, h])`，限宽 400px、保持宽高比
  - 解码失败/非图片 → 回退到现有文件名卡片
- **`crates/hermes-app/Cargo.toml` 加 `image = "0.25"`**（默认 features 含 png/jpeg，纯 Rust 无系统依赖）

验证：对话里附一张 png/jpg，消息里直接显示图。

## P1-2 Markdown 补表格 / 任务列表 / 图片

**改 `crates/hermes-app/src/markdown.rs`**
- 重构 `render_markdown`：先按空行/结构把文本收集成「块」序列（段落/标题/列表/引用/代码块/**表格**/**任务列表**），再逐块渲染（表格是多行，必须先收集）
- **表格**：识别 GFM 管道表（`| a | b |` 行 + `|---|---|` 分隔行）→ `egui::Grid`，表头 `strong`，单元格走 `parse_inline`
- **任务列表**：`- [ ]` / `- [x]` → `egui::CheckBox`（只读显示勾选态）+ 文本
- **图片**：`![alt](url)` → 本地路径用 P1-1 的纹理缓存渲染；http(s) 链接显示为 `[图片: alt]` 链接
- 行内解析 `parse_inline` 加 `![...](...)` 分支（现有只处理 `[...](...)` 链接）

验证：发含表格、`- [x]` 任务列表、`![](本地路径)` 的消息。

## P1-3 终端鼠标选择 / 双击选词 / IME

**改 `crates/hermes-app/src/terminal_view.rs`** + **`crates/hermes-core/src/terminal/mod.rs`**
- **鼠标选择**：渲染区 `Response` 加 `Sense::drag()`；记录起止（行,列）；渲染时用 `ui.painter().rect_filled` 高亮选中格子；选中时 `Ctrl+C` / `Ctrl+Shift+C` 复制选中文本（`ctx.copy_text`）
- **双击选词**：`double_clicked()` 定位格子，向左右扩展到非 `[a-zA-Z0-9_]` 边界；三击选整行
- **IME**：输入栏已是 `TextEdit`（原生支持 IME 预编辑），保持；终端网格内 preedit 显示较复杂，本轮先保证输入框 IME 可用（满足基本需求）
- **鼠标追踪透传**：`session.mouse_tracking == true` 时，把鼠标按下/移动编码成 SGR 序列 `\x1b[<按钮;列;行M/m` 写给 shell（`TerminalSession::write`）

验证：终端跑 `vim`/`htop`，鼠标选择复制、双击选词。

## P1-4 Agent 树连接线 / 详情页 / 按轮次分组

**改 `crates/hermes-app/src/agent_tree.rs`**
- **连接线**：`render_node` 里用 `ui.painter()` 画父→子折线（`egui::Shape::line_segment`，从父节点底边中点到子节点左边中点，颜色 `theme::border`）
- **详情页**：节点名旁加展开按钮（或单击选中）→ 面板下方显示详情：完整任务原文、完整结果、全部日志（不截断）、用量（`AgentUsage`）
- **按轮次分组**：按 `started_at` 聚类（相邻间隔 > 30s 算新轮次），每组一个可折叠标题「第 N 轮 · HH:MM:SS」

验证：派子智能体（含并行），看连接线、点节点看完整详情。

## P1-5 能力工具本地方案（看图 / 生图 / 听 / 说）

**改 `crates/hermes-core/src/tools/builtin.rs`**（`capability_stub!` 宏，1067–1109 行）+ 相关引擎

### 说话 speak（TTS）— 用 Windows SAPI，完全本地零模型 ✅可做
- 方案 A（推荐）：`hermes-core/Cargo.toml` 加 `windows = { version = "0.6x", features = ["Win32_System_Com", "Win32_Media_Audio"] }`，直接 COM 调 `ISpVoice::Speak`
- 方案 B（零新依赖备选）：`std::process::Command` 调 PowerShell `Add-Type -AssemblyName System.Speech; (New-Object System.Speech.Synthesis.SpeechSynthesizer).Speak($text)`（每次 ~200ms 启动，朗读按钮可接受）
- 实现 `Speak::execute` → 调 TTS；返回 `ToolOutput::Ok("已朗读")`
- 消息底栏「朗读」按钮（`blocks.rs` 80–82 行空实现）接同一套 TTS

### 听录音 transcribe_audio — whisper.cpp，需模型文件 ⚠️缺模型
- `whisper-server.exe` 提供 OpenAI 兼容 `/v1/audio/transcriptions`（启动：`whisper-server.exe --host 127.0.0.1 --port 0 --model <模型>`）
- 实现：检测本机 whisper 模型（`ggml-*.bin`，搜索 `C:\msys64\models`、`%USERPROFILE%\.cache\whisper` 等）
  - 有模型 → 启动 whisper-server（或直接 `whisper-cli -f 音频 -m 模型`），取转录文本
  - 无模型 → `ToolOutput::Err("需要 whisper 模型文件（本机未找到 ggml-*.bin）；请放入模型目录后重试")`（不擅自下载）
- 也可走 `OpenAICompatEngine`（whisper-server 是 OpenAI 兼容）

### 看图 analyze_image — 用已有 MiniCPM-V ✅可做
- MiniCPM-V 是视觉模型；llama-server 加载 `--mmproj` 后 `/v1/chat/completions` 支持图像输入（content parts 含 `image_url`）
- **扩展 `crates/hermes-core/src/engine/openai_compat.rs`**：消息编码支持图像（`ChatMessage` 加 `images: Vec<String>` 或 `MessageBlock` 加 `BlockKind::Image{path}`；编码时转成 `{"type":"image_url","image_url":{"url":"data:image/png;base64,..."}}`）
- `AnalyzeImage::execute`：读图片 → base64 → 经 MiniCPM-V 引擎发问（问题 = `question` 参数）→ 返回描述

### 生成图片 generate_image — 无扩散模型 ⚠️缺模型
- 本机无扩散模型（safetensors/ckpt/onnx 均未找到）。按「不擅自下载大模型」约束，不做假实现
- `GenerateImage::execute` 返回如实说明：「需要本地扩散模型（stable-diffusion.cpp + SD 模型），当前本机未找到」
- 待用户提供模型文件后接入（stable-diffusion.cpp 或 diffusers 本地）

验证：speak 出声（中文）；analyze_image 用 MiniCPM-V 描述一张本机图片；transcribe_audio 在放入 whisper 模型后转录一段录音。

---

## 实施顺序与依赖

1. **P1-1**（媒体图片）→ 无依赖，先做（P1-2 图片、P1-5 analyze_image 都复用纹理缓存）
2. **P1-2**（Markdown）→ 依赖 P1-1 的纹理缓存
3. **P1-5 speak**（SAPI TTS）→ 独立，可与 P1-2 并行
4. **P1-5 analyze_image**（MiniCPM-V 图像输入）→ 依赖 P0 的引擎改动已合并
5. **P1-3**（终端交互）→ 独立
6. **P1-4**（Agent 树）→ 独立
7. **P1-5 transcribe_audio** → 等 whisper 模型文件到位
8. **P1-5 generate_image** → 等扩散模型文件到位

## 缺口汇总（需用户提供，不擅自下载）

- whisper 模型文件（`ggml-tiny/base/small.bin`）→ 解锁 transcribe_audio
- 扩散模型（SD 1.5 / SDXL safetensors）→ 解锁 generate_image
