# UI 移植规格：Swift v1.5–v1.9 → Qt（界面部分）

只读分析产物。Swift 基准 = `origin/main`（merge-base `44116fb` → `e71cf89` v1.5 → `76534ff` v1.6 → `e3aa578` v1.7 → `f1f6d58` v1.8 → `e2f8f41` v1.9）。
Swift 行号 = `origin/main:Friday/Friday/<路径>`；Qt 行号 = 当前工作区 `qt/src/app/<路径>`（这五个版本的界面一项都还没移植：`isStopping / stoppedByUser / interjected / fromAgent / queuedInputs / liveAgents / treeFocus` 在 qt/ 里都搜不到）。
「核心」= AgentRunner / AgentTeamState / SubAgentRunner 等 core 层（另有规格负责实现）；本文第 3 节只写界面需要核心暴露什么。
**附录说明**：本文是 `qt/docs/PORT_V15_V19.md`（总契约）的附录，对应工作包 **chat-ui**（对话里的部分、d696a44）、**tree-tray**（Agent 树、工具栏合计、托盘、DebugHarness）、
**settings**（§1.4「设置」、§2.8）。接口、归属、决定以总契约为准；标「**已由 PORT_V15_V19.md §x 取代**」的段落不再生效，其余（行为、逐字文案、布局数值、脚本）照做。

界面契约以 `qt/docs/STYLE.md` 为准：QQC 控件只用自身 API（不换 background / contentItem / indicator），要叠东西作为额外子项；自绘块用平面 Rectangle + Theme 色；图标 `Icon` / `Theme.iconSource` + `adoptIconColor`；提示 `HelpArea` / `QQC.ToolTip`（800 ms）；状态色当文字用 `Theme.orangeText / greenText / redText`；`Color.primary.opacity(x)` → `Theme.opacity(Theme.primary, x)`，`.fill.tertiary / .quaternary` → `Theme.tertiaryFill / quaternaryFill`，`.tint` → `Theme.accent`；圆角取 `Theme.radius.*`（都很小：Swift 20 → `bubble` 10，16/14/12/10/9 → `card` 6，8 → `medium` 4，胶囊 → `height / 2`）。快捷键提示里的「⌘」照现有写法换成 `App.shortcutModifier`（Windows / Linux 显示 `Ctrl+`）。

---

## 0. 总览

### 0.1 改动清单

| 版本 | Swift（视图） | Qt 改 | Qt 新 | 依赖核心（§3） |
|---|---|---|---|---|
| v1.5 停止状态 | SpinningRefreshIcon（新）、ComposerView 停止键、MessageRow 打字动画 / 底栏、ToolCard「已停止」 | ComposerView.qml、MessageRow.qml、ToolCard.qml、MessageListModel | chat/SpinningRefreshIcon.qml | `isStopping`、`ChatMessage.stoppedByUser`、`ToolEvent.stopped` |
| v1.6 工作中补充 | ComposerView（可发送条件、停止 + 排队两个键、占位、「下一轮生效」）、ChatView（submit、空状态、QueuedBubbles）、MessageRow「工作中补充」、ModelStatusView 工作中可换 | ComposerView.qml、ChatView.qml、MessageList.qml、MessageRow.qml、ModelStatusView.qml、ChatViewModel、MessageListModel | chat/QueuedBubbles.qml、chat/DashedOutline.qml | `submit`、排队 / 暂停 / 撤回 / 恢复、`pendingTurnChanges`、`ChatMessage.interjected` |
| v1.7 父子协作 | MessageRow 汇报卡片（fromAgent）、AgentControls（卡片上发消息 / 单独停止，v1.8 改成只在树的详情里发消息） | MessageRow.qml、ToolCard.qml、MessageListModel | chat/AgentControls.qml | `ChatMessage.fromAgent`、`noticeTitle`、`AgentTeamState`（running / queues / activity）、`sendToAgent / stopAgent / removeAgentQueued`、`agentStatusText` |
| v1.8 Agent 树 + 用量 | ToolCard 默认收起 + 模型胶囊 + 并行小标签 + 深层收窄；AgentTreePanel.swift（新，863 行）；ChatView 右侧一列 + 拖动条 + 工具栏两项；ContentView 收侧边栏；设置 › 子 Agent / 引擎「起步并发」 | ToolCard.qml、SubAgentTranscript.qml、BlockView.qml、ChatView.qml、ContentView.qml、AgentsSettingsView.qml、EngineEditor.qml、SecretsSettingsView.qml、ServiceModelPicker.qml、ServiceModelsSheet.qml、ModelMenuBuilder.cpp、FridayApp.h/.cpp（删 agentTeamMaxDepth）、MessageListModel | chat/ModelPill.qml、ChildAgentChips.qml、AgentStatusIcon.qml、AgentTreeButton.qml、ConversationUsageBadge.qml、AgentTreePanel.qml、AgentPanelSummary.qml、AgentTreeRow.qml、AgentNodeCard.qml、AgentDetailView.qml、PanelResizeHandle.qml；viewmodels/AgentTreeModel.{h,cpp}、viewmodels/UiState.{h,cpp} | `ToolEvent.usage`（AgentUsage）、`team.speed / passTokens / treeFocus`、`mainLiveSpeed`、`AppSettings.subAgentTurnLimit`、`currentPath` |
| v1.9 菜单栏 | MenuBarLabel（图标右边两行）、MenuBarMenu 多一行 | FridayApp.h/.cpp（托盘 tooltip + 菜单行） | — | `liveAgents / liveSpeed / liveMeterChanged` |

### 0.2 关键决定（细节见第 2 节）

1. **旋转刷新图标**：`Icon` + `RotationAnimator`（渲染线程转，等价于 Swift 用 Core Animation 而不用 SwiftUI 动画的理由），顺时针 0.9 s 一圈，颜色 `Theme.secondaryLabel`。
2. **Agent 树面板的位置**：照 Swift——**ChatView 里、对话区右边的一列**（不是系统检查器 / Sheet / 独立窗口），在工具栏行**下面**（工具栏行仍横跨整个详情区，Windows 11 上就是合并标题栏），左边一条可拖动的分隔条（260–560，默认 320），开关和宽度全局持久化（`agentTreeVisible / agentTreeWidth`，新 `UiState` 单例）。分隔条不穿过工具栏行（工具栏右端的用量徽标和树按钮正好在面板上方，竖线会从它们中间穿过；也避开标题栏命中测试）。
3. **托盘（v1.9）**：Windows / Linux 托盘不能在图标右边放字。**不把字画进图标**（16–32 px 的方形图标里两行字不可读，见 §2.9），改为：tooltip 三行 `Friday` / `N AI` / `速度`；托盘菜单状态行下面一行禁用项「N AI 在工作 · 速度」（同 Swift 的 MenuBarMenu，只在 N > 0 时显示）；值变了才更新（核心每秒最多发一次）。
4. **新图标 9 个**（mapping.json + gen_icons.py，§2.10）：`stop.circle`、`stop.circle.fill`、`text.bubble`、`waveform.path.ecg`、`point.3.filled.connected.trianglepath.dotted`、`arrow.up.forward`、`arrow.down.backward`、`arrow.up.circle.fill`、`arrow.up.message`。
5. **数字格式**：Swift 有三套（StatsLabel.compact、AgentTree.compact / speedText、MenuBarLabel.speedText），现有 `Format::compact` 是「万 / 亿」，**不能复用**，新增三个格式函数（§3.6）。
6. **数据**：Agent 树的结构、合计、行由新的 C++ 模型 `AgentTreeModel`（每个 ChatView 一个，工具栏按钮 / 徽标 / 面板共用）算；按秒变化的部分由核心的 `AgentTeam.clock` 驱动，不在每张卡片里各开一个 Timer。
   （**PORT_V15_V19.md §3.4 定稿**：主 Agent 在生成或有子 Agent 在跑时，core 的计量每秒 `tickClock()` +1，都停下后再 +1 一次；`AgentTeam` 内部加锁、任意线程可调，信号只在主线程发。）

---

## 1. Swift 行为（按版本、按视图）

### 1.1 v1.5 停止状态（e71cf89，提交 a9a6d27）

#### SpinningRefreshIcon（新，`Views/SpinningRefreshIcon.swift:1–56`）
- NSViewRepresentable；参数 `size = 14`、`weight = .bold`、`symbol = "arrow.triangle.2.circlepath"`（两个箭头首尾相接的刷新 / 同步符号）。
- 颜色 = `NSColor.secondaryLabelColor`，随浅色 / 深色更新（:45–51）；形状是符号做遮罩。
- 动画：Core Animation 绕中心**顺时针**一圈 0.9 s、无限循环（:33–39）。注释写明不用 symbolEffect：SwiftUI 驱动的动画会让整个窗口显示列表每帧重走。
- 无障碍：元素、角色 image、标签「正在停止」（:15–17）。尺寸 = size × size（:53–55）。
- 用处：输入框停止键（size 13）、消息底栏「正在停止…」（size 10，weight .semibold）、子 Agent 卡片的停止键（size 13）。

#### ComposerView 停止键（`Views/ComposerView.swift:379–402`）
- 只在 `runner.isGenerating` 时出现。点下 → `runner.stop`。
- `runner.isStopping`（点停止到真正停下之间）：图标换成 `SpinningRefreshIcon(size: 13)`，玻璃去掉红色着色（普通底），`.disabled(true)` + 不接收点击，提示「正在停止…」。
- 平时：`stop.fill`（12pt bold），红色 0.55 着色，提示「停止生成 (⌘.)」。
- 核心 `stop()` 只认第一次（`AgentRunner.swift:908–911`：`guard isGenerating, !isStopping`），⌘. / 菜单再按也不会重复中止；`isStopping` 在 `finish()` 收尾时复位（:1479）。

#### MessageRow（`Views/MessageRow.swift`）
- 打字指示（:130–133）：`isStreaming && !runner.isStopping && (blocks.isEmpty || (lastIsTool && !isWaitingForTool))`——点了停止就不再显示「还在写」的动画。
- 底栏出现条件（:154–160）：`stats != nil || (isStreaming && runner.isStopping) || (!isStreaming && (!blocks.isEmpty || stoppedByUser == true))`——没有任何块但被停止的消息也有底栏。
- 底栏（:176–220，HStack 间距 12，整体 caption、三级色、borderless）从左到右：
  1. `isStreaming && isStopping`：HStack(6){ `SpinningRefreshIcon(size: 10, weight: .semibold)`，「正在停止…」} 二级色——**代替** StatsLabel；否则有 stats 时照旧 StatsLabel。
  2. `!isStreaming && stoppedByUser == true`：`Label("已停止", systemImage: "stop.circle")` 二级色，提示「这一轮是你点停止结束的，回答和操作可能不完整；接着发消息时 Agent 会知道这里被打断过」。
  3. `!isStreaming`：复制 / fork / 朗读（不变）。
- 核心配合（`AgentRunner.swift:1430–1460`）：停下时整条消息 `stoppedByUser = true`；没有产出就不留统计（stats = nil）；被停止的不补「（没有输出）」。

#### ToolCard「已停止」（`Views/MessageRow.swift:565–573、591–592`）
- 着色：`event.stopped == true` → 灰色（优先于状态）；其余照旧（待确认橙、运行中蓝 / 嵌套待确认橙、成功绿、失败 / 拒绝红）。
- 状态徽标：`.failed where stopped` → `Label("已停止", systemImage: "stop.circle")` 二级色（不再是红色「失败」）。
- 核心 `MessageBlock.markStopped`（`Models/ChatModels.swift:158–174`）：还在跑 / 排队 / 等确认的调用（递归进子 Agent 运行记录）→ `status = failed`、`stopped = true`、`queued = nil`，结果文字「已停止」或「等待确认时被停止，没有执行」；被停的子 Agent 的结果是「被用户停止了，没做完。」+ 做到一半的结果（`AgentLiveness.swift:21–25`）。卡片上的审批面板因此消失。

### 1.2 v1.6 工作中补充（76534ff，提交 9536c33、1b760a4）

#### ComposerView（`Views/ComposerView.swift`）
- `hasContent`（:39–41）= 正文去空白后非空 || 有附件。
- `canSend`（:43–48）：`guard hasContent`；`runner.isGenerating`（任何对话在生成）时 = `editing == nil`（「重新输入」要等空闲）；否则 = `runner.canSend(in:)`。
- `isWorkingHere`（:50–51）= `runner.generatingConversationID == conversationID`。
- 动作区（:355–377）：
  - 生成中：HStack(spacing 8){ 停止键；`if canSend` 排队键 }。排队键：`arrow.up`（14pt bold、白色），32×32，强调色玻璃（同发送键），提示 `isWorkingHere ? "补充给 Agent：做完当前这一步就交给它 (Return)" : "排队：另一段对话结束后发出 (Return)"`，点了走 `onSend`。
  - 不在生成：发送键（:404–416，不变：`arrow.up` / 重新输入时 `arrow.triangle.branch`，`!canSend` 禁用，提示「发送 (Return)」/「从这里分叉并发送 (Return)」）。
  - Return 键：`onSend: { if canSend { onSend() } }`（:81）→ 生成中按 Return 就是排队。
- 占位文字（:418–429，按顺序）：重新输入「修改后发送，会从这条消息处分叉…」→ `isWorkingHere`「Agent 正在工作，可以继续补充——做完当前这一步就交给它…」→ `isGenerating`「另一段对话正在生成，现在发出的消息会排队…」→ 在线「给 Friday 发消息…  支持 Markdown，Shift+Return 换行」→ 本地按 phase（ready 同上 / failed「模型加载失败，请在下方重新选择模型」/ missing「在下方选择模型后开始对话」/ 其余「模型准备中…」）。
- 「下一轮生效」（:150–158）：底栏 `Spacer` 之后、模型胶囊之前；`runner.pendingTurnChanges(in:)` 的 model 或 thinking 为真时显示 `Label("下一轮生效", systemImage: "clock.arrow.circlepath")`，caption2 medium、橙色、fixedSize；提示 = 「Agent 这一轮还在用原来的」+（model ? 「模型」）+（两者都有 ? 「和」）+（thinking ? 「思考强度」）+「，下一轮自动换成现在选的」，即三种：「Agent 这一轮还在用原来的模型，下一轮自动换成现在选的」/「…原来的思考强度，…」/「…原来的模型和思考强度，…」。`pendingTurnChanges` 只在**这段**对话在生成、且当前大脑 / 思考强度与这一轮开始时定下的不同时为真（`AgentRunner.swift:1304–1308`）。
- 底栏顺序：附件、权限、工作目录、Spacer、[下一轮生效]、模型、深度思考。

#### ChatView（`Views/ChatView.swift`）
- 发送（:80–86）：`runner.submit(draft, attachments:, in:, replacing: editing)`，然后清空草稿、附件、editing。
- 空状态（:63–65）：建议按钮可用 = `runner.canSend(in:) || runner.isGenerating`；点建议 → `runner.submit(suggestion, in:)`（生成中点也会排队）。
- 核心 `submit`（`AgentRunner.swift:654–670`）：空闲、能发、这段没有排队 → 直接 `send`；`replacing != nil` 且忙 → 丢弃（界面不会让它发生）；否则追加 `QueuedInput`、解除暂停、空闲时安排投递、这段在生成就叫醒主 Agent。

#### QueuedBubbles（`Views/ChatView.swift:131–198`；插在 MessageList 的 VStack 最后 :317–318）
- 位置：消息列表内容的最后（在最后一条消息下面，同列表行距 18），随列表滚动；**只有会话非空**（MessageList 显示时）才有——空会话里排队的消息不显示（Swift 原样）。
- 整块 VStack(alignment: .trailing, spacing 8)，`frame(maxWidth: .infinity, alignment: .trailing)`。
- 每条（像用户消息一样靠右，左边至少留 80）：VStack(trailing, 6){
  - 有附件：`Label(附件文件名用「、」连接, systemImage: "paperclip")`，caption、二级色、单行、中间截断；
  - 有正文：`MarkdownText`，内边距 h16 v11，底色强调色 8%、圆角 20，**虚线**描边强调色 45%、1pt、dash [5, 4]（正式用户气泡是强调色 22% 实底）；
  - `Button("撤回", systemImage: "arrow.uturn.backward")`，borderless、caption、三级色，提示「撤回这条，不交给 Agent」→ `runner.removeQueued(id, in:)`。}
- 状态行 HStack(6)，caption medium，暂停时橙色否则二级色：图标 `pause.circle`（暂停）/ `clock.arrow.circlepath`；文字（:191–197，按顺序）：暂停「已暂停：上一轮被你停止了」→ 这段在生成「排队中：Agent 做完当前这一步就会看到」→ 别处在生成「排队中：另一段对话结束后发出」→ 不能发「排队中：模型就绪后发出」→ 否则「马上发出…」；暂停时后面加 `Button("发送")`（glass、small）→ `runner.resumeQueue(in:)`。
- 暂停的来源（`AgentRunner.swift:1472–1475`）：被用户停止的一轮结束时这段还有排队 → 进 `pausedQueues`。

#### 插入后的界面（核心 `takeInterjection`，`AgentRunner.swift:687–735`）
做完当前这一步（工具调用全部执行完、发起下一段请求之前）：当前助手消息收尾（统计定稿）→ 先接后台子 Agent 的汇报卡片（fromAgent）→ 每条排队消息各成一条用户消息（`interjected = true`）→ 新开一条助手消息接着写。没有下一步就在结束后把几条一起作为下一轮发出（普通用户消息，不标 interjected，一个回复回应）。

#### MessageRow「工作中补充」（`Views/MessageRow.swift:74–79`）
用户气泡 VStack 的最上面（附件之前）：`interjected == true` → `Label("工作中补充", systemImage: "text.bubble")`，caption、二级色，提示「Agent 工作时发的：做完当时那一步就交给了它，它接着往下做」。复制 / 重新输入不变（重新输入生成中仍禁用）。

#### ModelStatusView（`Views/ModelStatusView.swift:51–52`）
去掉 `.disabled(runner.isGenerating)`：生成中也能换模型（这一轮照旧，下一轮生效；新的本地模型等这一轮结束再加载）。深度思考胶囊本来就没禁用。

### 1.3 v1.7 父子 Agent 协作（e3aa578，提交 7f53e44、33aff41）

#### 汇报卡片（`Views/MessageRow.swift:34、42–68`）
- `role == .user && fromAgent != nil` → `agentNotice`（不是用户说的话）：靠左，HStack(top, 12){ 30×30 圆（`.fill.tertiary`）里 `person.2.badge.gearshape`（12pt semibold、二级色）；VStack(leading, 6){ `Text("子 Agent「\(fromAgent)」\(title)")` caption semibold 二级色；正文非空时 `MarkdownText(body)` } 内边距 12、`frame(maxWidth: 620, alignment: .leading)`（卡片宽 = min(可用宽, 620)）、`.fill.quaternary` 圆角 14 }。没有复制 / 重新输入按钮；重新输入时同样半透明。
- `text = message.plainText`，按第一个换行拆：第一行给 `AgentRunner.noticeTitle`，其余是正文。`noticeTitle`（`AgentTeamControl.swift:467–472`）：含「中途汇报」→「中途汇报」；含「完成了]」→「完成」；含「失败了]」→「失败」；否则「已停止」。第一行的原文（`noticeText` :450–458）形如「[子 Agent「X」（id：1a2b3c4d）中途汇报，它还在继续]」「…完成了]」「…失败了]」「[子 Agent「X」（id：…）被用户停止了，没做完。]」。

#### AgentControls（v1.7 形态，e3aa578 的 `MessageRow.swift`）
每张运行中的 `delegate_task` 卡片下面：排队消息列表（可撤回）+ 输入框 + 发送 + 停止。v1.8 起卡片里**不放输入框**（`showsInput: false`），输入框只在 Agent 树的详情里——最终形态见 1.4。

#### 运行记录里新出现的卡片（核心产生，界面照常用 ToolCard 画）
- 子 Agent 收到的消息：工具名 `agent_message`，标题「用户补充」或「\(from)追加的指示」（from 如「主 Agent」「父 Agent「X」」，期限提醒是「Friday」），图标 `text.bubble`，成功，结果「已交给子 Agent」，参数 = 消息原文（`AgentTeamControl.swift:426–433`）。
- 新工具的卡片：「给子 Agent 追加指示」`text.bubble`、「停止子 Agent」`stop.circle`、「等子 Agent」`hourglass`、「向父级汇报」`arrow.up.message`（`AgentProfiles.swift:134–145`）。
- 并行 / 后台分组卡片的 agentName「并行 ×N」/「后台 ×N」（标题「并行派发 · 后台 ×3」），后台组全部结束时结果「后台子 Agent 全部结束」（+「（有的被停止了）」）。

### 1.4 v1.8 Agent 树、用量、深层卡片（f1f6d58；提交 95d4df0、a3b7284、2084be2、9096118、e672b61——下面是最终形态 e672b61）

#### ToolCard（`Views/MessageRow.swift:435–646`）
- 层数：环境值 `agentNesting`（:885–888）主 Agent 消息里 0，每进一层子 Agent 运行记录 +1；并行派发卡片不算一层（`deepens = event.name != parallel`，:447–448）。`compact = nesting >= 2`（第 3 层子 Agent 起）→ 内边距 8（否则 12，:550）。
- 标题行（:477–481）：HStack(6){ 标题（callout semibold）；有 `event.usage` 时 `ModelPill(usage)` }。
- 运行记录区（:503–531，仅当 transcript 非空）：
  1. `expanded || pendingApproval` → 完整 `SubAgentTranscript(blocks, isLive: status == .running, deepens:)`（等确认的不折叠）；
  2. 否则并行派发 → `ChildAgentChips(blocks:)`；
  3. 否则运行中 → HStack(14){ `Label("已经 \(transcript.count) 步，展开看过程", systemImage: "ellipsis")` → expanded = true（snappy）；`Label("在 Agent 树里看", systemImage: "point.3.filled.connected.trianglepath.dotted")` → `runner.team.treeFocus = event.id` }，plain、caption、二级色；
  4. 否则（结束的子 Agent、收起）→ 什么都不显示。
  旧的「运行中只显示最近 4 步」（`liveTailCount`）删除。
- 运行中的子 Agent（:533–536）：`transcript != nil && name == delegate_task && team.running.contains(id)` → `AgentControls(eventID, name: agentName ?? "子 Agent", showsInput: false)`。
- 宽度上限照旧 620 / 有运行记录 760；展开后的「参数」「结果 / 子 Agent 的结论」不变。

#### SubAgentTranscript（`Views/MessageRow.swift:648–693`）
里面的块 nesting = `deepens ? nesting + 1 : nesting`；左内边距 `nesting >= 1 ? 6 : 12`（这里的 nesting 是它所在卡片的层数）；左侧 2pt 强调色 35% 竖线不变。`hiddenCount / onShowAll` 还在但 ToolCard 不再用。

#### AgentControls（最终，`Views/MessageRow.swift:223–316`）
- VStack(leading, 6)：
  - HStack(8)：`team.activity[id] != nil` 时按秒刷新的 `Label(agentStatusText(id, now), systemImage: "waveform.path.ecg")`，caption、二级色、单行；`!showsInput` 时 `Spacer(4)` + 停止键（在行尾）。
  - `team.queues[id]` 每条：HStack(6){ `clock.arrow.circlepath`；正文（最多 2 行、主色）；`from` 非空时「· \(from)」三级色；Spacer(8)；`Button("撤回", systemImage: "arrow.uturn.backward")` borderless，提示「撤回这条，不交给它」→ `removeAgentQueued` }，caption、二级色，内边距 h8 v5，虚线框强调色 45%、1pt、dash [4, 3]、圆角 8。
  - `showsInput`：HStack(8){ `TextField("给「\(name)」补充…（它做完当前一步就看到）")` roundedBorder、callout、回车发送；`arrow.up.circle.fill`（title3）borderless，空白时禁用，提示「发给这个子 Agent」；停止键 }。
- 停止键：本地 `@State stopping`；点了 `stopping = true; runner.stopAgent(id)`；`stopping` → `SpinningRefreshIcon(size: 13)` 否则 `stop.circle`（title3、红色）；`.disabled(stopping)`；提示 `stopping ? "正在停止…" : "只停止这个子 Agent，主 Agent 和其他子 Agent 照常"`。
- 发送：`runner.sendToAgent(id, text:)` 返回真才清空（不在运行或空白返回假）。
- 状态行文字（核心 `agentStatusText`，`AgentLiveness.swift:62–91`），各段用「 · 」连接：在做什么（「在思考 / 写」「在执行「<工具标题>」」「在等它的下级」「在等用户确认」「在排队」）；完整版才有「<时长>前有动静」；「第 N 步」；完整版「已运行 <时长>」/ compact 版只有「<时长>」；完整版有期限时「期限还剩 <时长>」/「已过期限」。时长：「N 秒」「M 分 S 秒」「H 小时 M 分」。没有活动（未注册）返回 nil。

#### ChildAgentChips + FlowLayout（`Views/MessageRow.swift:890–960`）
并行派发卡片收起时：一排可换行的小标签（间距 6），`AgentTree.build(transcript)` 的顶层节点（嵌套的并行组摊平）；每个是按钮 → `team.treeFocus = node.id`；内容 HStack(4){ `AgentStatusIcon`（caption2，12×12）；名字（caption medium、单行）；有 usage 时模型名（caption2、三级色、单行）}，内边距 h8 v4，胶囊底主色 6%；提示「在 Agent 树里看「\(name)」」。

#### AgentTreePanel.swift（新，863 行）

**数据（:7–174）**
- `AgentNode`（:7–19）：event、depth（主 Agent 直接派的是 1）、parallel（同一次并行派发出来的）、children；id = event.id；name = agentName ?? 「子 Agent」；isRunning = status == .running。
- `build(blocks, depth = 1, parallel = false)`（:30–44）：`delegate_task` 且 transcript != nil → 节点，children = build(transcript, depth + 1)；`run_parallel_tasks` → build(transcript, depth, parallel: true)（不加层）；其余忽略。**派发被拒的（没有运行记录）不算节点**。
- `turns(messages)`（:46–57）：每条「用户自己发的」消息（不是 interjected、不是 fromAgent）开新的一轮，request = 它的 plainText；之后的助手消息的节点都加进这一轮；**没有节点的轮次丢掉**；新的在前。
- `messages(conversationID)`（:59–69）：当前分支路径 + 正在生成的那条。
- `stats`（:85–102）：running（running + awaitingApproval）、done（succeeded）、stopped（failed && stopped）、failed（其余 failed / denied）、deepest、total。
- `usage(of:)`（:123–136）：生成 = usage.generatedTokens（+ 运行中这一段的估算 `team.passTokens`），输入 = usage.promptTokens；速度 = 运行中 `team.speed(id)`（最近 3 秒窗口），结束后 `usage.tokensPerSecond`（平均）；agents 1，running 0/1。
- `conversationUsage`（:138–158）：主 Agent = 各助手消息 `stats.tokenCount / promptTokenCount` 之和，速度 `runner.mainLiveSpeed`，running = 这段在生成 ? 1 : 0；子 Agent = 全部节点之和（结束的速度记 0）。合计 = 主 + 子。
- `compact(n)`（:160–167）：≥1,000,000 → `%.1fM`；≥1000 → `%.1fk`；否则原数。`speedText`（:169–173）：≤0 → 「— tok/s」；≥100 → `%.0f tok/s`；否则 `%.1f tok/s`。

**AgentTreeButton（:176–219）**——对话工具栏（`.primaryAction`，在用量徽标右边）
- 统计取**最新一个有节点的轮次**（`turns(...).first`）。
- 标签：HStack(4){ `point.3.filled.connected.trianglepath.dotted`；running > 0 时等宽数字的个数（数字放图标右边，不用角标）}；无障碍名「Agent 树」。
- 提示：total == 0「Agent 树：这段对话派出的子 Agent」，否则「Agent 树：运行中 \(running) 个，最深 \(deepest) 层」。点了切换面板。
- 自动打开（:208–214）：`deepest >= 3 && running > 0` **由假变真**时，本视图实例还没自动开过、面板关着 → 打开一次（之后用户关了不再打扰；切换对话 ChatView 重建后又能自动开一次）。
- `team.treeFocus` 变成非空 → 打开面板（:215–217）。

**ModelPill（:221–238）**：模型名，caption2 medium、单行、中间截断、二级色，内边距 h6 v1.5，胶囊底主色 7%；提示 `usage.detail` =「深度思考：<关闭 / 低 / 中 / 高 / 不支持>」「温度：<0–2 位小数>」[「服务：<在线服务名>」] 换行连接（`ChatModels.swift:101–105`）。

**ConversationUsageBadge（:240–299）**——对话工具栏
- 只在这段对话在生成时按秒刷新，否则随存档变化。
- 显示条件：合计 generated > 0 || running > 0。内容 HStack(5)：running > 0 时 `bolt.fill`（强调色）+ speedText + 「·」（三级色）；「生成 \(compact)」「·」「输入 \(compact)」；caption medium、等宽数字、二级色，左右内边距 6。
- 提示（多行）：「这段对话全部 Agent 的合计」；running > 0：「实时速度：<speedText>（正在生成的 Agent 相加）」；「生成 <千分位> tokens · 输入 <千分位> tokens」；「主 Agent：生成 <compact> · 输入 <compact>」+（主在跑：「 · 现在 <speedText>」）；子 Agent 数 > 0：「子 Agent N 个」+（「（运行中 R）」）+「：生成 <compact> · 输入 <compact>」+（「 · 现在 <speedText>」）；最后「输入：在线引擎是各段请求的输入合计（工具调用后的每段都带上完整上下文），本地模型是新处理的提示词」。

**AgentTreePanel（:301–407）**
- 视图状态：selected（节点 id）、collapsed（收起的节点，默认都展开）、expandedTurns（更早的轮次默认收起）。
- 选中且能在树里找到 → `AgentDetailView`（`.id(selected)`：换节点整页重建、滚回顶部）；树为空 → `ContentUnavailableView("还没有子 Agent", systemImage: "point.3.filled.connected.trianglepath.dotted", description: "主 Agent 派出子 Agent 后，整棵树会显示在这里：谁派了谁、用什么模型、在做什么、用了多少 token。")`；否则 ScrollView{ LazyVStack(leading, 16){ PanelSummary；各轮 }.padding(12) }。找不到选中的节点时退回列表。
- 整块铺满、顶对齐，底色主色 2.5%。出现时和 `team.treeFocus` 变化时「接住焦点」：selected = focus，再把 treeFocus 清空。
- 轮次（:350–398）：VStack(leading, 6)；标题是按钮（最新一轮不响应）：更早的轮次前有 `chevron.right`（caption2 bold，展开时转 90°）；「本轮」（最新）/「第 N 轮」（N = 轮数 − 下标，**只数有节点的轮次**），caption semibold；请求（换行换成空格）caption 单行二级色；Spacer(6)；「\(total) 个 · \(deepest) 层」caption2 三级色等宽数字；整行二级色；提示 = 请求原文。展开时下面是摊平的行（行距 0）：`TreeRow`，节点的展开箭头切换 collapsed（snappy），点卡片 → selected。

**PanelSummary（:409–456）**：内边距 12，圆角 12 主色 4.5% 底。
- 第一行 HStack(6)：`point.3.filled.connected.trianglepath.dotted`（强调色）、「Agent 树」（headline）、Spacer、「\(total) 个 · 最深 \(deepest) 层」（caption 二级色等宽）。
- 第二行 HStack(6)：StatusChip「运行中」(蓝) 、「完成」(绿) 总是显示；「停止」(灰) 、「失败」(红) 只在 > 0 时显示。StatusChip（:458–475）：HStack(4){ 6pt 圆点（该色）；标题；数字 semibold 等宽 } caption，内边距 h8 v3，胶囊底该色 `count > 0 ? 14% : 6%`，文字 `count > 0 ? 主色 : 二级色`。
- 第三行（生成中按秒刷新）HStack(12)：Metric(`bolt.fill`, running > 0 ? speedText : 「—」, 「速度」)、Metric(`arrow.up.forward`, compact(generated), 「生成」)、Metric(`arrow.down.backward`, compact(prompt), 「输入」)；Metric（:477–489）= HStack(4){ 图标 caption2 强调色；值 callout semibold 等宽；标签 caption2 二级色 }。提示「这段对话全部 Agent（主 Agent + 子 Agent）的合计。主 Agent：生成 X、输入 Y；子 Agent：生成 X、输入 Y」（compact）。

**摊平与连接线（:491–574）**
- `flatten`（:503–515）：每行记 level（轮次内 0 起）、guides（`guides[k]` = 第 k 层的那个上级后面还有兄弟）、isLast；收起的节点不展开子行。
- TreeRow（:517–536）：NodeCard 左内边距 level × 14，上下各 2；背景左侧 level × 14 宽画连接线。
- TreeGuides（:538–574）：每列宽 14，列中心 x = 列 × 14 + 7；列 < level − 1：`guides[列 + 1]` 为真时整行高的竖线；最后一列：从顶到 elbow − 4（elbow = 19，对齐卡片第一行中线），二次曲线拐到 (x + 4, elbow)，横线到 level × 14；不是最后一个兄弟时竖线从 elbow − 4 继续到底。线宽 1，二级色 40%。level 0 不画。

**NodeCard（:576–686）**：HStack(top, 6)
- 展开箭头位（12×16）：有子节点时 `chevron.right`（caption2 bold 二级色，展开时转 90°），提示「收起它的下级」/「展开它的下级（\(n) 个）」；没有时留空。
- `AgentStatusIcon`（14×16）。
- VStack(3)：HStack(5){ 名字（callout semibold 单行）；ModelPill；Spacer(4)；parallel 时「并行」（caption2 二级色，内边距 h5，胶囊描边二级色 40%）；有子节点时 `Label("\(n)", systemImage: "person.2")`（caption2 三级色）}；第二行（运行中按秒刷新）：文字 = 运行中 `agentStatusText(id, compact: true) ?? "运行中"`，结束后结论第一行（去掉开头的「结论：」），caption 二级色单行；第三行 HStack(8){ `bolt.fill`「<speedText>」（结束后「平均 <speedText>」）；`arrow.up.forward` compact(生成)；`arrow.down.backward` compact(输入) }，图标强调色、图文间距 2（CompactLabelStyle :688–696），caption2 二级色等宽，提示「速度 · 生成 · 输入（tokens）」。
- 内边距 h8 v7，圆角 9 底色：等确认（running 且嵌套待确认 / awaitingApproval）橙 14%；运行中强调色 8%；停止灰 10%；失败 / 拒绝红 8%；成功主色 4%；悬停时不透明度 × 1.6。运行中加 1pt 强调色 35% 描边。整卡点按 → 打开详情；提示「查看「\(name)」的任务、结论和过程」。

**AgentStatusIcon（:698–718）**：等确认 → `hand.raised.fill` 橙；排队 → `hourglass` 二级色；运行中 → 迷你转圈；成功 → `checkmark.circle.fill` 绿；停止 → `stop.circle.fill` 二级色；失败 / 拒绝 → `xmark.octagon.fill` 红。

**AgentDetailView（:720–863）**：ScrollView{ VStack(leading, 12) 内边距 12 }
1. `Button(Label("Agent 树", systemImage: "chevron.left"))` borderless → 回列表。
2. 头卡（内边距 12，圆角 12 主色 4.5%）：HStack(8){ AgentStatusIcon（title3）；名字 title3 semibold 单行；ModelPill；Spacer(4)；「第 \(depth) 层」caption medium 二级色，内边距 h7 v2，胶囊主色 7% }；横向滚动（无滚动条）的上级链：「主 Agent」（二级色）→ 每个祖先 `chevron.right`（caption2 三级色）+ link 按钮（点了跳到那一级）→ 最后 `chevron.right` + 自己的名字，caption；有 usage 时一行 `usage.detail` 把换行换成「 · 」，caption2 三级色。
3. 指标（运行中按秒刷新）HStack(8) 四块等宽：「速度」/（结束后）「平均速度」= speedText；「生成」= compact；「输入」= compact；「工具调用」= 它自己运行记录顶层的工具卡片数（不含 `agent_message`）。块：值 callout semibold 等宽单行（最小缩到 0.7）、标签 caption2 二级色，内边距 h8 v6，圆角 8 主色 4.5%。
4. 运行中：`AgentControls(eventID, name)`（**带输入框**）。
5. 卡片（标题 caption semibold 二级色，内边距 10，圆角 10 主色 3%）：「任务」= 参数里的 task（解析不了就原文），可选中；结束且结果非空：「结论」（成功）/「结果」，可选中；有下级：「它派出的下级（\(n)）」= 子节点的 NodeCard（展开箭头无效），点了跳过去；运行记录非空：「过程」= `SubAgentTranscript(isLive: 运行中)`，环境 nesting 设为 2（里面的下级卡片按深层的样子收起，要看就在上面点进去）；等确认的调用在这里同样能允许 / 拒绝（ToolCard 的审批面板）。

#### ChatView（`Views/ChatView.swift:16–47、96–127`）
- `@AppStorage("agentTreeVisible")`（默认 false）、`@AppStorage("agentTreeWidth")`（默认 320）——全局，记住上次。
- body：HStack(spacing 0){ 对话区；开着时 `PanelResizeHandle` + `AgentTreePanel.frame(width:)`（`.move(edge: .trailing)` + 透明度转场）}，`.animation(.snappy, value: agentTreeVisible)`。注释：不用系统检查器（检查器分栏和输入框底栏来回约束，AppKit 抛异常退出）。
- 工具栏（`.primaryAction`）：`ConversationUsageBadge`，然后 `AgentTreeButton`（最右）。空会话也有这两项（徽标没用量时隐藏）。
- PanelResizeHandle（:96–127）：1pt 分隔线色竖线，左右各 3 的命中区；悬停左右拉伸光标；拖动 width = clamp(起始宽 − 位移, 260…560)；提示「拖动调整 Agent 树的宽度」。

#### ContentView（`Views/ContentView.swift:12–15、56–78`）
自动收起侧边栏的阈值加上面板宽：窗口宽 < 840 + 面板宽（开着时）→ 收起；> 900 + 面板宽且是我们收起的 → 展开；面板开关变化时也重算（宽度变化不单独触发）。

#### 设置
- 设置 › 子 Agent（`Views/AgentsSettingsView.swift:31–43`）：在「限制每个子 Agent 的工具调用次数」及其 Stepper 之后加 `Toggle("限制每轮派发的子 Agent 总数")`（开 → 100，关 → 0），提示「所有层加起来。默认 100 个，只防失控：到了就不再派，用已有结果收尾」；开着时 `Stepper("最多 \(n) 个", 10…1000, step 10)`。说明文字换成：「何时派发、派几个由主 Agent 按任务需要自己判断：能拆成互不依赖的部分就并行派多个，需要多步工具操作或独立复核就派一个，简单问答自己做。子 Agent 全部是临时的：主 Agent 现场写角色，运行完即销毁，只在对话里留下嵌套的运行记录；需要确认的操作仍会弹出审批。层数不限：子 Agent 自己也能按同样的标准再往下派（串里可以并、并里可以串），但每一层都必须把任务拆小，原样往下传会被拒绝。派发数量和并发都不设上限：本地模型按剩余内存决定同时跑几个，在线引擎从起步并发开始、没被限流就往上加、遇到 429 减半，其余排队；子 Agent 等下级时不占名额。」（不再插值层数上限）。
- EngineEditor（`Views/EngineEditor.swift:3、23–24`）：Stepper 文案「起步并发 \(n)（不设上限：没被限流就往上加，遇到 429 减半）」（1…64）。
- 密钥（`Views/SecretsSettingsView.swift:160`）：「模型」按钮提示「拉取模型列表，勾选哪些模型可以用；也可改每个模型的起步并发等」。
- ServiceModelPicker（`Views/ServiceModelPicker.swift:102、108、283`）：行尾「起步并发 \(n)」+（看图「 · 看图」）+（无工具「 · 无工具」）；「高级」提示「改起步并发、是否支持看图 / 工具调用等」；ServiceModelsSheet 说明「打开时会自动拉取列表，新出现的模型默认勾上；勾上 = 可以用（保存后成为一个可选的大脑），去掉勾 = 保存后移除。「高级」改起步并发、是否支持看图 / 工具调用等；列表不全时手动添加。」
- 模型菜单（`Views/ModelStatusView.swift:157`）：在线模型副标题「能看图 / 文本 · [无工具调用 ·] 起步并发 \(n)」。

### 1.5 v1.9 菜单栏（e2f8f41，提交 5103122）

- MenuBarLabel（`App/FridayApp.swift:223–265`，MenuBarExtra 的 label :153–155）：图标右边常驻两行小字，第一行「\(agents) AI」，第二行速度；`HStack(spacing 3){ MenuBarIcon；VStack(leading, spacing −1){ 两行 } 9pt medium 等宽数字、单行、宽 44 }`，高 22，用 ImageRenderer 画成模板图（随菜单栏深浅色、数字变化不左右跳）。没有在工作的是「0 AI」「0 t/s」。
- `speedText`（:234–243）：≤0「0 t/s」；≥1e12 / 1e9 / 1e6 / 1e3 → 数值 ≥100 用 `%.0f` 否则 `%.1f` + 「 Tt/s / Gt/s / Mt/s / Kt/s」；否则 ≥10「%.0f t/s」、<10「%.1f t/s」。例：0 t/s、8.5 t/s、179 t/s、1.2 Kt/s、3.4 Mt/s。（README 写「K / M / T」，代码还有 G。）
- MenuBarMenu（:267–298）：状态行（「Friday 正在生成…」或 models.statusText）之后，`liveAgents > 0` 时多一行「\(n) AI 在工作 · \(speedText)」；其余（停止生成、—、打开 Friday、新对话、—、设置…、—、退出 Friday）不变。
- 数据（`AgentRunner.swift:11–15、1378–1400`）：`liveAgents` = (主 Agent 在生成 ? 1 : 0) + 运行中的子 Agent 数（全局，跨对话）；`liveSpeed` = 主 Agent 3 秒窗口速度 + 运行中子 Agent 速度之和，四舍五入到 0.1；有 Agent 在工作时每秒算一次、**值变了才赋值**，都停下归零。

---

## 2. Qt 映射

### 2.1 文件清单

| 动作 | 文件 | 对应 Swift |
|---|---|---|
| 新 | `qml/chat/SpinningRefreshIcon.qml` | Views/SpinningRefreshIcon.swift |
| 新 | `qml/chat/DashedOutline.qml` | QueuedBubbles / AgentControls 的 `strokeBorder(dash:)` |
| 新 | `qml/chat/QueuedBubbles.qml` | ChatView.swift QueuedBubbles |
| 新 | `qml/chat/PanelResizeHandle.qml` | ChatView.swift PanelResizeHandle |
| 新 | `qml/chat/AgentControls.qml`、`ChildAgentChips.qml` | MessageRow.swift AgentControls / ChildAgentChips + FlowLayout |
| 新 | `qml/chat/ModelPill.qml`、`AgentStatusIcon.qml`、`AgentTreeButton.qml`、`ConversationUsageBadge.qml`、`AgentTreePanel.qml`、`AgentPanelSummary.qml`、`AgentTreeRow.qml`、`AgentNodeCard.qml`、`AgentDetailView.qml` | Views/AgentTreePanel.swift 的同名 struct（PanelSummary / TreeRow+TreeGuides / NodeCard 加 Agent 前缀；QML 类型名全模块唯一，已查无重名） |
| 新 | `viewmodels/AgentTreeModel.{h,cpp}` | AgentTree（build / turns / stats / flatten / usage）+ 各视图的数据 |
| 新 | `viewmodels/UiState.{h,cpp}` | `@AppStorage("agentTreeVisible" / "agentTreeWidth")` |
| 改 | `qml/chat/ComposerView.qml`、`ChatView.qml`、`MessageList.qml`、`MessageRow.qml`、`ToolCard.qml`、`SubAgentTranscript.qml`、`BlockView.qml`、`ModelStatusView.qml`、`EmptyStateView`（经 ChatView） | 同名 Swift |
| 改 | `qml/ContentView.qml` | ContentView.adjustSidebar |
| 改 | `qml/settings/AgentsSettingsView.qml`、`EngineEditor.qml`、`SecretsSettingsView.qml`、`ServiceModelPicker.qml`、`ServiceModelsSheet.qml` | 同名 Swift |
| 改 | `viewmodels/ChatViewModel.{h,cpp}`、`MessageListModel.{h,cpp}`、`ModelMenuBuilder.cpp`、`Formatters.h` | — |
| 改 | `FridayApp.{h,cpp}`、`main.cpp`（注册 AgentTeam 单例）、`DebugHarness.{h,cpp}` | FridayApp.swift MenuBarLabel / MenuBarMenu |
| 改 | `style/icons/mapping.json` + 运行 `qt/tools/gen_icons.py`（提交生成物） | 新 SF Symbol |

每个新文件头照现有写法注明「对应 Swift：…」。

> 归属与位置**已由 PORT_V15_V19.md §5 取代**：对话里的组件（SpinningRefreshIcon、DashedOutline、QueuedBubbles、AgentControls、ChildAgentChips、ModelPill、AgentStatusIcon）在 `qml/chat/`，归 chat-ui；
> Agent 树的组件（AgentTreeColumn（新：分隔条 + 面板 + 宽度动画 + 自动打开 + 接住焦点）、AgentTreeButton、ConversationUsageBadge、AgentTreePanel、AgentPanelSummary、AgentTreeRow、
> AgentNodeCard、AgentDetailView、PanelResizeHandle）在 `qml/agenttree/`，归 tree-tray；设置页归 settings；`main.cpp` 注册 `AgentTeam` 单例、图标生成、跨包组件的占位由骨架做（PORT §6）。
> `ModelMenuBuilder.cpp` 归 settings，`Formatters.h` 归 tree-tray，`DebugHarness` 归 tree-tray。

### 2.2 v1.5

**SpinningRefreshIcon.qml**
```qml
// 对应 Swift：Views/SpinningRefreshIcon.swift
// 旋转的刷新图标（点了停止、正在停下时用）。RotationAnimator 在渲染线程转（== Swift 用 Core Animation，不让主线程每帧重排）
import QtQuick
import Friday.Style

Icon {
    id: root
    name: "arrow.triangle.2.circlepath"
    size: 14
    weight: Font.Bold
    color: Theme.secondaryLabel
    Accessible.role: Accessible.Graphic
    Accessible.name: "正在停止"
    RotationAnimator on rotation {
        from: 0; to: 360          // 正角度 = 顺时针（同 Swift）
        duration: 900
        loops: Animation.Infinite
        running: root.visible
    }
}
```
尺寸换算（Lucide 图标框比 SF 字形大一圈，按现有 stop.fill 12pt → 14px 的比例）：输入框 / 子 Agent 停止键用 16；消息底栏用 `Theme.iconSize("caption")`、`weight: Font.DemiBold`。

**ComposerView.qml 动作区**（现 :180–236 的 `action` Item）改成一行三个按钮，提示层仍放在按钮**外面**（禁用的按钮收不到悬停，现有注释 :231）：
- `stopButton`（objectName `stopButton`）：`visible: generating`；`enabled: !AgentRunner.isStopping`；`icon.source: AgentRunner.isStopping ? "" : Theme.iconSource("stop.fill", Font.Bold)`、`icon.color: Theme.red`（停止中不着红）；额外子项 `SpinningRefreshIcon { anchors.centerIn: parent; visible: AgentRunner.isStopping; size: 16 }`；外层 HelpArea 文字 `AgentRunner.isStopping ? "正在停止…" : "停止生成 (" + App.shortcutModifier + ".)"`。
- `queueButton`（v1.6，见 2.3）。
- `sendButton` 不变（`visible: !generating`）。
- 停止中整个按钮 `enabled: false`（== `.disabled` + `.allowsHitTesting(false)`）。`Ctrl+.` 的 Shortcut、☰ 菜单「停止生成」、托盘「停止生成」不用改（核心只认第一次）。

**MessageRow.qml**（现 :234–237、284–322）
- TypingIndicator：`visible: root.isStreaming && !AgentRunner.isStopping && (…原条件)`。
- footer：`visible: root.hasStats || (root.isStreaming && AgentRunner.isStopping) || (!root.isStreaming && (root.blockCount > 0 || root.stoppedByUser))`。
- footer 第一项：`Row { spacing: 6; visible: root.isStreaming && AgentRunner.isStopping; SpinningRefreshIcon { size: Theme.iconSize("caption"); weight: Font.DemiBold } StyledText { text: "正在停止…"; textStyle: "caption"; color: Theme.secondaryLabel } }`；StatsLabel 改 `visible: root.hasStats && !(root.isStreaming && AgentRunner.isStopping)`。
- 第二项：`Label { visible: !root.isStreaming && root.stoppedByUser; title: "已停止"; iconName: "stop.circle"; textStyle: "caption"; color: Theme.secondaryLabel; help: "这一轮是你点停止结束的，回答和操作可能不完整；接着发消息时 Agent 会知道这里被打断过" }`，在复制 / fork / 朗读之前。
- 新 required 角色 `stoppedByUser`（MessageListModel，§3.8）。

**ToolCard.qml**（现 :51–54、187–193）
- `tint: root.event.stopped === true ? Theme.gray : …原逻辑`。
- 原「失败」Label 加 `&& root.event.stopped !== true`；新增 `Label { visible: root.status === "failed" && root.event.stopped === true; title: "已停止"; iconName: "stop.circle"; color: Theme.secondaryLabel }`。
- 需要 `ToolEvent::toVariant` 带 `stopped`（§3.1）。

### 2.3 v1.6

**ChatViewModel**（§3.8 有声明）
- `canSend` 语义改成 == Swift 的 `runner.canSend(in:)`（模型就绪 / 有在线引擎），**不再** `&& !isGenerating`；`suggestionsEnabled = runner.canSend(id) || runner.isGenerating`（现 :143–144）。
- `placeholderFor` 先判断 `isStreamingHere`（「Agent 正在工作，可以继续补充——做完当前这一步就交给它…」），再 `isGenerating`（「另一段对话正在生成，现在发出的消息会排队…」），然后原逻辑（现 :25–36、186）。
- 新属性：`isStopping`、`pendingModelChange / pendingThinkingChange / pendingChangeHelp`（三种文案见 1.2）、`queuedInputs / queuePaused / queueStatus`（五种文案见 1.2）；新方法 `submit / removeQueued / resumeQueue`。连接 `AgentRunner::stoppingChanged`、`queuedInputsChanged(conv)`（只认自己这段）；pending 跟着 `generatingChanged` + `conversationSettingsChanged` 重算（现已连接）。

**ComposerView.qml**
- `readonly property bool hasContent: text.trim().length > 0 || attachments.length > 0`；`canSend: vm !== null && hasContent && (vm.isGenerating ? !isEditing : vm.canSend)`（现 :32–33）。Return 走 `onSendRequested: if (root.canSend) root.sendRequested()`（不变）。
- `queueButton`（objectName `queueButton`）：`QQC.RoundButton { visible: generating && root.canSend; highlighted: true; icon arrow.up（Font.Bold，16）；adoptIconColor；onClicked: root.sendRequested() }`，外层 HelpArea `vm.isStreamingHere ? "补充给 Agent：做完当前这一步就交给它 (Return)" : "排队：另一段对话结束后发出 (Return)"`。与停止键间距 8，动作区宽 32 或 72。
- 「下一轮生效」：`Label { id: pendingLabel; visible: vm.pendingModelChange || vm.pendingThinkingChange; title: "下一轮生效"; iconName: "clock.arrow.circlepath"; textStyle: "caption2"; weight: Font.Medium; color: Theme.orangeText; iconColor: Theme.orange; help: vm.pendingChangeHelp }`，锚在 `modelChip` 左边、间距 6；`bottomBar.fixedWidth`（现 :251–252）可见时加上它的宽 + 6（模型名仍是唯一被压缩的）。
- 占位文字照旧取 `vm.placeholderText`。

**ChatView.qml**：`send()` → `chatModel.submit(root.draft, root.attachments, root.editingID)` 再清空（现 :42–48）；EmptyStateView `onPicked: (text) => chatModel.submit(text, [], "")`（现 :92）；`MessageList { chat: chatModel … }`。

**ModelStatusView.qml**（现 :59、65–70）：`enabled: root.vm !== null`（去掉 `!isGenerating`）；禁用时的 HelpArea 留着无害（只在 vm 为空时生效）或删掉。核心要保证生成中改本地模型只记下、这一轮结束才加载（进程内推理的情形尤其要验，见 §5）。

**QueuedBubbles.qml**（新）——MessageList 里 ListView 的 `footer`（随列表滚动、算进 contentHeight，跟随到底 / 「有新内容」逻辑自动生效）：
- `property ChatViewModel vm`；`visible/height`：没有排队时高 0。ListView 的 `spacing` 一般不加在最后一行和 footer 之间（实现时截图确认），所以 footer 内部自己留上边距 18（== VStack 行距）。宽 = `list.columnWidth`。
- Repeater(`vm.queuedInputs`) 每条：右对齐的 Column(spacing 6)，最大宽 = 列宽 − 80：
  - `Label { title: modelData.attachmentNames; iconName: "paperclip"; textStyle: "caption"; color: Theme.secondaryLabel; elide: Text.ElideMiddle }`（有附件时）；
  - 气泡：`Rectangle { radius: Theme.radius.bubble; color: Theme.opacity(Theme.accent, 0.08) }` + `DashedOutline { color: Theme.opacity(Theme.accent, 0.45); dash: [5, 4]; radius: Theme.radius.bubble }` + `MarkdownText`（内边距 16 / 11，宽 = min(自然宽, 最大宽)，同 MessageRow 用户气泡的写法）；
  - `Button { buttonStyle: "borderless"; controlSize: "small"; text: "撤回"; iconName: "arrow.uturn.backward"; textStyle: "caption"; foregroundColor: Theme.tertiaryLabel; help: "撤回这条，不交给 Agent"; onClicked: vm.removeQueued(modelData.id) }`。
- 状态行（右对齐 Row，spacing 6）：`Icon { name: vm.queuePaused ? "pause.circle" : "clock.arrow.circlepath"; color: vm.queuePaused ? Theme.orange : Theme.secondaryLabel; size: Theme.iconSize("caption") }`、`StyledText { text: vm.queueStatus; textStyle: "caption"; weight: Font.Medium; color: vm.queuePaused ? Theme.orangeText : Theme.secondaryLabel }`、`Button { visible: vm.queuePaused; text: "发送"; buttonStyle: "bordered"; controlSize: "small"; onClicked: vm.resumeQueue() }`。

**DashedOutline.qml**（新）：照 `qml/settings/CustomServiceCard.qml:21–43` 的先例——`Shape`（几何渲染器 + `layer.samples: 4`），`ShapePath { strokeStyle: ShapePath.DashLine; dashPattern: [dash[0] / strokeWidth, dash[1] / strokeWidth]; fillColor: "transparent" }` + `PathRectangle`（描边画在内侧：偏移半个线宽）。属性 `color / strokeWidth(1) / dash / radius`，不接收鼠标。

**MessageRow.qml 用户气泡**（现 :115–171）：`bubbleColumn` 最上面加 `Label { anchors.right: parent.right; visible: root.interjected; title: "工作中补充"; iconName: "text.bubble"; textStyle: "caption"; color: Theme.secondaryLabel; help: "Agent 工作时发的：做完当时那一步就交给了它，它接着往下做" }`。新 required 角色 `interjected`。

### 2.4 v1.7

**MessageRow.qml 汇报卡片**：Loader（现 :98–102）`sourceComponent: root.isUser ? (root.fromAgent.length > 0 ? agentNotice : userBubble) : assistantBody`。`agentNotice` 组件：
- 头像：30×30 圆 `Theme.tertiaryFill`，`Icon { name: "person.2.badge.gearshape"; size: Theme.iconSize(12); weight: Font.DemiBold; color: Theme.secondaryLabel }`。
- 卡片：x = 42，宽 = min(行宽 − 42, 620)，`Rectangle { color: Theme.quaternaryFill; radius: Theme.radius.card }`，内边距 12，Column(spacing 6){ `StyledText { text: "子 Agent「" + root.fromAgent + "」" + root.agentNoticeTitle; textStyle: "caption"; weight: Font.DemiBold; color: Theme.secondaryLabel }`；`MarkdownText { visible: root.agentNoticeBody.length > 0; text: root.agentNoticeBody }` }。
- 新 required 角色 `fromAgent / agentNoticeTitle / agentNoticeBody`（标题、正文在 C++ 里拆好，`noticeTitle` 用核心的静态函数，§3.3）。

**AgentControls.qml**（新，最终形态；v1.7 的「卡片里也有输入框」不移植，直接做 v1.8）
- 属性：`eventID`、`name`、`showsInput`（默认 true）、`draft`、`stopping`（本地，同 Swift 的 @State）。
- 依赖：`AgentTeam.revision`（running / activity / queues 变化）、`AgentTeam.clock`（每秒）——绑定里让它们参与求值（项目惯例：qmlcachegen 会删掉没用到的读取）：
  `readonly property string statusText: AgentTeam.clock >= 0 && AgentTeam.revision >= 0 ? AgentRunner.agentStatusText(eventID, false) : ""`
- 第一行 RowLayout(spacing 8)：`Label { Layout.fillWidth: true; visible: root.statusText.length > 0; title: root.statusText; iconName: "waveform.path.ecg"; textStyle: "caption"; color: Theme.secondaryLabel; elide: Text.ElideRight }`；`!showsInput` 时填充 + 停止键。
- 排队条目：Repeater(`AgentTeam.revision >= 0 ? AgentTeam.queue(eventID) : []`)，每条 `Item` + `DashedOutline { color: Theme.opacity(Theme.accent, 0.45); dash: [4, 3]; radius: Theme.radius.medium }`，内容 RowLayout(spacing 6, 内边距 8 / 5)：`Icon clock.arrow.circlepath`（caption、二级色）、正文 `StyledText`（caption、`Theme.label`、换行、最多 2 行）、`StyledText { visible: from 非空; text: "· " + from; color: Theme.tertiaryLabel }`、填充、`Button { buttonStyle: "borderless"; controlSize: "small"; text: "撤回"; iconName: "arrow.uturn.backward"; textStyle: "caption"; help: "撤回这条，不交给它"; onClicked: AgentRunner.removeAgentQueued(modelData.id, eventID) }`。
- `showsInput` 行 RowLayout(spacing 8)：原生 `TextField { Layout.fillWidth: true; placeholderText: "给「" + name + "」补充…（它做完当前一步就看到）"; font.pixelSize: Theme.pixelSize("callout"); onAccepted: root.send() }`；`IconButton { iconName: "arrow.up.circle.fill"; iconSize: Theme.iconSize("title3"); foregroundColor: Theme.accent; enabled: root.draft.trim().length > 0; help: "发给这个子 Agent"; onClicked: root.send() }`；停止键。
- 停止键（包一层 Item，提示挂外层）：`IconButton { visible: !root.stopping; iconName: "stop.circle"; iconSize: Theme.iconSize("title3"); foregroundColor: Theme.red; onClicked: { root.stopping = true; AgentRunner.stopAgent(root.eventID) } }` + `SpinningRefreshIcon { visible: root.stopping; size: 16 }` + `HelpArea { text: root.stopping ? "正在停止…" : "只停止这个子 Agent，主 Agent 和其他子 Agent 照常" }`。
- `function send() { if (AgentRunner.sendToAgent(eventID, draft)) draft = "" }`。

### 2.5 v1.8 对话里的卡片

**BlockView.qml / SubAgentTranscript.qml / ToolCard.qml 增加 `nesting`**（== 环境值 agentNesting）
- BlockView：`property int nesting: 0` → ToolCard.nesting。MessageRow 里是 0。
- SubAgentTranscript：`property int nesting: 0`、`property bool deepens: true`；内容 x / 竖线后的缩进 = `nesting >= 1 ? 6 : 12`（现 :110 写死 12）；BlockView 的 `nesting: root.deepens ? root.nesting + 1 : root.nesting`。`tailCount / showAllRequested` 不再被 ToolCard 使用（可保留、默认 0）。
- ToolCard：`property int nesting: 0`；`readonly property bool deepens: event.name !== App.agentTeamParallelTool`；`Card.padding: nesting >= 2 ? 8 : 12`（现 :90）；删 `liveTailCount`（:29–30）。

**ToolCard.qml 运行记录区**（现 :227–263）
- transcriptLoader：`active: hasTranscript && transcriptCount > 0 && (expanded || hasPendingApproval)`；绑定 `isLive: status === "running"`、`tailCount: 0`、`nesting: root.nesting`、`deepens: root.deepens`。
- 并行收起：`ChildAgentChips { visible: hasTranscript && transcriptCount > 0 && !expanded && !hasPendingApproval && event.name === App.agentTeamParallelTool; chips: root.revision >= 0 && root.messages ? root.messages.childAgentsAt(root.row, root.path) : [] }`。
- 运行中的子 Agent 收起：RowLayout(spacing 14) 两个 `Button { buttonStyle: "plain"; textStyle: "caption"; foregroundColor: Theme.secondaryLabel }`：「已经 N 步，展开看过程」（`ellipsis`）→ `root.toggleExpanded()`（带现有高度动画）；「在 Agent 树里看」（`point.3.filled.connected.trianglepath.dotted`）→ `AgentTeam.treeFocus = root.event.id`。条件：有记录、未展开、无待确认、不是并行、`status === "running"`。
- 运行中的 delegate：`AgentControls { visible: hasTranscript && event.name === App.agentTeamDelegateTool && root.agentRunning; eventID: event.id; name: event.agentName || "子 Agent"; showsInput: false }`，`readonly property bool agentRunning: AgentTeam.revision >= 0 && AgentTeam.isRunning(event.id)`。
- 标题行：`RowLayout { StyledText 标题 (Layout.maximumWidth: implicitWidth，可换行); ModelPill { usage: root.event.usage; visible: !!usage.model; Layout.maximumWidth: …（中间截断）}; Item { Layout.fillWidth: true } }`。

**ChildAgentChips.qml**：`Flow { spacing: 6 }` + Repeater(`chips`)：每个 `T.AbstractButton`（自绘可点块，允许自定 background）：`background: Rectangle { radius: height / 2; color: Theme.opacity(Theme.primary, 0.06) }` + 悬停 / 按下叠 `Theme.hoverFill / pressedFill`；内容 Row(spacing 4){ `AgentStatusIcon { kind: modelData.statusKind; size: 12 }`、`StyledText { text: modelData.name; textStyle: "caption"; weight: Font.Medium; elide }`、`StyledText { visible: modelData.model.length > 0; text: modelData.model; textStyle: "caption2"; color: Theme.tertiaryLabel }` }，内边距 8 / 4；`QQC.ToolTip`「在 Agent 树里看「name」」；`onClicked: AgentTeam.treeFocus = modelData.id`。

**AgentStatusIcon.qml**：`Item { property string kind; property real size: 14 }`，按 kind 显示一个子项：`approval` → Icon hand.raised.fill `Theme.orange`；`queued` → hourglass `Theme.secondaryLabel`；`running` → `Spinner { controlSize: "mini" }`（12，比 size 小时居中）；`succeeded` → checkmark.circle.fill `Theme.green`；`stopped` → stop.circle.fill `Theme.secondaryLabel`；`failed` → xmark.octagon.fill `Theme.red`。

**ModelPill.qml**：`Rectangle { property var usage; radius: height / 2; color: Theme.opacity(Theme.primary, 0.07) }`，`StyledText { text: usage.model; textStyle: "caption2"; weight: Font.Medium; color: Theme.secondaryLabel; elide: Text.ElideMiddle; maximumLineCount: 1 }`，内边距 6 / 1.5，`HelpArea { text: usage.detail }`。

### 2.6 v1.8 Agent 树

**AgentTreeModel**（新 C++，声明见 §3.9；每个 ChatView 一个：`AgentTreeModel { id: agentTree; conversationID: root.conversationID; active: panelLoader.active }`）
- 输入：核心 `currentPath(conversationID)`（含正在生成的那条）、`AgentTeam`（running / activity / speed / passTokens）、`mainLiveSpeed`、`generatingConversationID`。
- 重建：`ConversationStore::messageChanged / pathChanged / conversationChanged`（只认自己这段）、`AgentRunner::generatingChanged`、`AgentTeam::changed` → 合并成一次（单次计时器 ~150–250 ms；**按消息 id + 修订号缓存每条消息的节点**，流式时只重算正在写的那条）。`setConversationID` 里**同步**算一次（`deepRunning` 的初值不能触发「由假变真」，== SwiftUI onChange 不对初值触发）。
- 按秒：`AgentTeam.clockChanged` 且（面板开着或这段在生成）→ 重算合计（`usageChanged`）、运行中节点行的 `DetailLine / SpeedLabel / Generated` 发 dataChanged；结束后最后再算一次。
- 行：「turn」与「node」两类（概览放 ListView header）；用行身份（turn:id / node:id）做差量插删 + dataChanged（同 SidebarModel 的 identity / sameContent 写法），**不要** beginResetModel（悬停、转圈动画、展开状态会丢）。`collapsed / expandedTurns` 存在模型里（随 ChatView 重建而重置，同 Swift 的 @State）。
- 「过程」区：模型自己实现与 MessageListModel 同名同参的 `blockIDsAt / blockAt / blockRevisionAt / childAgentsAt`（row = **完整路径**里的消息下标——MessageListModel 只暴露最后 visibleCount 条，树要能看任何一轮）。建议把 MessageListModel 里按块计修订号的逻辑（`BlockSnapshot / diffBlocks`）抽成共用的小类，两边复用。

**AgentTreePanel.qml**（`objectName: "agentTreePanel"`）
- `property AgentTreeModel tree`、`property string selectedID`（== @State selected）；`readonly property bool showingDetail: selectedID.length > 0 && tree.revision >= 0 && tree.hasNode(selectedID)`。
- 底：`Rectangle { color: Theme.opacity(Theme.primary, 0.025) }`（只在工具栏行以下）。
- 内容三选一：详情 `Loader`（selectedID 变就重建 == `.id(selected)`）；`tree.isEmpty` → `ContentUnavailableView { title: "还没有子 Agent"; iconName: "point.3.filled.connected.trianglepath.dotted"; description: "主 Agent 派出子 Agent 后，整棵树会显示在这里：谁派了谁、用什么模型、在做什么、用了多少 token。" }`；否则 `ListView`（边距 12，header = AgentPanelSummary，delegate 按 rowType 用 Loader 选 turn / node）。行距靠委托上边距：turn 行上空 16，轮次第一个节点上空 6，其余 0。
- 接住焦点：`Component.onCompleted: takeFocus()`；`Connections { target: AgentTeam; function onTreeFocusChanged() { panel.takeFocus() } }`；`function takeFocus() { if (AgentTeam.treeFocus.length === 0) return; selectedID = AgentTeam.treeFocus; AgentTeam.treeFocus = "" }`。
- turn 行（可点 Item + TapHandler，最新一轮不响应；HelpArea = 原文）：`chevron.right`（非最新；caption2 Bold，展开转 90°，`Theme.duration.normal` snappy）、`StyledText turnTitle（caption DemiBold）`、请求（caption 单行 elide、二级色、fillWidth）、计数（caption2 三级色 monospacedDigit）；文字整体二级色。

**AgentTreeRow.qml**（== TreeRow + TreeGuides）：`AgentNodeCard` 的 x = level × 14，上下各 2；左侧 level × 14 宽的连接线：一条 `Shape`（`preferredRendererType: Shape.CurveRenderer`，或几何渲染器 + `layer.samples`），几何照 1.4 TreeGuides（列宽 14、列中心 +7、elbow 19、圆角 4、线宽 1、`Theme.opacity(Theme.secondaryLabel, 0.4)`）；level 0 不画。

**AgentNodeCard.qml**（== NodeCard；也用于详情页「它派出的下级」，那里 `toggleEnabled: false`）
- 属性：节点行的各字段（name、statusKind、isRunning、parallel、childCount、expanded、usage、detailLine、speedLabel、generated、prompt）+ 信号 `toggleRequested() / openRequested()`。
- 展开箭头位 12×16：有子节点时 `IconButton`/自绘可点 `chevron.right`（caption2 Bold、二级色、展开转 90°），提示「收起它的下级」/「展开它的下级（N 个）」。
- `AgentStatusIcon { kind; size: 14 }`（框 14×16）。
- Column(spacing 3)：RowLayout(spacing 5){ 名字（callout DemiBold、单行）；ModelPill；填充(≥4)；「并行」（caption2 二级色，`Rectangle { radius: height/2; color: "transparent"; border.color: Theme.opacity(Theme.secondaryLabel, 0.4) }`，左右 5）；`Label { title: childCount; iconName: "person.2"; textStyle: "caption2"; color: Theme.tertiaryLabel }` }；detailLine（caption 二级色单行）；Row(spacing 8){ 三个「图标强调色 + 间距 2 + 文字」：bolt.fill speedLabel、arrow.up.forward generated、arrow.down.backward prompt }（caption2 二级色 monospacedDigit，HelpArea「速度 · 生成 · 输入（tokens）」）。
- 底：`Rectangle { radius: Theme.radius.card; color: <按状态> }`：approval `Theme.opacity(Theme.orange, 0.14)`、running/queued `Theme.opacity(Theme.accent, 0.08)`、stopped `Theme.opacity(Theme.gray, 0.1)`、failed `Theme.opacity(Theme.red, 0.08)`、succeeded `Theme.opacity(Theme.primary, 0.04)`；悬停 × 1.6（`Theme.opacity(c, 1.6)`）；运行中 1px `Theme.opacity(Theme.accent, 0.35)` 描边。HoverHandler + TapHandler → `openRequested()`；HelpArea「查看「name」的任务、结论和过程」。内边距 8 / 7。

**AgentPanelSummary.qml**：圆角 `Theme.radius.group`，`Theme.opacity(Theme.primary, 0.045)`，内边距 12；三行照 1.4（StatusChip / Metric 用内联 component；StatusChip 圆点 6、底 `Theme.opacity(<色>, count > 0 ? 0.14 : 0.06)`、文字 `count > 0 ? Theme.label : Theme.secondaryLabel`；Metric 值 callout DemiBold monospacedDigit）；指标行 HelpArea = `tree.summaryHelp`。

**AgentDetailView.qml**：`ScrollView`（Friday.Style）里 ColumnLayout(spacing 12，边距 12)；数据 `readonly property var info: AgentTeam.clock >= 0 && tree.revision >= 0 ? tree.node(nodeID) : ({})`。
1. `Button { buttonStyle: "borderless"; text: "Agent 树"; iconName: "chevron.left"; onClicked: back() }`。
2. 头卡：`AgentStatusIcon { size: Theme.iconSize("title3") }`、名字（title3 DemiBold 单行）、ModelPill、填充、「第 N 层」（caption Medium 二级色，胶囊 `Theme.opacity(Theme.primary, 0.07)`，7 / 2）；上级链放横向 `Flickable`（不显示滚动条）：「主 Agent」二级色、每个祖先 `Icon chevron.right`（caption2 三级色）+ `Button { buttonStyle: "link"; text: 祖先名; onClicked: open(id) }`，最后自己名字，caption；`info.usageLine`（caption2 三级色）。
3. 指标 RowLayout(spacing 8) 四块（各 `Layout.fillWidth`，圆角 `Theme.radius.medium`，`Theme.opacity(Theme.primary, 0.045)`，内边距 8 / 6；值 callout DemiBold monospacedDigit，放不下时 `fontSizeMode: Text.HorizontalFit` + `minimumPixelSize` = 0.7 倍）。
4. 运行中：`AgentControls { eventID; name; showsInput: true }`。
5. 卡片（圆角 `Theme.radius.card`，`Theme.opacity(Theme.primary, 0.03)`，内边距 10，标题 caption DemiBold 二级色）：「任务」`SelectableText`；「结论」/「结果」`SelectableText`；「它派出的下级（N）」Column(spacing 4) 的 AgentNodeCard（点了 `open(child.id)`）；「过程」`SubAgentTranscript { messages: tree; row: info.row; path: info.blockPath; revision: tree.revision; totalCount: info.transcriptCount; isLive: info.isRunning; nesting: 2 }`。

**AgentTreeButton.qml**（`ToolbarButton`，objectName `agentTreeButton`）：`iconName: "point.3.filled.connected.trianglepath.dotted"`；`text: tree.latestRunning > 0 ? String(tree.latestRunning) : ""`（有文字时 TextBesideIcon；可选 `font.features: { "tnum": 1 }` 做等宽数字）；`help: tree.buttonHelp`；`Accessible.name: "Agent 树"`；`onClicked: UiState.agentTreeVisible = !UiState.agentTreeVisible`。

**ConversationUsageBadge.qml**（objectName `conversationUsageBadge`）：`Item { visible: tree.usageVisible; implicitHeight: Theme.window.toolbarItemHeight; implicitWidth: row.implicitWidth + 12 }`，Row(spacing 5)：`Icon bolt.fill`（`Theme.accent`，caption 图标大小，`visible: tree.usageRunning`）、`speedText`、「·」（`Theme.tertiaryLabel`）[这三项只在 usageRunning 时]、`generatedText`、「·」、`promptText`；全部 `StyledText { textStyle: "caption"; weight: Font.Medium; monospacedDigit: true; color: Theme.secondaryLabel }`；`HelpArea { anchors.fill: parent; text: tree.usageHelp }`（带 HoverHandler，合并标题栏时这一块命中为 HTCLIENT、能弹提示，不能在它上面拖窗口——可接受）。

### 2.7 v1.8 ChatView / ContentView / UiState

**UiState**（新 C++，QML_SINGLETON，声明 §3.9）：`agentTreeVisible`（默认 false）、`agentTreeWidth`（默认 320，写入时夹到 260…560），存 `SettingsFile`（settings.ini）键 `agentTreeVisible` / `agentTreeWidth`（同 Swift 的 @AppStorage 键名）。

**ChatView.qml**

> 结构**已由 PORT_V15_V19.md §3.11.9 取代**：骨架把 ChatView 改成 `chatArea`（宽 = `root.width - treeColumn.space`）+ `AgentTreeColumn`（tree-tray 的组件，下面的 `reveal / panelSpace`、
> PanelResizeHandle、面板 Loader、自动打开、接住 treeFocus 都写在它里面，对外只读 `space`、`panelActive`）+ 工具栏两项；之后 ChatView.qml 归 chat-ui，tree-tray 不改它。
> 下面的布局数值、动画、拖动、自动打开规则仍然有效，只是位置挪进 AgentTreeColumn。

- 新结构：现有的 contentClip / ForkIndicator / ComposerView 包进 `Item { id: chatArea; x: 0; width: root.width - root.panelSpace }`（锚到 chatArea 而不是 root；MessageList 列宽、输入栏宽自然跟着变）；Toolbar 仍 `width: root.width`（横跨面板上方，合并时就是标题栏）。
- `readonly property real panelSpace: reveal * (UiState.agentTreeWidth + 7)`；`property real reveal: UiState.agentTreeVisible ? 1 : 0; Behavior on reveal { NumberAnimation { duration: Theme.duration.normal; easing.type: Theme.snappyEasing } }`（== `.animation(.snappy)` + `.move(edge: .trailing)` + 透明度；若长对话里每帧重排太卡，退路是对话区宽度一步到位、只滑面板，见 §5）。
- `PanelResizeHandle { x: chatArea.width; y: toolbar.height; height: root.height - toolbar.height; width: 7; visible: root.reveal > 0 }`：中间 1px `Theme.separator`（`Theme.hairline`）竖线；`MouseArea { cursorShape: Qt.SplitHCursor; hoverEnabled: true }` 按住拖动 `UiState.agentTreeWidth = clamp(起始宽 − dx, 260, 560)`（同 NavigationSplitView 的 resizer 写法）；`HelpArea`「拖动调整 Agent 树的宽度」；objectName `agentTreeResizeHandle`。**从工具栏下沿开始**，不进工具栏行。
- `Loader { id: panelLoader; active: root.reveal > 0; x: chatArea.width + 7; y: toolbar.height; width: UiState.agentTreeWidth; height: root.height - toolbar.height; opacity: root.reveal; sourceComponent: AgentTreePanel { tree: agentTree } }`（面板关着时不建，节省按秒刷新）。内容在工具栏下沿剪掉（同 contentClip）。
- 工具栏：`Toolbar { title: chatModel.title; ConversationUsageBadge { tree: agentTree } AgentTreeButton { tree: agentTree } }`（声明顺序 = 从左到右；放不下时先截标题，再把左边的项收进「»」——徽标不是按钮，收起时不进溢出菜单，等价于 NSToolbar 收掉自定义视图）。
  （溢出**已由 PORT_V15_V19.md §3.11.7 改为**：Toolbar 支持 `overflowText`；树按钮在「»」里是「Agent 树（运行中 N）」，徽标收起时是一行禁用的「速度 · 生成 · 输入」。）
- 自动打开：`property bool autoOpened: false`；`Connections { target: agentTree; function onDeepRunningChanged() { if (agentTree.deepRunning && !root.autoOpened && !UiState.agentTreeVisible) { root.autoOpened = true; UiState.agentTreeVisible = true } } }`。
- 树焦点：`Connections { target: AgentTeam; function onTreeFocusChanged() { if (AgentTeam.treeFocus.length > 0) UiState.agentTreeVisible = true } }`（面板建好后自己 takeFocus；两个处理器谁先跑都一样）。

**ContentView.qml**（现 :88–99）：
```js
function applyWidth() {
    if (width <= 0) return
    const panel = UiState.agentTreeVisible ? UiState.agentTreeWidth : 0
    if (width < collapseSidebarBelow + panel && split.sidebarVisible) { sidebarAutoCollapsed = true; split.sidebarVisible = false }
    else if (width > expandSidebarAbove + panel && sidebarAutoCollapsed) { sidebarAutoCollapsed = false; split.sidebarVisible = true }
}
// + Connections { target: UiState; function onAgentTreeVisibleChanged() { root.applyWidth() } }（宽度变化不单独触发，同 Swift）
```

### 2.8 v1.8 设置（只改文案 / 加一项）
- `AgentsSettingsView.qml`（现 :14–61）：footer 换成 1.4 的新文字（删 `App.agentTeamMaxDepth` 插值）；在「最多 N 次」FormRow 后加：`FormRow { Toggle { text: "限制每轮派发的子 Agent 总数"; help: "所有层加起来。默认 100 个，只防失控：到了就不再派，用已有结果收尾"; checked: AppSettings.subAgentTurnLimit > 0; onToggled: { AppSettings.subAgentTurnLimit = checked ? 100 : 0; checked = Qt.binding(…) } } }` 与 `FormRow { visible: AppSettings.subAgentTurnLimit > 0; Stepper { text: "最多 " + AppSettings.subAgentTurnLimit + " 个"; from: 10; to: 1000; stepSize: 10; value: …; onValueModified: { 保存; value = Qt.binding(…) } } }`。
- `FridayApp.h/.cpp`：删 `agentTeamMaxDepth`（核心去掉 `AgentTeamTools::maxDepth`，不删会编不过）。
- `EngineEditor.qml:141–154`：「起步并发 N（不设上限：没被限流就往上加，遇到 429 减半）」；文件头注释「并发请求数」→「起步并发」。
- `SecretsSettingsView.qml:258`、`ServiceModelPicker.qml:321、332`、`ServiceModelsSheet.qml:80`、`ModelMenuBuilder.cpp:187`：「并发」→「起步并发」。注意：Qt 版这三处另有「改接口地址、并发数…」——那是更早的 Swift d621837（显示名 / 接口地址 / 密钥名改为服务一级）还没移植留下的差异，见 §5。

### 2.9 v1.9 托盘

**平台分析**
- Windows 通知区图标是方形 SM_CXSMICON：100% 16 px、125% 20、150% 24、200% 32；不能加宽，也没有「图标旁边一行字」的 API。Swift 的标签宽约 3 + 44 + 图标、两行 9pt 字：放进 16 px 高的方块，每行字高不到 8 px（CJK / 数字至少要约 9–10 px 才认得出），「179 t/s」这类 6–7 个字符在 16–32 px 宽里怎么排都放不下；32 px 时勉强塞一行 3–4 个字符，但图标本身就认不出了。**结论：不可读，不画字。**
- Linux：SNI（StatusNotifierItem）同样只有图标；GNOME + AppIndicator 扩展有 Ayatana 的 `XAyatanaLabel`（图标旁的文字），但 Qt 的 QSystemTrayIcon 不支持，要自己实现 SNI —— 不做（记为以后可选）。GNOME 的 AppIndicator 扩展不显示 tooltip；KDE 显示。
- 最接近的原生做法：**tooltip + 菜单里的状态行**；图标本身不变（Swift 的图标字形也不变）。可选：有 Agent 在工作时图标角上加一个强调色小圆点（与 Swift 有差异，默认不做，§5 Q1）。

**FridayApp 改动**（现 `FridayApp.cpp:91–167`）
- `buildTrayMenu`：状态行后插入 `m_trayAgents = m_trayMenu->addAction(QString()); m_trayAgents->setEnabled(false);`。
- `updateTrayMenu`：
```cpp
const int agents = runner->liveAgents();
const QString speed = Format::meterSpeed(runner->liveSpeed());
QString line = QStringLiteral("%1 AI 在工作 · %2").arg(agents).arg(speed);
line.replace(QLatin1Char('&'), QLatin1String("&&"));
m_trayAgents->setVisible(agents > 0);
if (m_trayAgents->text() != line) m_trayAgents->setText(line);
const QString tip = QStringLiteral("Friday\n%1 AI\n%2").arg(agents).arg(speed);
if (m_tray->toolTip() != tip) m_tray->setToolTip(tip);
```
  （tooltip 不超过 127 字符；初值「Friday\n0 AI\n0 t/s」，同 Swift 空闲时的「0 AI」「0 t/s」。）
- `setupTray`：`connect(runner, &AgentRunner::liveMeterChanged, this, &FridayApp::updateTrayMenu)`。核心每秒最多发一次、值不变不发，所以菜单开着时不闪、不被关掉（现有「只建一次、只改文字」的做法不变）。
- 给 DebugHarness：`QString trayToolTip() const`。

### 2.10 图标（`style/icons/mapping.json`，改完运行 `python3 qt/tools/gen_icons.py`，提交生成物）

| SF Symbol | Lucide | 备注 |
|---|---|---|
| `stop.circle` | `circle-stop` | 「已停止」、子 Agent 停止键、「停止子 Agent」工具 |
| `stop.circle.fill` | `circle-stop` + `fill: knockout`，`container` 指向圆（照 `checkmark.circle.fill` 的写法，生成后目视确认下标） | 树里「停止」状态 |
| `text.bubble` | `message-square-text` | 「工作中补充」、`agent_message` 卡片、「给子 Agent 追加指示」 |
| `waveform.path.ecg` | `activity` | 子 Agent 状态行 |
| `point.3.filled.connected.trianglepath.dotted` | `network` | Agent 树按钮 / 空状态 / 「在 Agent 树里看」 |
| `arrow.up.forward` | `arrow-up-right` | 生成 |
| `arrow.down.backward` | `arrow-down-left` | 输入 |
| `arrow.up.circle.fill` | `circle-arrow-up` + knockout | 详情页发给子 Agent |
| `arrow.up.message` | `message-square-share` | 「向父级汇报」工具（设置 › 工具 列表里也会出现） |

已有：`arrow.triangle.2.circlepath`、`clock.arrow.circlepath`、`pause.circle`、`person.2.badge.gearshape`、`person.2`、`chevron.left`、`hourglass`、`bolt.fill`、`ellipsis`、`hand.raised.fill`、`xmark.octagon.fill`、`checkmark.circle.fill`、`person.3.sequence`、`doc.text.magnifyingglass`。

---

## 3. 界面需要核心提供的数据（C++ 声明）

约定：主线程读写、改了发信号；QML 的参数是 UUID 字符串（大写）/ 原始字符串，空字符串 = nil（同现有 `AgentRunner` 的 QML 部分）。子 Agent 在工作线程跑，它们对团队状态的修改要排队到主线程（同现有 `updateMessage / updateEvent`）。

### 3.1 `core/models/ChatModels.h`
```cpp
/// 一个子 Agent 用的大脑和 token 用量（== AgentUsage，Models/ChatModels.swift:84–106）
struct AgentUsage {
    QString model;                    // 在线引擎的 model；本地模型的显示名
    std::optional<QString> service;   // 在线服务名；本地 nullopt
    QString thinking;                 // 「关闭」「低」「中」「高」；不支持「不支持」
    double temperature = 0;
    int generatedTokens = 0;
    int promptTokens = 0;             // 本地：新处理的提示词；在线：各段请求的输入合计
    double generateTime = 0;          // 不含等首 token
    double tokensPerSecond() const;   // generateTime > 0 ? generated / time : 0
    QString detail() const;           // "深度思考：X\n温度：Y[\n服务：Z]"（温度 0–2 位小数、去尾零）
    QJsonObject toJson() const;       // 键同 Swift Codable
    static AgentUsage fromJson(const QJsonObject&);
    /// {model, service, thinking, temperature, generatedTokens, promptTokens, generateTime, tokensPerSecond, detail}
    QVariantMap toVariant() const;
    bool operator==(const AgentUsage&) const = default;
};

struct ToolEvent {
    // … 现有字段 …
    /// 用户点了停止、没跑完（status 记 failed；界面灰色「已停止」）
    std::optional<bool> stopped;
    /// 子 Agent 的大脑和用量（子 Agent 每段生成结束更新）
    std::optional<AgentUsage> usage;
};
// ToolEvent::toVariant 追加：{"stopped": bool（缺省 false）, "usage": AgentUsage::toVariant()（没有为空 map）}

struct ChatMessage {
    // … 现有字段 …
    std::optional<bool> stoppedByUser;   // 这一轮是用户点停止结束的
    std::optional<bool> interjected;     // 工作中补充
    std::optional<QString> fromAgent;    // 后台子 Agent 的汇报 / 结果（值是子 Agent 名字；role 为 user）
};
// ChatMessage::toVariant 追加：stoppedByUser、interjected（bool）、fromAgent（缺省空串）

/// 还在跑 / 排队 / 等确认的调用（递归进子 Agent 记录）标成已停止（== MessageBlock.markStopped）
static void MessageBlock::markStopped(QList<MessageBlock>& blocks);
```

### 3.2 `core/agent/QueuedInput.h`（新）
```cpp
/// Agent 工作时排队的消息（== QueuedInput.swift）；子 Agent 的队列也用它
struct QueuedInput {
    QUuid id = QUuid::createUuid();
    QString text;
    QStringList attachments;              // 本地路径（发出时才导入媒体目录）
    QDateTime createdAt = QDateTime::currentDateTimeUtc();
    std::optional<QString> from;          // nullopt = 用户；否则父 Agent 的名字（message_agent）
    /// {id, text, attachments: [路径], attachmentNames: "a.png、b.pdf", from（缺省空串）}
    QVariantMap toVariant() const;
};
```

### 3.3 `core/agent/AgentRunner.h` 追加

> **已由 PORT_V15_V19.md §3.2–§3.3 取代**的几处：没有 `Q_PROPERTY(AgentTeamState* team …)` / `AgentTeamState* team() const`（C++ 是公有成员 `AgentRunner::team`，QML 用单例 `AgentTeam`）；
> `agentMessageTool` 是 `static QString agentMessageTool()`（不是 `constexpr const char*`）；`TurnChanges` 名字不变；另有 `queueRevision` 属性与 `turnMessageStarted` 信号。其余一致。

```cpp
    Q_PROPERTY(bool isStopping READ isStopping NOTIFY stoppingChanged)
    /// 菜单栏 / 托盘：正在工作的 Agent 个数（主 Agent + 运行中的子 Agent，全局）与速度合计（0.1 精度）
    Q_PROPERTY(int liveAgents READ liveAgents NOTIFY liveMeterChanged)
    Q_PROPERTY(double liveSpeed READ liveSpeed NOTIFY liveMeterChanged)
    Q_PROPERTY(AgentTeamState* team READ team CONSTANT)
public:
    bool isStopping() const;                       // 点了停止、这一轮还没收尾
    int liveAgents() const;
    double liveSpeed() const;
    AgentTeamState* team() const;

    // 工作中补充——主线程
    void submit(const QString& text, const QStringList& attachments, const QUuid& conversationID,
                const std::optional<QUuid>& replacing = std::nullopt);
    QList<QueuedInput> queuedInputs(const QUuid& conversationID) const;
    bool isQueuePaused(const QUuid& conversationID) const;
    void removeQueued(const QUuid& itemID, const QUuid& conversationID);
    void resumeQueue(const QUuid& conversationID);
    struct TurnChanges { bool model = false; bool thinking = false; };
    /// 这段在生成、且当前大脑 / 思考强度与这一轮开始时不同（== pendingTurnChanges）
    TurnChanges pendingTurnChanges(const QUuid& conversationID) const;
    /// 主 Agent 最近 3 秒窗口的速度（停下 3 秒后 0；不是这段在生成为 0）
    double mainLiveSpeed(const QUuid& conversationID, const QDateTime& now = QDateTime::currentDateTimeUtc()) const;

    // 子 Agent——主线程（== AgentTeamControl.swift / AgentLiveness.swift 里的 extension AgentRunner）
    /// 「在执行「X」 · 12 秒前有动静 · 第 5 步 · 已运行 3 分 2 秒[ · 期限还剩 …]」；compact 只留在做什么 · 第几步 · 时长；没活动 nullopt
    std::optional<QString> agentStatusText(const QUuid& agentID, const QDateTime& now, bool compact = false) const;
    bool sendToAgent(const QUuid& agentID, const QString& text, const QStringList& attachments = {},
                     const std::optional<QString>& from = std::nullopt);
    void removeAgentQueued(const QUuid& itemID, const QUuid& agentID);
    bool stopAgent(const QUuid& agentID, const QString& by = QStringLiteral("用户"));
    /// 汇报卡片标题：「中途汇报」「完成」「失败」「已停止」（参数是消息第一行）
    static QString noticeTitle(const QString& firstLine);
    static constexpr const char* agentMessageTool = "agent_message";

    // QML
    Q_INVOKABLE void submit(const QString& text, const QStringList& attachments, const QString& conversationID,
                            const QString& replacing = QString());
    Q_INVOKABLE QVariantList queuedInputs(const QString& conversationID) const;     // [QueuedInput::toVariant]
    Q_INVOKABLE bool isQueuePaused(const QString& conversationID) const;
    Q_INVOKABLE void removeQueued(const QString& itemID, const QString& conversationID);
    Q_INVOKABLE void resumeQueue(const QString& conversationID);
    Q_INVOKABLE QVariantMap pendingTurnChanges(const QString& conversationID) const; // {model, thinking}
    Q_INVOKABLE double mainLiveSpeed(const QString& conversationID) const;
    Q_INVOKABLE QString agentStatusText(const QString& agentID, bool compact = false) const; // 没活动返回空串
    Q_INVOKABLE bool sendToAgent(const QString& agentID, const QString& text);
    Q_INVOKABLE void removeAgentQueued(const QString& itemID, const QString& agentID);
    Q_INVOKABLE bool stopAgent(const QString& agentID);

signals:
    void stoppingChanged();
    /// 某段对话的排队列表或暂停状态变了
    void queuedInputsChanged(const QUuid& conversationID);
    /// liveAgents / liveSpeed 变了（每秒最多一次，值不变不发；都停下归零时发一次）
    void liveMeterChanged();
    // pendingTurnChanges 的变化由现有 generatingChanged（这一轮开始 / 结束）+ conversationSettingsChanged（改大脑 / 思考）覆盖
```
已有、继续用：`currentPath(conversationID)`（含正在生成的那条）、`generatingConversationID`、`resolveApproval`、`AgentRunner::arguments(json)`（详情页取 `task`）。

### 3.4 `core/agent/AgentTeamControl.h`：`AgentTeamState`（QObject，注册成 QML 单例 `AgentTeam`）

> **已由 PORT_V15_V19.md §3.4 取代**：文件是 `core/agent/AgentTeamState.h`；不是「主线程」对象——内部加锁、任意线程可调，信号只在主线程发，只有 `changed()`
> （running / activity / steps / queues 任一变化，配 `revision`）、`treeFocusChanged()`、`clockChanged()`，没有 `runningChanged / activityChanged / queuesChanged`；
> QML 方法 `isRunning / queue / speed / passTokens / isStopRequested / hasActivity`，属性 `revision / runningCount / clock / treeFocus`。`main.cpp` 的注册由骨架做。

```cpp
/// 父子 Agent 往来的状态（== AgentTeamState，Agent/AgentTeamControl.swift:12–194）。主线程
class AgentTeamState : public QObject {
    Q_OBJECT
    /// 要在 Agent 树面板里打开的子 Agent（「在 Agent 树里看」、小标签、调试钩子写；面板接住后清空）
    Q_PROPERTY(QString treeFocus READ qmlTreeFocus WRITE setQmlTreeFocus NOTIFY treeFocusChanged)
    /// running / activity / steps / queues 任一变化 +1（QML 绑定的依赖）
    Q_PROPERTY(int revision READ revision NOTIFY changed)
    /// 有子 Agent 在跑或主 Agent 在生成时每秒 +1（状态行、树的运行中节点、用量合计按它重算；都停下就停）
    Q_PROPERTY(int clock READ clock NOTIFY clockChanged)
public:
    enum class Activity { Generating, Tool, WaitingChildren, WaitingApproval, Queued };
    bool isRunning(const QUuid& id) const;
    const QSet<QUuid>& running() const;
    QList<QueuedInput> queue(const QUuid& id) const;
    std::optional<Activity> activity(const QUuid& id) const;
    /// 最近 3 秒窗口的实时速度（tok/s）
    double speed(const QUuid& id, const QDateTime& now = QDateTime::currentDateTimeUtc()) const;
    /// 这一段还没结束的估算 token（段结束由引擎的精确数字接替、清零）
    int passTokens(const QUuid& id) const;
    std::optional<QUuid> treeFocus() const;
    void setTreeFocus(const std::optional<QUuid>& id);

    Q_INVOKABLE bool isRunning(const QString& id) const;
    Q_INVOKABLE QVariantList queue(const QString& id) const;    // [{id, text, from}]
    Q_INVOKABLE double speed(const QString& id) const;
    Q_INVOKABLE int passTokens(const QString& id) const;
signals:
    void changed();
    void runningChanged();
    void activityChanged(const QUuid& id);
    void queuesChanged(const QUuid& id);
    void clockChanged();
    void treeFocusChanged();
};
```
`main.cpp`：`registerSingleton("AgentTeam", core.runner->team());`（在 qml_register_types_Friday 之后、与其他核心单例一起）。

### 3.5 `core/models/AppSettings.h` 追加
```cpp
    /// 一轮最多派多少个子 Agent（所有层加起来；只防失控）。0 = 不限；默认 100；键 subAgentTurnLimit
    Q_PROPERTY(int subAgentTurnLimit READ subAgentTurnLimit WRITE setSubAgentTurnLimit NOTIFY subAgentTurnLimitChanged)
```

### 3.6 `core/util/Format.h` 追加（+ `viewmodels/Formatters.h` 的 QML 包装）
```cpp
    /// == AgentTree.compact：≥1,000,000 "%.1fM"，≥1000 "%.1fk"，否则原数（与「万 / 亿」的 compact 不同）
    static QString agentTokens(qint64 n);
    /// == AgentTree.speedText：≤0 "— tok/s"，≥100 "%.0f tok/s"，否则 "%.1f tok/s"
    static QString agentSpeed(double tokensPerSecond);
    /// == MenuBarLabel.speedText：0 t/s、8.5 t/s、179 t/s、1.2 Kt/s、3.4 Mt/s、1.1 Gt/s、1.1 Tt/s
    static QString meterSpeed(double tokensPerSecond);
```
千分位（`Int.formatted()`）用现有 `Format::grouped`。

### 3.7 子 Agent 区需要保证的界面可见行为
- 派发被拒（相似度过高、超过每轮总数）的 delegate 事件**没有** transcript（树据此排除）。
- 并行 / 后台分组卡片 `agentName`「并行 ×N」/「后台 ×N」、结束时的结果文字；子 Agent 收到消息时在它的运行记录里追加 `agent_message` 卡片（标题、图标、结果见 1.3）。
- `usage` 在子 Agent 每段生成结束写回卡片（重开对话还在）；生成中通过 `team.noteTokens` 维护 `passTokens / speed`。
- 停止：`markStopped` 递归标记；被停的子 Agent 卡片结果「被<谁>停止了，没做完。」+ 半成品。
- 后台汇报以 `role: user, fromAgent: 名字` 的消息存档（界面画成汇报卡片）。

### 3.8 界面层 view-model 声明（UI 移植里实现）

`viewmodels/ChatViewModel.h` 追加 / 修改：
```cpp
    /// == runner.canSend(in:)：在线引擎存在 / 这段对话选的本地模型已就绪（**不再** && !isGenerating；文字 / 附件 / 生成中的判断在 QML）
    Q_PROPERTY(bool canSend READ canSend NOTIFY changed)
    Q_PROPERTY(bool isStopping READ isStopping NOTIFY changed)
    /// 「下一轮生效」
    Q_PROPERTY(bool pendingModelChange READ pendingModelChange NOTIFY changed)
    Q_PROPERTY(bool pendingThinkingChange READ pendingThinkingChange NOTIFY changed)
    Q_PROPERTY(QString pendingChangeHelp READ pendingChangeHelp NOTIFY changed)
    /// 排队（QueuedBubbles）：[{id, text, attachmentNames}]
    Q_PROPERTY(QVariantList queuedInputs READ queuedInputs NOTIFY queueChanged)
    Q_PROPERTY(bool queuePaused READ queuePaused NOTIFY queueChanged)
    /// 「已暂停：上一轮被你停止了」/「排队中：Agent 做完当前这一步就会看到」/「排队中：另一段对话结束后发出」/「排队中：模型就绪后发出」/「马上发出…」
    Q_PROPERTY(QString queueStatus READ queueStatus NOTIFY queueChanged)
    /// 空闲就直接发；Agent 在工作就排队（== runner.submit）
    Q_INVOKABLE void submit(const QString& text, const QStringList& attachments, const QString& replacing = QString());
    Q_INVOKABLE void removeQueued(const QString& itemID);
    Q_INVOKABLE void resumeQueue();
signals:
    void queueChanged();
```
（`queueStatus` 依赖生成状态和 canSend：在 `refresh()` 里一起比较、变了发 `queueChanged`。）

`viewmodels/MessageListModel.h` 追加：
```cpp
        StoppedByUserRole,     // bool
        InterjectedRole,       // bool
        FromAgentRole,         // QString（空 = 不是汇报）
        AgentNoticeTitleRole,  // 「中途汇报」「完成」「失败」「已停止」
        AgentNoticeBodyRole,   // plainText 第一个换行之后
    // 以上加进 contentRoles()（MessageListModel.cpp:45）
    /// 并行派发卡片收起时的小标签：path 指向的工具块的 transcript 里的顶层子 Agent（嵌套并行摊平；没 transcript 的跳过）
    /// [{id, name, statusKind, model}]；statusKind 同 AgentTreeModel
    Q_INVOKABLE QVariantList childAgentsAt(int row, const QVariantList& path) const;
```

### 3.9 新 view-model：`AgentTreeModel`、`UiState`
```cpp
// viewmodels/AgentTreeModel.h —— 对应 Swift：Views/AgentTreePanel.swift
class AgentTreeModel : public QAbstractListModel {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(QString conversationID READ conversationID WRITE setConversationID NOTIFY conversationIDChanged)
    /// 面板开着（维护行、按秒刷新运行中的行）；关着时只算按钮 / 徽标要的数
    Q_PROPERTY(bool active READ active WRITE setActive NOTIFY activeChanged)
    Q_PROPERTY(bool isEmpty READ isEmpty NOTIFY treeChanged)          // turns.isEmpty
    Q_PROPERTY(int revision READ revision NOTIFY treeChanged)         // 每次重建 +1
    // AgentTreeButton（最新一个有节点的轮次）
    Q_PROPERTY(int latestRunning READ latestRunning NOTIFY treeChanged)
    Q_PROPERTY(int latestDeepest READ latestDeepest NOTIFY treeChanged)
    Q_PROPERTY(int latestTotal READ latestTotal NOTIFY treeChanged)
    Q_PROPERTY(QString buttonHelp READ buttonHelp NOTIFY treeChanged)
    Q_PROPERTY(bool deepRunning READ deepRunning NOTIFY treeChanged)  // latestDeepest >= 3 && latestRunning > 0
    // PanelSummary（全部轮次）
    Q_PROPERTY(int total READ total NOTIFY treeChanged)
    Q_PROPERTY(int deepest READ deepest NOTIFY treeChanged)
    Q_PROPERTY(int running READ running NOTIFY treeChanged)
    Q_PROPERTY(int done READ done NOTIFY treeChanged)
    Q_PROPERTY(int stopped READ stopped NOTIFY treeChanged)
    Q_PROPERTY(int failed READ failed NOTIFY treeChanged)
    // 会话合计（ConversationUsageBadge / PanelSummary 指标行）
    Q_PROPERTY(bool usageVisible READ usageVisible NOTIFY usageChanged)   // generated > 0 || running > 0
    Q_PROPERTY(bool usageRunning READ usageRunning NOTIFY usageChanged)   // total.running > 0
    Q_PROPERTY(QString speedText READ speedText NOTIFY usageChanged)      // Format::agentSpeed(total.speed)
    Q_PROPERTY(QString generatedText READ generatedText NOTIFY usageChanged) // "生成 12.3k"
    Q_PROPERTY(QString promptText READ promptText NOTIFY usageChanged)       // "输入 1.2M"
    Q_PROPERTY(QString usageHelp READ usageHelp NOTIFY usageChanged)         // 徽标多行提示
    Q_PROPERTY(QString metricSpeed READ metricSpeed NOTIFY usageChanged)     // running > 0 ? speedText : "—"
    Q_PROPERTY(QString metricGenerated READ metricGenerated NOTIFY usageChanged)
    Q_PROPERTY(QString metricPrompt READ metricPrompt NOTIFY usageChanged)
    Q_PROPERTY(QString summaryHelp READ summaryHelp NOTIFY usageChanged)
public:
    enum Role {
        RowTypeRole = Qt::UserRole + 1,   // "turn" | "node"
        IDRole, TurnIDRole, FirstInTurnRole,
        TurnTitleRole /*本轮 / 第 N 轮*/, IsLatestRole, RequestRole, RequestHelpRole, OpenRole, TurnCountsRole /*N 个 · D 层*/,
        NameRole, DepthRole, LevelRole, GuidesRole /*QVariantList<bool>*/, IsLastRole, ParallelRole, ChildCountRole,
        ExpandedRole, StatusKindRole /*approval|queued|running|succeeded|stopped|failed*/, IsRunningRole, UsageRole,
        DetailLineRole, SpeedLabelRole /*运行中 speed / 结束「平均 」+speed*/, GeneratedRole, PromptRole,
    };
    Q_INVOKABLE void toggleTurn(const QString& turnID);   // 最新一轮不响应
    Q_INVOKABLE void toggleNode(const QString& nodeID);
    Q_INVOKABLE bool hasNode(const QString& nodeID) const;
    /// 详情：{id, name, depth, statusKind, isRunning, usage, usageLine, path:[{id,name}]（祖先）, task, result,
    ///        resultTitle（结论 / 结果）, children:[节点行字段], toolCalls, speedTitle（速度 / 平均速度）, speed,
    ///        generated, prompt, row（完整路径里的消息下标）, blockPath, transcriptCount}
    Q_INVOKABLE QVariantMap node(const QString& nodeID) const;
    // SubAgentTranscript / ToolCard 的 messages 接口（同 MessageListModel 的签名；row = 完整路径下标）
    Q_INVOKABLE QStringList blockIDsAt(int row, const QVariantList& path) const;
    Q_INVOKABLE QVariantMap blockAt(int row, const QVariantList& path) const;
    Q_INVOKABLE int blockRevisionAt(int row, const QVariantList& path) const;
    Q_INVOKABLE QVariantList childAgentsAt(int row, const QVariantList& path) const;
signals:
    void conversationIDChanged(); void activeChanged(); void treeChanged(); void usageChanged();
};

// viewmodels/UiState.h —— 对应 Swift 的 @AppStorage("agentTreeVisible") / @AppStorage("agentTreeWidth")
class UiState : public QObject {
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(bool agentTreeVisible READ agentTreeVisible WRITE setAgentTreeVisible NOTIFY agentTreeVisibleChanged)
    Q_PROPERTY(double agentTreeWidth READ agentTreeWidth WRITE setAgentTreeWidth NOTIFY agentTreeWidthChanged) // 260…560
};
```

---

## 4. GUI 测试计划（DebugHarness）

### 4.1 环境
- Debug 构建（核心的调试钩子只在 `!NDEBUG` 编进去）；每次用新的隔离数据目录 `FRIDAY_DATA_DIR=<scratch>\ui-v19\data`。
- 在线大脑用 mock：`python Friday/Tools/Perf/mock_sse.py 8765 <work>`（origin/main 版，纯 Python、跨平台），`mock_config.json` 调短：`{"agents": 3, "rate": 20, "think1": 6, "think2": 1, "answer": 6, "main_think": 2, "main_answer": 4}`。首次在 设置 › 密钥 用「自定义 OpenAI 兼容服务」接入 `http://127.0.0.1:8765/v1`（不需要密钥、勾工具调用），把生成的 engines.json / settings.ini 存成 fixture 以后复用。
- mock 需要扩展（新，写进同一个脚本的 DEFAULTS；**归 team**，PORT_V15_V19.md §5.2）：`nest: <层数>`（子 Agent 在层数不够时自己再 `run_parallel_tasks` 两个下级，用来出第 3 层、测自动打开）；`background: true`（主 Agent 派发带 `background: true`，测汇报卡片）；`report: true`（子 Agent 先 `report_to_parent` 一次）；`sub_approval: true`（子 Agent 调一个要确认的调用，例如 `run_shell` 删除类命令，测嵌套待确认的橙色）。
- 静态状态用「录制再回放」：跑一遍实时脚本后把数据目录复制成 fixture（停止、补充、汇报、结束的树都会存档）。注意：重新加载时还在跑的调用会被 `markInterrupted` 标成「已中断」（失败），所以**运行中**的状态只能实时测。
- 每个脚本跑：FluentWinUI3 浅 / 深；Fusion 浅 / 深（`FRIDAY_TITLEBAR=system`，模拟 Linux）；改了原生控件的（输入框圆按钮、子 Agent 输入框、设置里的 Stepper）再跑 `QT_QUICK_CONTROLS_STYLE=Windows`（只有浅色）。Linux 真机不测（交给 CI）。

### 4.2 脚本（`FRIDAY_SCREENSHOT_SCRIPT`，一行一步；选择器用 §2 定的 objectName / 类型名）

**S1 v1.5 停止**（`FRIDAY_SLOW_STOP=4`）
```
window:main
click:help~新对话
click:type=GrowingTextView
type:并行调查一下
key:Return
wait:generating
wait:2500
shot:v15-running
click:#stopButton
wait:300
shot:v15-stopping            # 旋转图标、按钮禁用、底栏「正在停止…」、没有打字动画
hover:#stopButton
wait:1000
shot:v15-stopping-help       # 提示「正在停止…」
until:AgentRunner.isGenerating === false
shot:v15-stopped             # 底栏「已停止」；子 Agent / 并行卡片灰色「已停止」
hover:text=已停止@0
wait:1000
shot:v15-stopped-help
```
另跑 `FRIDAY_AUTOSTOP=approval`（配 `sub_approval`）：等确认时被停 → 卡片「已停止」、结果「等待确认时被停止，没有执行」。

**S2 v1.6 工作中补充**
```
（同上发送）
wait:generating
click:type=GrowingTextView
type:第一条补充
shot:v16-queue-button        # 停止键右边出现强调色排队键；hover 提示「补充给 Agent：…」
key:Return
type:第二条补充
key:Return
shot:v16-queued              # 两个虚线气泡 + 撤回 +「排队中：Agent 做完当前这一步就会看到」；占位文字
click:text=撤回@1
shot:v16-withdrawn
click:type=ThinkingChip
click:text=高                # （或 FRIDAY_SWITCH="after:2|thinking:high"）
closepopups
shot:v16-next-turn           # 「下一轮生效」+ 提示文案
wait:text=工作中补充
shot:v16-interjected         # 当前回复收尾 → 「工作中补充」气泡 → 新回复接着写
```
分支：生成中切到另一段对话（`click:help~新对话`）→ 占位「另一段对话正在生成，现在发出的消息会排队…」、发一条 → 「排队中：另一段对话结束后发出」；排队后点停止 → `v16-paused`（「已暂停：上一轮被你停止了」+「发送」）→ `click:text=发送` → 发出。空状态：生成中新建对话，建议按钮可点（排队，但空会话不显示气泡——Swift 原样）。模型胶囊生成中可点（`menu:model` 在生成中截一张）。

**S3 v1.7 子 Agent 控件 / 汇报**
```
（发送后）wait:type=ChildAgentChips
shot:v18-chips               # 并行卡片收起：小标签（状态图标 + 名字 + 模型）
click:type=ToolCard@0        # 展开并行卡片
wait:type=AgentControls
shot:v17-card-controls       # 每个子 Agent 卡片：状态行（waveform）+ 行尾停止键；模型胶囊
```
`FRIDAY_AGENT_MESSAGE="after:4|改成只看日志"` → `shot:v17-agent-queue`（虚线条目 +「· 」来源 + 撤回）；`click:help~只停止这个子 Agent` → `shot:v17-agent-stopping`（需要钩子 `FRIDAY_SLOW_AGENT_STOP`，否则一闪而过）→ 卡片灰色「已停止」，其余子 Agent 照常。mock `background: true` + `report: true` → `wait:type=MessageRow`… `shot:v17-notice`（汇报卡片「子 Agent「研究员1」中途汇报 / 完成」）。

**S4 v1.8 Agent 树**（mock `nest: 3`）
```
wait:generating
until:UiState.agentTreeVisible === true     # 第一次出现第 3 层、还有在跑的 → 自动打开
shot:v18-tree-autoopen       # 概览（个数 / 状态小标签 / 速度生成输入）、本轮、连接线、运行中节点（转圈、强调色描边、状态行）
hover:#agentTreeButton
wait:1000
shot:v18-tree-button-help    # 「Agent 树：运行中 N 个，最深 D 层」+ 图标右边的个数
click:type=AgentNodeCard@2
shot:v18-detail-running      # 上级链、四个指标、带输入框的 AgentControls、任务、过程
click:type=TextField@0       # 详情里的输入框（按实际 objectName）
type:先停下这个方向
key:Return
shot:v18-detail-queued
click:text=Agent 树
click:type=AgentNodeCard@0   # 展开箭头：toggle
until:AgentRunner.isGenerating === false
shot:v18-tree-done           # 平均速度、结论第一行、完成 / 停止 / 失败计数
drag:#agentTreeResizeHandle::-200,0
shot:v18-tree-wide
resize:1000x700
shot:v18-sidebar-collapsed   # 840 + 面板宽阈值
```
另：`FRIDAY_TREE_FOCUS="after:5|最深"`（= 卡片上「在 Agent 树里看」）→ 详情直接打开；第二个问题（`FRIDAY_AUTOSEND_FILE` 两条）→ 「本轮」+ 收起的「第 1 轮」，`click:text=第 1 轮` 展开；没有子 Agent 的对话打开面板 → 「还没有子 Agent」；`hover:type=ModelPill` → 深度思考 / 温度 / 服务提示；`hover:#conversationUsageBadge` → 多行合计提示（生成中 / 结束后各一张）。合并标题栏的工具栏（徽标 + 按钮在系统按钮左边）用真实屏幕截图看一次（`grabWindow` 不含系统按钮）。

**S5 设置**：`settings:agents`（新 Toggle + 「最多 100 个」Stepper；关掉后 Stepper 消失）；`settings:secrets` → 点「模型」→ hover「高级」提示 → 点「高级」看 EngineEditor「起步并发 …」；`menu:model` 看在线模型副标题「起步并发 N」。

**S6 v1.9 托盘**
```
tray                          # 空闲：状态行、无「AI 在工作」行
traytip                       # 「Friday / 0 AI / 0 t/s」
（发送）wait:generating
wait:2500
tray                          # 第二行「N AI 在工作 · X t/s」
traytip
until:AgentRunner.isGenerating === false
wait:1500
traytip                       # 回到 0
```
Windows 上悬停真实托盘图标截一次 tooltip（屏幕截图）。

### 4.3 需要的调试钩子

> 归属见 PORT_V15_V19.md §3.13：1–7 由 core（DebugHooks / `AgentRunner::stop`）做，8、9、10 由 team 做，11 由 tree-tray 做（外加「`#` 开头的行跳过」，脚本可以写注释），12 由 team 做。
> 脚本按包拆成 `qt/tools/harness/*.txt`（PORT §7.3）。

Swift 已有、要移植到 Qt 核心 `DebugHooks`（语义同 `Agent/DebugHooks.swift` 文件头）：
1. `FRIDAY_SLOW_STOP=<秒>`：点停止后「正在停止」多留一会儿（`AgentRunner.stop()` 里，调试构建）。
2. `FRIDAY_AUTOSTOP=<条件>`：条件 `tool:<名>`（运行 1.5 秒后）/ `approval`（1 秒后）/ `after:<秒>` / `text:<秒>`（正文开始输出后，Swift 用 `LiveTextRegistry.hasText` 判断——Qt 没有 live 注册表，直接看正在写的消息有没有非空正文块）。
3. `FRIDAY_QUEUE="<条件>|文字|文字…"`、4. `FRIDAY_SWITCH="<条件>|engine:<id>|thinking:<档位>"`、5. `FRIDAY_AGENT_MESSAGE="<条件>|文字"`、6. `FRIDAY_STOP_AGENT="<条件>[|名字]"`、7. `FRIDAY_TREE_FOCUS="<条件>|<名字或「最深」>"`、8. `FRIDAY_STALL_LIMIT=<秒>`（看门狗，配 mock `sub_hang` 测「生成中 N 分钟没输出被停」的卡片文字）、9. `FRIDAY_LOCAL_SLOTS=<n>`（本地子 Agent 名额，测「在排队」）。

Qt 新增：
10. `FRIDAY_SLOW_AGENT_STOP=<秒>`：`stopAgent` 推迟这么久才真正取消，截子 Agent 停止键的旋转图标。
11. DebugHarness 步骤 `drag:<选择器>::<dx>,<dy>`（按下 → 带左键状态的移动 → 抬起；现有 `press / hoverat / release` 的移动不带按键状态）、`traytip`（打印 `FridayApp::trayToolTip()`）。
12. mock 扩展 `nest / background / report / sub_approval`（§4.1）。

### 4.4 验收
- 每次脚本结束「QML 警告 0 条」；没有 `未知的 SF Symbol`、`does not support customization`、`Binding loop`。
- 文案逐字对照第 1 节；浅 / 深色、FluentWinUI3 / Fusion / Windows 各一套截图；与 Swift 同一状态的截图（若有）比结构、顺序。
- 长对话（`FRIDAY_PERF_STRESS` 或 100+ 节点的 mock）下面板开关、拖宽、流式时主线程不卡（帧间隔看 `perf` 日志）。

---

## 5. 风险 / 待定

**风险**
1. **性能**：树每次重建要遍历整段对话所有块（含嵌套运行记录）。必须按消息缓存、只重算正在写的那条、合并到 ≤ 4–6 Hz；行差量更新；面板关着不建行。100 个运行中节点 = 100 个 BusyIndicator（Swift 同样是 ProgressView），必要时只给可见行转。
2. **对话区宽度动画**：MessageList 实例化了全部行（cacheBuffer 1e6），宽度每帧变都会重排全部 Markdown；与拖窗口的代价相同，先照 Swift 做动画，卡就退成「宽度一步到位、只滑面板 + 淡入」。
3. **详情页的「过程」**：MessageListModel 只暴露最后 visibleCount 条，树要能看任何一轮 → AgentTreeModel 自己提供 `blockIDsAt / blockAt / blockRevisionAt`（按完整路径下标）；块修订号逻辑建议抽成共用类，不然两套实现易漂移。
   **已定（PORT_V15_V19.md §4.2）**：这次先各写一份（tree-tray 可以照抄 MessageListModel 的实现），抽公共类记为后续；四个方法的签名见 PORT §3.11.4。
4. **标题栏**：面板和拖动条都在工具栏行以下；工具栏右端多了徽标 + 按钮，窄窗口时标题先截断、徽标可能被收掉（不进「»」菜单）；`AgentTreeButton` 收进「»」时菜单项文字是运行中的个数（Toolbar 溢出取 `text`），可接受或给 Toolbar 加「溢出时用 help」的规则。
   **已定（PORT_V15_V19.md §3.11.7）**：Toolbar 加 `overflowText`，树按钮在「»」里是「Agent 树（运行中 N）」，徽标收起时是一行禁用项；树按钮最右、最后才收。
5. **ListView footer**：spacing 不作用于 footer（手动上边距 18）；排队变化改 contentHeight → 跟随中自动贴底、没跟随时出「有新内容」，与 Swift 一致；空会话里排队不显示（Swift 原样，可能让人困惑，保持一致）。
6. **线程**：团队状态由子 Agent 工作线程产生，必须排队到主线程后再改 QObject 并发信号；`agentStatusText` / `speed` 等在主线程读。
7. **本地状态丢失**：`AgentControls.stopping`、输入框草稿是委托内状态；Loader / 委托重建会丢（Swift 的 @State 也随视图身份走）。若重建频繁，可改读核心「已请求停止」（`runs[id].stoppedBy != nil`，加一个 `isStopRequested(id)`）。
   **已定（PORT_V15_V19.md Q11）**：`AgentTeam.isStopRequested(id)` 由 team 提供，AgentControls 的停止键用 `stopping || AgentTeam.isStopRequested(eventID)`。
8. **自动打开的初值**：`deepRunning` 必须在 `setConversationID` 里同步算好，否则异步首算会被当成「由假变真」误开面板。
9. **工作中换本地模型**（ModelStatusView 不再禁用）：Qt 的进程内推理（inproc，当前 v1.5 分支正在做）切模型会卸载 / 加载，核心必须保证这一轮跑完才加载（`prepareModel` 生成中不动）；需要专门测「生成中选另一个本地模型 → 下一轮生效 → 排队消息等新模型加载好再发」。
10. **托盘**：GNOME 不显示 tooltip，只能靠菜单行；Windows tooltip 打开时文字不一定实时刷新；`liveAgents` 是全局（跨对话）计数，菜单行与当前对话的工具栏徽标数字可能不同（Swift 同样）。
11. **核心协调**：`AgentTeamTools::maxDepth` 删除 → `FridayApp::agentTeamMaxDepth` 与 AgentsSettingsView 插值要同一提交删；`ToolEvent::toVariant` / `ChatMessage::toVariant` 新键与 MessageListModel 角色要一起上。
    **已定（PORT_V15_V19.md §6）**：maxDepth 的删除与全部同步修改、toVariant 新键由骨架一次做；MessageListModel 的新角色由 chat-ui 加（读骨架写好的字段）。
12. **原先就有的差异**（不属于这五个版本，但碰的是同几个文件，建议决定是否顺手对齐）：d696a44（输入区底栏胶囊只显示图标和当前值：深度思考胶囊 Swift 显示「开启 / 关闭 / 档位」、提示「深度思考：这个模型只有开 / 关」「深度思考分档：低 / 中 / 高，越高越慢越贵」，Qt 仍是「深度思考 · 中」+ 下拉箭头；模型胶囊 Swift 只显示模型名）；d621837（显示名 / 接口地址 / 密钥名改为服务一级，EngineEditor 只剩模型名等；Qt 的 EngineEditor 仍有这三项，ServiceModelPicker / ServiceModelsSheet 的「高级」文案还写「改接口地址、并发数…」）。

**待定（需要用户 / 协调者拍板）**——**全部已定，见 PORT_V15_V19.md §4.2**：
- Q1 托盘图标在有 Agent 工作时要不要加强调色小圆点？（默认不加：Swift 的图标字形不变，信息在 tooltip + 菜单。）**已定：不加。**
- Q2 面板开关时对话区宽度做动画还是一步到位？（默认照 Swift 做动画，卡再退。）**已定：先动画，达不到 PORT §7.5-F 的门槛就退。**
- Q3 面板左边的分隔线要不要像侧边栏那样穿过工具栏行？（默认不穿：会从工具栏右端的徽标 / 按钮中间穿过，也会碰标题栏命中测试。）**已定：不穿。**
- Q4 托盘 tooltip 第一行保留「Friday」？（默认保留，后两行 = Swift 标签的两行。）**已定：保留。**
- Q5 Qt 专用钩子 `FRIDAY_SLOW_AGENT_STOP`、DebugHarness `drag / traytip`、mock 扩展是否接受？**已定：接受（PORT §2.4 Q12）。**
- Q6 §5-12 的原有差异是否在这次一起改？**已定：都改；d696a44 归 chat-ui，d621837 归 settings（PORT §1.6、§1.7、§3.12）。**
