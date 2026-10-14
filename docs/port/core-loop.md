# 核心 Agent 循环（主 Agent）移植规格：Swift v1.5–v1.9 → Qt

> 本文件是实现者照着做的契约。Swift 是行为标准（origin/main = `e2f8f41`），Qt 只在外观上不同。
> 行号一律是 **origin/main** 上的文件行号（`git show origin/main:<路径>`）；Qt 行号是当前工作树（2026-10-02）的。
> 中文字符串凡是用「」或代码块给出的，都是**逐字**的，包括全角标点、空格和换行。
>
> **附录说明**：本文是 `qt/docs/PORT_V15_V19.md`（总契约）的附录。接口、归属、决定以总契约为准；下文标「**已由 PORT_V15_V19.md §x 取代**」的段落不再生效，
> 其余（行为、逐字文案、Swift 出处、测试向量）照做。

---

## 0. 范围、基线、分工

### 0.1 版本 → 提交

| 版本 | 合并提交 | 功能提交 | 本区相关的 Swift 文件 |
|---|---|---|---|
| v1.5 停止状态 | e71cf89 | a9a6d27 | AgentRunner、AgentTools（runShell）、ChatModels（stopped / stoppedByUser / markStopped）、LiveText（hasText）、DebugHooks（FRIDAY_AUTOSTOP、FRIDAY_SLOW_STOP）、SelfTests（stopState） |
| v1.6 工作中补充 | 76534ff | 9536c33、1b760a4 | AgentRunner、QueuedInput、ChatEngine / OpenAIChatEngine / LocalMLXEngine（interjection）、Vendor ChatSession、ChatModels（interjected）、DebugHooks（FRIDAY_QUEUE、FRIDAY_SWITCH）、SelfTests（queueState） |
| v1.7 父子协作 | e3aa578 | 7f53e44、33aff41 | AgentRunner（team、takeInterjection 的汇报、drainBackgroundAgents、cancelTeam、updateEvent、toolResultText、waitingWithoutSlot）、ToolTypes（runsInBackground）、ChatModels（fromAgent）、TurnCheck（AgentLog 里的 agent_message）、QueuedInput（from）、DebugHooks（FRIDAY_AGENT_MESSAGE、FRIDAY_STOP_AGENT）、OpenAIChatEngine（RequestGate：**另一区**） |
| v1.8 层数不限 / 活性 / 用量 | f1f6d58 | 95d4df0、a3b7284、2084be2、9096118、e672b61 | AgentRunner（guidance(team:)、原地打转、activity、SubAgentEnded、步数提醒、mainLiveSpeed、estimateTokens）、ChatModels（AgentUsage）、DebugHooks（FRIDAY_TREE_FOCUS 等） |
| v1.9 菜单栏两行 | e2f8f41 | 5103122 | AgentRunner（liveAgents / liveSpeed / meter）、FridayApp（MenuBarLabel：**界面区**，本文给出数据与 speedText） |

### 0.2 Qt 现状（基线 = 44116fb 的功能对齐）

在 `qt/src` 里 grep `stopped|isStopping|markStopped|interjec|QueuedInput|fromAgent|stoppedByUser|runsInBackground|pausedQueue|turnMessage`：
除了 `OpenAIChatEngine::StreamState::stopped`（与本功能无关）之外**一个都没有**。v1.5–v1.9 在 Qt 里全部要新做。已经有、可以直接用的：

- `MessageBlock::markInterrupted` 已经是「先按原状态定原因、再改状态」（`ChatModels.cpp:387-403`）——v1.5 修的那个 Swift bug Qt 当初就修了，**不用再改**。
- `ProcessRunner::run` 在 Windows 上用作业对象整棵进程树一起结束（`ProcessRunner.cpp:317-330`）——v1.5 的「连同子进程结束」在 Windows 已满足；Linux / macOS 只结束 shell 本身，要补（§2.9）。
- `ConversationStore` 读存档时对**每条**消息做 `markInterrupted`（`ConversationStore.cpp:217-230`，比 Swift 只看 live 统计的范围大，保留）。
- 线程原语：`Worker::run / spawn`、`CancelToken`（含 `makeChild`、`onCancel`）、`Promise<T>`、`runOnMain / runOnMainSync`、`RefreshThrottle`。本规格全部用这些表达 Swift 的 `Task` / `await` / continuation，不引入 QPromise / 协程。

### 0.3 分工边界

> **已由 PORT_V15_V19.md §5 取代**（五个工作包与文件归属）。下表只作背景。

| 区 | 内容 |
|---|---|
| **本区（核心循环）** | `AgentRunner`（主 Agent 部分）、`QueuedInput`、`ChatEngine` / `OpenAIChatEngine` / `LocalLlamaEngine` 的 interjection、`ChatModels` 的新字段与 `markStopped` / `AgentUsage`（数据结构）、`ToolTypes` 的 `runsInBackground`、`TurnCheck` 的 AgentLog、`AgentTools::runShell` + `ProcessRunner`、`DebugHooks`、`ChatViewModel` / `MessageListModel` 的 C++ 部分（给界面的数据契约） |
| 子 Agent 区 | `AgentTeamState`（Swift AgentTeamControl.swift）、`SubAgentEnded` / 看门狗 / 期限 / 半成品（AgentLiveness.swift）、`SubAgentRunner`（后台、message_agent / stop_agent / wait_agents / report_to_parent、checkDispatch、ancestryNote、usage 统计）、`SubAgentScheduler`（让出 / 拿回名额、深的先走）、`AgentProfiles`（新工具定义与说明文字）、`ToolRegistry` 的 `mainAgentSpecs` / `guidance(team:)` / report_to_parent 预检、`AppSettings.subAgentTurnLimit` |
| 界面区 | 全部 QML（停止键旋转图标、排队气泡、汇报卡片、Agent 树面板、工具栏合计、子 Agent 卡片控件、托盘两行）、图标映射 |
| 在线限流区 | `RequestGate`（33aff41 的 429 自适应并发闸门）——只在 `OpenAIChatEngine` 的传输层，和本区的 interjection 改动不重叠（§2.6.4） |

### 0.4 线程约定（沿用 ARCHITECTURE.md §3.4，本文新增的不变量用 ★ 标出）

- 主线程：`send / submit / removeQueued / resumeQueue / stop / resolveApproval / set* / prepareModel`、`m_queuedInputs`、`m_pausedQueues`、`m_stoppedConversations`、`m_turnEngineKey`、`m_turnThinking`、meter。
- 回合线程（`Worker::run(m_currentTask, …)`）：`runTurn`、`consume`、`takeInterjection`、`drainBackgroundAgents`、`verifyIfNeeded`、`recover`、`finish` 的前半段；流式统计（`m_turn*`）只在这里读写。
- ★ `takeInterjection` 由引擎在**回合线程**里回调（`OpenAIChatEngine::run` 的工具循环末尾），读主线程状态一律 `runOnMainSync`，改会话一律 `runOnMain`（FIFO，后面排的更新一定在它之后执行）。
- ★ `m_turnMessageIDs`（这一轮写过的助手消息）和 `m_streamMessageID` 一起由 `m_streamMutex` 保护：子 Agent 线程的 `updateEvent` 要读它。
- ★ `isStopping()` 任意线程可读（原子量），子 Agent 线程大量用到。
- ★ 主线程里会发 `generatingChanged` 的地方（`setGenerating`）**必须是收尾闭包里最后改状态的那一步之一**：直连槽可能同步重入 `send()`（现有用例 `stopAfterReentrantSend`），所以这一轮的状态（`isStopping`、`turnEngineKey`、`turnThinking`、暂停队列、`stoppedConversations`、`team.reset`）都要在它**之前**清 / 设好（§2.2）。

---

## 1. 行为规格（按版本）

### 1.1 v1.5 停止状态（a9a6d27）

#### 1.1.1 状态

| 名字（Swift） | 位置 | 含义 | 生命周期 |
|---|---|---|---|
| `isStopping: Bool`（`private(set)`，可观察） | AgentRunner.swift:77-78 | 用户点了停止、这一轮还没真正收尾。界面：停止键换旋转图标并禁用，消息底栏「正在停止…」 | `stop()` 置 true；`finish()` 末尾置 false |
| `stoppedConversations: Set<ID>` | :79-80 | 上一轮被停止的会话：下一轮先按存档重新同步内存里的会话 | `finish()`（stopped 时）insert；`send()` remove |
| `ChatMessage.stoppedByUser: Bool?` | ChatModels.swift:223-224 | 这条助手消息是点停止结束的 | `stop()` 立即写 true（当前正在写的那条）；`finish()` 再写一次 true |
| `ToolEvent.stopped: Bool?` | ChatModels.swift:73-74 | 这个调用没跑完是因为停止（状态记 `failed`），界面灰色「已停止」，回灌给模型时说明不要重试 | 见下 |

#### 1.1.2 `stop()`（AgentRunner.swift:908-932）

1. `guard isGenerating, !isStopping else { return }`——**只认第一次**；不在生成时什么也不做（⌘.、菜单再按都无效）。
2. `isStopping = true`
3. `updateMessage { $0.stoppedByUser = true }`（正在写的那条助手消息）
4. 调试构建且设了 `FRIDAY_SLOW_STOP=<秒>`：起一个任务，睡这么多秒后 `cancelTeam()` + 取消回合任务；**审批立刻**全部 `resume(false)` 并清空；然后 return（「正在停止…」多停留一会儿，好截图）。
5. 否则：`cancelTeam()`（v1.7 加，见 §1.3.4）→ `currentTask?.cancel()` → 所有等确认的 continuation `resume(returning: false)`，清空 `approvals`。
   等确认的调用按「停止」收尾、**不算用户拒绝**——`dispatch` 里看 `isStopping`（§1.1.3）。

#### 1.1.3 工具调度里的「已停止」（AgentRunner.swift:1078-1143、1519-1527）

- 审批等完之后（:1089-1099）：
  ```text
  guard approved, !Task.isCancelled, !isStopping else {
      if isStopping || Task.isCancelled { status = .failed; stopped = true; result = "等待确认时被停止，没有执行" }
      else { status = .denied }
      sink.replace(event); return toolResultText(event)
  }
  ```
- 执行抛错时（:1129-1143），按顺序：
  1. （v1.8）`SubAgentEnded` 且 `stoppedBy != nil` → `result = ended.cardText`、`failed`、`stopped = true`（§1.4.3）。
  2. `isStopping || Task.isCancelled || CancellationError || URLError.cancelled` → `result = "已停止"`、`failed`、`stopped = true`。**不记诊断日志**。
  3. 其余 → `result = error.localizedDescription`、`failed`，记诊断日志「工具 <name> 失败」（原有）。
- `replaceKeepingTranscript`（主 Agent :1519-1527，子 Agent 同）多复制一个字段：`stored.stopped = event.stopped`。

#### 1.1.4 回合收尾 `finish()`（AgentRunner.swift:1430-1485，完整顺序）

```text
finishMainPass()
let stopped = isStopping
if stopped {
    updateMessage { $0.stoppedByUser = true; MessageBlock.markStopped(in: &$0.blocks) }          // 当前这条
    for id in turnMessageIDs.dropLast() { store.updateMessage(id) { markStopped(&$0.blocks) } }  // v1.7：这一轮更早的几条（后台子 Agent 的卡片可能在那里）
    stoppedConversations.insert(streamConversationID)
}
if turnGeneratedTokens > 0 || turnPromptTokens > 0 { publishFinalStats(live: false); persistCache(…) }
else { updateMessage { $0.stats = nil } }
updateMessage { if blocks.isEmpty && error == nil && stoppedByUser != true { blocks = [.text("（没有输出）")] } }
store.commitLive(); store.touch(id); 有临时标题就 generateTitle(id)
generatingConversationID = nil
prepareModel(for: selectedID)
team.reset(conversationID)                                   // v1.7
if stopped && queuedInputs[id] 非空 { pausedQueues.insert(id) } // v1.6
streamConversationID = nil; streamMessageID = nil; currentTask = nil
isStopping = false; turnEngineKey = nil; turnThinking = nil; turnMessageIDs = []   // v1.6
store.saveNow()
if !queuedInputs.isEmpty { scheduleQueueDelivery() }        // v1.6
```

要点：
- **以前**空回复写「（已停止）」；**现在**被停止的空回复**不写任何块**（底栏显示「已停止」标签），没被停止的空回复写「（没有输出）」。
- `MessageBlock.markStopped`（ChatModels.swift:159-174）：递归进 `transcript`，凡 `running` 或 `awaitingApproval`：
  `result = (原来是 awaitingApproval ? "等待确认时被停止，没有执行" : "已停止")`、`status = .failed`、`stopped = true`、`queued = nil`。已完成的调用不动（`stopped` 保持 nil）。
  **先按原状态定 result、再改 status**（和 markInterrupted 一样的顺序）。

#### 1.1.5 下一轮：按存档重新同步（AgentRunner.swift:565-572、601-603）

内存里的会话没有「被中途停止」的说明和停掉的工具结果（在线引擎历史里可能还有半截回答、甚至没有结果的 tool_calls），所以：

```text
let resync = foldedTurns > 0 || stoppedConversations.remove(conversationID) != nil   // 注意 || 短路：折叠了就不 remove，下一轮还会再同步一次（无害，照搬）
let resyncedLocal = resync && replacing == nil ? sessions[id]?.engine as? LocalMLXEngine : nil
if replacing != nil || (resync && resyncedLocal == nil) { sessions[id] = nil }       // 在线引擎：丢会话，按存档重建
… Task 里：if let resyncedLocal, await !resyncedLocal.replaceHistory(historyMessages(history)) { sessions[id] = nil }
```

#### 1.1.6 停止时不再自动恢复续写（AgentRunner.swift:628-636）

回合任务里两处 `catch is CancellationError` 改成 `catch where error is CancellationError || isStopping || Task.isCancelled`：在线引擎被中止时抛的是 `URLError.cancelled`，不能当故障去 `recover`（那样会接着生成）。外层（建引擎失败）同理，停止时也不写 `error`。

#### 1.1.7 回灌给模型的文字

- `toolResultText`（AgentRunner.swift:996-1014，最终版，含 v1.7 / v1.8 的分支，**顺序就是判断顺序**）：
  1. `stopped == true && AgentTeamTools.isTeamTool(name)`（`delegate_task` / `run_parallel_tasks`）：
     - `result == "已停止"` 或空 → 「这个子 Agent 被停止了，没做完。不要重新派发同样的任务；按已有信息继续，或告诉用户需要什么。」
     - 否则 → `"这个子 Agent " + result + "\n\n" + SubAgentEnded.advice(forCardText: result)`（§1.4.3）
  2. `stopped == true` → 「用户停止了这一轮，这个调用没有完成（结果未知，可能执行了一部分）。不要自动重试，等用户下一步的指示。」
  3. `status` 是 `succeeded` **或 `running`**（v1.7：后台派发返回时卡片仍是 running）→ `ToolOutput(result, media).modelText`
  4. `denied` → 原文案不变；其余 → 原「工具执行失败：…」不变。
- `historyMessages`（AgentRunner.swift:949-990）：助手消息遍历完块之后，`if message.stoppedByUser == true { text += (text.isEmpty ? "" : "\n\n") + stoppedNote }`，再按原规则 `if !text.isEmpty { append(.assistant(text)) }`。
  - `stoppedNote` = 「（这一轮被用户中途停止了，上面的回答和操作可能不完整。）」（:994）
  - 结果：被停的空回复也会留一条助手消息（不会出现两条用户消息相连）；有工具块时说明接在最后一个工具结果之后的那条助手消息里。

#### 1.1.8 run_shell 连同子进程结束（AgentTools.swift:170-240）

- 启动前就被停止：不再启动（启动与停止用同一把锁）。
- 停止时：先给 shell 的直接子进程发 SIGTERM（`pkill -TERM -P <pid>`）并 `terminate()` shell；2 秒后还在的：子进程 SIGKILL、shell SIGKILL。
- 进程结束后 `if Task.isCancelled { throw CancellationError() }`（即使命令恰好自己跑完，只要回合已取消，也按「已停止」处理）。
- 60 秒超时、输出格式（「退出码 N」/「（超时或被终止）」+ 输出或「（无输出）」）不变。

#### 1.1.9 其他

- `verifyIfNeeded` 的 guard 加 `!isStopping`（AgentRunner.swift:1217）。
- `LiveTextRegistry.hasText`（LiveText.swift:26）只给调试钩子 `text:` 条件用；Qt 没有 live 注册表，§4.4 用消息块判断即可。
- 子 Agent 侧（子 Agent 区负责，这里列出以便核对整体效果）：排队时被停 → 拿到名额也不开跑（`if Task.isCancelled || isStopping { throw CancellationError() }`）；并行子卡片被停 → `result = "已停止"`、`stopped = true`、`queued = nil`，不记「子 Agent「X」失败」日志。

#### 1.1.10 界面文案（界面区实现，本区提供 `isStopping` / `stoppedByUser` / `stopped`）

| 位置（Swift） | 文案 / 规则 |
|---|---|
| ComposerView.swift:381-400 停止键 | 生成中是停止键（stop.fill，红色）；`isStopping` 时换成**旋转的刷新图标**（`arrow.triangle.2.circlepath`，顺时针 0.9 s 一圈，次要文字色），按钮禁用、不接收点击；help：`isStopping ? "正在停止…" : "停止生成 (⌘.)"`（Qt 用 `App.shortcutModifier`） |
| SpinningRefreshIcon.swift | 无障碍标签「正在停止」 |
| MessageRow.swift:131 | 正在写的消息上打字动画：`isStreaming && !isStopping && (blocks 空 \|\| 最后是已返回的工具)` |
| MessageRow.swift:154-196 底栏 | 显示条件：`stats != nil \|\| (isStreaming && isStopping) \|\| (!isStreaming && (!blocks.isEmpty \|\| stoppedByUser == true))`。内容：`isStreaming && isStopping` → 旋转图标(10pt) +「正在停止…」（次要色），**代替**统计；否则统计。`!isStreaming && stoppedByUser` → 标签「已停止」（stop.circle，次要色），help「这一轮是你点停止结束的，回答和操作可能不完整；接着发消息时 Agent 会知道这里被打断过」 |
| MessageRow.swift:565-592 工具卡 | `stopped == true` 时色调灰色；状态位 `failed && stopped` 显示「已停止」（stop.circle，次要色）而不是红色「失败」 |
| FridayApp 菜单 | 「停止生成」照旧（生成中可用；停止中再点无效） |

#### 1.1.11 持久化

- `ToolEvent.stopped`：JSON 键 `"stopped"`，只写 `true`（nil 不写键）。
- `ChatMessage.stoppedByUser`：键 `"stoppedByUser"`，只写 `true`。
- 旧存档没有这两个键 → nil。

---

### 1.2 v1.6 工作中补充 + 工作中换模型（9536c33、1b760a4）

#### 1.2.1 `QueuedInput`（QueuedInput.swift，全文 12 行）

```swift
struct QueuedInput: Identifiable, Equatable {
    let id = UUID()
    var text: String          // 已 trim
    var attachments: [URL]    // 还没导入媒体目录；交给 Agent / 发出时才 importAttachments
    var createdAt = Date()
    var from: String?         // v1.7：nil = 用户；否则父 Agent 名字（只用于子 Agent 的队列）
}
```
1b760a4 删掉了 9536c33 的 `merge`（不再合成一条）。只在内存里，不落盘（退出即丢，Swift 同）。

#### 1.2.2 状态（AgentRunner.swift:82-93）

| 名字 | 可观察 | 含义 |
|---|---|---|
| `queuedInputs: [ConvID: [QueuedInput]]` | 是 | 每段对话排队的消息 |
| `pausedQueues: Set<ConvID>` | 是 | 上一轮被用户停止：排队的不自动发，等用户点「发送」 |
| `turnEngineKey: String?`、`turnThinking: ThinkingLevel?` | 是 | 这一轮开始时定下的大脑 / 思考强度（界面据此标「下一轮生效」） |
| `turnEngineName: String?` | 否 | 统计栏的引擎名（`"\(title) · \(model)"`，本地 nil）：工作中换了模型，统计仍显示实际在跑的 |
| `turnExpectsThinking: Bool` | 否 | 流式切分思考用的 assumeThinking，这一轮固定 |
| `turnMessageIDs: [MessageID]` | 否 | 这一轮写过的助手消息（插入补充后一轮拆成几条） |
| `queueDelivery: Task?` | 否 | 排队投递的轮询任务 |

#### 1.2.3 `submit`（输入框、空状态建议都走它；AgentRunner.swift:654-670）

```text
prompt = text.trim
guard !prompt.isEmpty || !attachments.isEmpty
if !isGenerating && canSend(id) && (queuedInputs[id] 为空) { send(prompt, attachments, id, replacing); return }
guard replacing == nil else { return }          // 「重新输入」要等空闲：忙的时候直接忽略（不排队、不提示）
queuedInputs[id].append(QueuedInput(text: prompt, attachments: attachments))
pausedQueues.remove(id)                          // 新发一条就解除暂停（连同之前暂停的那几条一起）
if !isGenerating { scheduleQueueDelivery() }
if generatingConversationID == id { team.wake(mainKey) }   // v1.7：主 Agent 在等后台子 Agent 时叫醒它
```

`removeQueued(itemID, id)`（:672-679）：删掉那条；队列空了就删键并 `pausedQueues.remove(id)`。
`resumeQueue(id)`（:681-685）：`pausedQueues.remove(id)`；`if !isGenerating { scheduleQueueDelivery() }`。

#### 1.2.4 发一轮：一条或几条（AgentRunner.swift:524-640）

- 公开的 `send(text, attachments, id, replacing)` 只是转调私有的 `send([(text, attachments)], id, replacing)`。
- 私有 `send(inputs:)`：每条 trim、丢掉空的；`guard !inputs.isEmpty, replacing == nil || inputs.count == 1, !isGenerating, canSend(id)`。
- 每条一个用户消息（附件先 `importAttachments`，失败记日志「导入附件失败」`"\(path)\n\(error)"`）。
- `taskNote` 只附在**最后一条**用户消息上。
- 新会话的临时标题取**第一条**：`plainText` 为空就用它第一个附件的文件名，再没有就「附件」，前 24 字。
- 「重新输入」分叉时只有一条；否则全部 `append`，再 `append` 空助手消息。
- 这一轮的快照（:585-591，在 `generatingConversationID = id` 之后、开始流式之前）：
  `turnEngineKey = engineKey(id)`、`turnThinking = thinkingLevel(id)`、`turnEngineName = engineConfig(id).map { "\($0.title) · \($0.model)" }`、
  `turnExpectsThinking = streamEngineIsLocal && thinkingLevel(id).isOn && ModelPreset.preset(models.loadedModelID ?? settings.modelID).supportsThinking`、
  `turnMessageIDs = [assistant.id]`。
- 任务里（:604-627）：每条用户消息各自 `describeAttachments(supportsVision:)` → `.user(promptText(described), images/videos 仅 supportsVision 时)`；
  **只有一条**时 `engine.stream(prompt:images:videos:)`，**多条**时 `engine.stream(messages: outgoing)`。
  自愈 `recover(...)` 用**最后一条**的 `(content, images, videos)` 重发（前面几条已经在历史里）。
- 顺序：`continueWithLoadedTools`（Qt 没有）→ `verifyIfNeeded` → `drainBackgroundAgents`（v1.7）。

#### 1.2.5 `engineKey` / `pendingTurnChanges`（AgentRunner.swift:1299-1308）

- `engineKey(id)` = 在线：`"engine:" + config.id`；本地：`"local:" + localModelID(id)`（注意：没单独选过引擎的对话跟随 `settings.defaultEngineID`，别的对话改了默认引擎也会让正在跑的这段显示「下一轮生效」——照搬）。
- `pendingTurnChanges(id)`：`generatingConversationID != id` → `(false, false)`；否则 `(turnEngineKey != engineKey(id), turnThinking != thinkingLevel(id))`（比原始档位，不比折算后的生效档位）。
- `expectsThinking` 改为直接读 `turnExpectsThinking`；`engineNameForStats` 改为 `turnEngineName`。
- 工作中换模型 / 思考：`setEngine` / `setLocalModel` / `setThinking` 本身没改——它们照样 `sessions[id] = nil`，但正在跑的这一轮手里拿着引擎，不受影响；
  `prepareModel` 有 `!isGenerating` 守卫，新的本地模型要等这一轮结束（`finish` 里 `prepareModel(selected)`、或排队投递里 `prepareModel(target)`）才加载。
  **已知不一致（照搬）**：这一轮里后续的 `configure(engine)`（`verifyIfNeeded`、`drainBackgroundAgents` 里）会把新的思考开关 / 温度 / 工具同步进**旧引擎**；模型本身不变。

#### 1.2.6 interjection 钩子（引擎层）

- `ChatEngine.interjection: (@Sendable () async -> [EngineMessage])?`（ChatEngine.swift:59-62）：一段生成的工具调用**全部执行完**、发起下一段请求**之前**调用一次，返回的消息接在工具结果后面。只有主 Agent 的会话设它（子 Agent 的由子 Agent 区设自己的）。
- `configure()`（AgentRunner.swift:327-332）每次都设：`engine.interjection = { await takeInterjection(for: conversationID, supportsVision: engine.supportsVision) }`。
- 在线（OpenAIChatEngine.swift:223-229）：工具结果都 append 进 history 之后：`try Task.checkCancellation()`，然后 `for m in await interjection() { history.append(try await userObject(m)) }`。
- 本地 MLX（LocalMLXEngine.swift:23-30 + Vendor ChatSession.swift:343-357、1631-1639）：同一时机把 `Interjection(text, images, videos)` 作为 `.user(...)` 追加进 `pendingMessages`；`!Task.isCancelled` 才调。
- `OpenAIChatEngine.userObject(_:)`（:124-141，新）：用户消息统一处理——图片按 `convert`；**视觉模型**的视频每段抽 6 帧当图片插在文字前，文字追加「\n（视频已按时间顺序抽帧附在图片里）」；不能看图的模型只有文字。
  `stream(prompt:)` 和（1b760a4 起）`stream(messages:)` 里的**用户消息**都走它。

#### 1.2.7 `takeInterjection`（AgentRunner.swift:687-735，最终版含 v1.7 的汇报）

```text
guard streamConversationID == id, isGenerating, !isStopping else { return [] }
items   = pausedQueues.contains(id) ? [] : queuedInputs.removeValue(forKey: id) ?? []
notices = team.takeNotices(for: mainKey).map { ChatMessage(role: .user, text: noticeText($0)) + fromAgent = $0.agentName }   // v1.7
guard !items.isEmpty || !notices.isEmpty else { return [] }
users = notices + items.map { ChatMessage(.user, text: $0.text, attachments: importAttachments($0.attachments)) + interjected = true }   // 汇报在前，用户的话在后
// 当前这条助手消息收尾
finishMainPass()
if turnGeneratedTokens > 0 || turnPromptTokens > 0 { publishFinalStats(live: false) } else { updateMessage { $0.stats = nil } }
store.commitLive()
if contextAssistEnabled { users.last.taskNote = TaskState.promptNote(for: conversation(id).messages) }   // 用当前路径（含刚收尾的那条）
assistant = ChatMessage(.assistant, "")
store.update(id) { for u in users { append(u) }; append(assistant) }
streamMessageID = assistant.id; turnMessageIDs.append(assistant.id); store.beginLive(assistant.id)
resetStats(); beginPass()
// 交给模型的内容与重建历史时逐字一致
return users.map { describeAttachments(of: $0, supportsVision:) → .user(promptText(described), images/videos 仅 supportsVision) }
```

- 界面效果：当前回复收尾（统计定稿），下面每条补充一个用户气泡（标「工作中补充」）或汇报卡片，再开一条新的助手消息接着写。存档顺序 = 交给模型的顺序。
- `promptText`（:781-798）：`interjected == true` 时在**正文最前面**加 `interjectionNote`，然后才处理附件（所以只有附件的补充消息不会出现「（请看附件）」，而是「note + \n\n[用户附带的文件]\n…」），最后附 taskNote（未折叠时）。
  `interjectionNote` = 「（用户在你工作时补充：和原来的任务一起考虑，原来的任务照常做完；有冲突以这条为准。）\n」（末尾一个 `\n`，:992）。
- `fromAgent` 的汇报消息走普通 `promptText`（不加 note）；如果这一批没有用户补充，taskNote 会附在最后一条汇报上（照搬）。

#### 1.2.8 排队投递 `scheduleQueueDelivery`（AgentRunner.swift:737-767）

```text
queueDelivery?.cancel()
queueDelivery = Task {
  var preparing: ID? = nil
  for _ in 0..<2400 {                        // 每 0.5 s 一次，最多 20 分钟（模型加载 / 下载）
    guard !cancelled, !isGenerating else { return }
    ready  = queuedInputs 里非空且不在 pausedQueues 的
    target = selectedID（在 ready 里时） ?? ready 里「第一条 createdAt 最早」的那段
    guard target else { return }
    guard conversation(target) 存在 else { queuedInputs[target] = nil; continue }    // 对话已删：丢掉，不睡，接着看下一段
    if canSend(target) { items = queuedInputs.removeValue(target); send(items 分条, in: target); return }   // 分条发出，Agent 在同一轮一起回应
    if preparing != target { preparing = target; prepareModel(for: target) }       // 每个目标只调一次
    sleep 500 ms
  }
}
```

调用点：`submit`（不在生成时）、`resumeQueue`（不在生成时）、`finish` 末尾（队列非空时）。第一轮检查**异步**进行（不在调用者的调用栈里同步发出）。

#### 1.2.9 验证检查点看整轮（AgentRunner.swift:1215-1230）

`blocks = turnMessageIDs.compactMap { store.message($0) }.flatMap(\.blocks)`，按顺序交给 `TurnCheck.unverifiedWrites`。
「用户这一轮说了不用运行」看的仍是**当前路径上最后一条 role == user 的消息**（可能是一条工作中补充，甚至是一条子 Agent 汇报——照搬）。

#### 1.2.10 界面文案（界面区实现）

| 位置 | 文案 / 规则 |
|---|---|
| ChatView.swift:135-197 排队气泡（接在消息列表最后） | 每条：靠右；有附件时一行 `paperclip` 图标 + 文件名（用「、」连接，单行、中间截断，caption 次要色）；文字用 Markdown、**虚线**边框（强调色 45%，dash 5/4）、背景强调色 8%；下面「撤回」按钮（`arrow.uturn.backward`，help「撤回这条，不交给 Agent」）。末尾一行状态：图标 `paused ? pause.circle : clock.arrow.circlepath`，颜色 `paused ? 橙色 : 次要`，文字按顺序判断：`paused` →「已暂停：上一轮被你停止了」（后面接一个小号「发送」按钮 → `resumeQueue`）；`generatingConversationID == 这段` →「排队中：Agent 做完当前这一步就会看到」；`isGenerating` →「排队中：另一段对话结束后发出」；`!canSend(这段)` →「排队中：模型就绪后发出」；否则「马上发出…」 |
| ComposerView.swift:44-48 canSend | `hasContent && (isGenerating ? editing == nil : runner.canSend(id))`；Return 键在 canSend 时触发 onSend（== submit） |
| ComposerView.swift:355-376 生成中的按钮 | 停止键 +（canSend 时）旁边一个发送键（arrow.up，强调色），help：`generatingConversationID == 这段 ? "补充给 Agent：做完当前这一步就交给它 (Return)" : "排队：另一段对话结束后发出 (Return)"` |
| ComposerView.swift:150-158 | `pendingTurnChanges` 任一为真：橙色小标签「下一轮生效」（clock.arrow.circlepath），help：「Agent 这一轮还在用原来的」+（model ?「模型」:「」）+（model && thinking ?「和」:「」）+（thinking ?「思考强度」:「」）+「，下一轮自动换成现在选的」 |
| ComposerView.swift:417-421 占位 | 顺序：editing →「修改后发送，会从这条消息处分叉…」；这段在生成 →「Agent 正在工作，可以继续补充——做完当前这一步就交给它…」；别的在生成 →「另一段对话正在生成，现在发出的消息会排队…」；其余同原来 |
| ModelStatusView.swift:51-52 | 模型胶囊**生成中不再禁用**（这一轮照旧、下一轮生效）。Qt `ModelStatusView.qml:59` 的 `enabled: !vm.isGenerating` 要去掉 |
| ChatView.swift:60-64 空状态 | 建议可点：`canSend(id) \|\| isGenerating`；点了走 `submit` |
| MessageRow.swift:73-79 | `interjected == true` 的用户气泡上方小字「工作中补充」（text.bubble，次要色），help「Agent 工作时发的：做完当时那一步就交给了它，它接着往下做」 |

#### 1.2.11 持久化

`ChatMessage.interjected`：键 `"interjected"`，只写 `true`。SelfTests 断言旧存档 `{"id":"2F1E7C1A-0000-4000-8000-000000000001","role":"user","blocks":[],"createdAt":0}` 读得出、`interjected == nil`。

---

### 1.3 v1.7 父子协作：主循环侧（7f53e44、33aff41）

子 Agent 区负责 `AgentTeamState` 与各团队工具；主循环要做的是下面这些挂钩。

#### 1.3.1 `team`

`let team = AgentTeamState()`（AgentRunner.swift:94-95），runner 持有。`AgentTeamState.mainKey = 00000000-0000-0000-0000-00000000000A`（AgentTeamControl.swift:47）。

#### 1.3.2 汇报接进主 Agent（takeInterjection，§1.2.7）

汇报 / 完成 / 失败 / 被停止的通知（`Notice`）作为 `role: .user` 的消息存档并交给模型，`fromAgent = notice.agentName`，文字是 `noticeText(notice)`（AgentTeamControl.swift:450-465）：

```text
who = "子 Agent「\(name)」（id：\(AgentLog.shortID(agentID))）"         // shortID = 大写 UUID 前 8 位再转小写
.report   → "[\(who)中途汇报，它还在继续]\n\(text)"
.finished → "[\(who)完成了]\n\(text)"
.failed   → "[\(who)失败了]\n\(text)"
.stopped  → parts = text.split("\n", maxSplits: 1, 保留空段); rest = parts[1].trim ?? ""
            "[" + who + (parts.first ?? text) + "]" + (rest.isEmpty ? "" : "\n" + rest) + "\n" + SubAgentEnded.advice(forCardText: text)
noticeTitle(text): 含「中途汇报」→「中途汇报」；含「完成了]」→「完成」；含「失败了]」→「失败」；否则「已停止」
```
界面（MessageRow.swift:34、43-68）：`fromAgent != nil` 的用户消息画成**靠左的灰色汇报卡片**：圆形底的 `person.2.badge.gearshape` 图标，标题「子 Agent「<fromAgent>」<noticeTitle(第一行)>」（caption 加粗、次要色），正文 = plainText 第一个 `\n` 之后的部分（Markdown），卡片最宽 620、圆角 14。持久化键 `"fromAgent"`（字符串，nil 不写）。

#### 1.3.3 `drainBackgroundAgents`（AgentRunner.swift:1194-1213）

主 Agent 想结束这一轮时还有后台子 Agent 在跑：等它们的汇报和结果，交给它整合后再结束。

```text
key = mainKey
while !Task.isCancelled && !isStopping {
    hasNotices   = team.notices[key] 非空
    hasUserInput = queuedInputs[id] 非空 && !pausedQueues.contains(id)
    if !hasNotices {
        guard team.backgroundChildren(of: key, in: id) 非空 else { return }
        if !hasUserInput { await team.waitForNotice(key); continue }    // 被 post / wake / 取消 叫醒
    }
    messages = await takeInterjection(id, engine.supportsVision)
    guard !messages.isEmpty else { continue }
    configure(engine, id); finishMainPass()
    try await consume(engine.stream(messages: messages))
}
```
没有后台子 Agent 的轮次第一次循环就 return。

#### 1.3.4 停止时连子 Agent 一起停

- `cancelTeam()`（AgentRunner.swift:934-939）：`guard let id = generatingConversationID`；`team.runs` 里 `conversationID == id` 的全部 `task.cancel()`；`team.wake(mainKey)`。`stop()` 在取消回合任务**之前**调它（slow-stop 时在延时后调）。
- `finish()`：`team.reset(conversationID)`（取消这段对话剩下的 run 并注销、叫醒所有等待、清空**全部**通知、清 `launched[id]`）；被停止时 `markStopped` 这一轮**所有**助手消息（后台子 Agent 的卡片可能在更早的一条里，§1.1.4）。

#### 1.3.5 后台派发的卡片

`ToolOutput.runsInBackground`（ToolTypes.swift:154-164，构造参数默认 false）。`dispatch` 里 `event.status = output.runsInBackground ? .running : .succeeded`（AgentRunner.swift:1126-1127）：工具已返回、子 Agent 还在跑，卡片保持运行中（子 Agent 结束时由子 Agent 区定稿）。`toolResultText` 把 `.running` 当成功（§1.1.7）。`refreshTools` 只在 `.succeeded` 时看，后台派发不触发。

#### 1.3.6 `updateEvent` 跨这一轮的几条消息（AgentRunner.swift:1278-1294）

```text
updateEvent(eventID, body):
    guard let id = streamConversationID
    for messageID in turnMessageIDs.reversed() {            // 先试正在写的这条
        found = store.updateMessage(messageID, in: id) { MessageBlock.update(eventID, in: &$0.blocks, body) }
        if found { return }
    }
currentTurnMessages(id) = turnMessageIDs.compactMap { store.message($0, in: id) }   // 子 Agent 区找「并行派发」卡片用
```

#### 1.3.7 等确认时让出本地名额（33aff41 → a3b7284）

审批等待包进 `waitingWithoutSlot(sink, as: .waitingApproval) { await withCheckedContinuation { approvals[event.id] = $0 } }`，
`approved = (try? …) ?? false`（AgentRunner.swift:1083-1086）。主 Agent 的 sink 直接执行 body；SubRun 才让名额、标「在等用户确认」、等完排队拿回（子 Agent 区实现）。拿回名额时被取消 → 视为没确认 → 走「等待确认时被停止」分支。

#### 1.3.8 工具列表

`configure` 与 `refreshTools` 改用 `tools.mainAgentSpecs`（= activeDescriptors 去掉 `report_to_parent`；ToolRegistry.swift:77-79）。

#### 1.3.9 AgentLog 里的「收到的补充」（TurnCheck.swift:80-81）

运行记录里名为 `agent_message`（`AgentRunner.agentMessageTool`）的工具块渲染成 `"【\(call.title ?? "补充")】" + clip(call.arguments, 800)`，放在 `.text` 分支之后、普通 `.tool` 分支之前。（这种块由子 Agent 区在子 Agent 收到消息时写，title 是「用户补充」或「<父名>追加的指示」。）

---

### 1.4 v1.8 层数不限 / 活性 / 用量：主循环侧（95d4df0、a3b7284、e672b61）

#### 1.4.1 系统提示词

`instructions += tools.guidance(team: .main)`（AgentRunner.swift:323；ToolRegistry 新签名 `guidance(team: Role)`，`.main` 带完整派发说明，`.sub` 带「再往下派」的说明——子 Agent 区）。

#### 1.4.2 原地打转（AgentRunner.swift:1043-1053、1148-1150；AgentLiveness.swift:52-58、144-155）

- 常量：`repeatWarn = 3`、`repeatBlock = 5`、`stepNudge = 40`、`pollingTools = ["wait_agents", "check_job", "get_current_time"]`。
- `noteRepeat(call, sink) -> Int`：轮询类工具返回 0（不计）；`signature = name + "\u{1}" + call.prettyArguments`；sink 是 SubRun 用 `run.lastCall`，否则用主 Agent 的 `mainLastCall`；签名与上次相同则 `count + 1`，否则 1；记下并返回。
- `mainLastCall` 在每轮**第一次**流式之前置 nil（AgentRunner.swift:619），中途插入补充、自愈都不清。
- 位置：`consumeStep` → 参数预检 → **这里** → 审批。`repeats >= 5`：
  `problem = "同样的调用已经连续 \(repeats) 次了，结果不会变，这次没有执行。换个方法（换参数、换工具、先读错误信息），或者交付已有结果，把没做完的写进「未完成」。"`；
  追加一张 `failed`、`result = problem`（带 title / symbol）的卡片；诊断日志（tool 类）标题「拦下重复调用 \(name)（连续 \(repeats) 次）」、内容 `event.arguments`；返回 `"调用被拒绝（未执行）：" + problem`。被拦的调用照样计数（第 6、7 次继续拦）。
- 执行完、拼回灌文字时：`repeats == 3` → 追加「\n（同样的调用已经连续 3 次了。结果不会因为重复而变：换个方法，或者交付已有结果；再重复两次会被拦下。）」（数字插值 `\(repeats)`）。

#### 1.4.3 被单独停掉的子 Agent（AgentRunner.swift:1129-1133、998-1004；AgentLiveness.swift:13-40）

- `dispatch` 捕获 `SubAgentEnded` 且 `stoppedBy != nil` → `result = ended.cardText`、`failed`、`stopped = true`；`stoppedBy == nil`（失败）落到普通失败分支，`result = cardText`（「失败了：…」）。
- `SubAgentEnded`（子 Agent 区实现，文字本区要用）：
  - `cardText` = `(stoppedBy.map { "被\($0)停止了，没做完。" } ?? "失败了：\(message)") + (partial.map { "\n\n" + $0 } ?? "")`
  - `advice(forCardText:)`：
    - 以「失败了」开头 →「判断失败原因：能修就换个做法（拆小一点、换个大脑）再派，不行就自己做或告诉用户。」
    - 以「被看门狗」开头 →「它卡住了：上面做到一半的结果可以直接用；剩下的换个做法（拆小一点、换个大脑）再派，或者自己做。」
    - 以「被期限」开头 →「时间到了：用上面做到一半的结果收尾，没做完的在回答里说明。」
    - 其余 →「不要重新派发同样的任务；按已有信息继续（上面做到一半的结果可以直接用），或告诉用户需要什么。」

#### 1.4.4 子 Agent 的活动与步数（AgentRunner.swift:1104-1110、1151-1153）

审批之后、执行之前：sink 是 SubRun 时 `team.setActivity(eventID, .tool(event.title ?? name))`、`team.noteStep(eventID)`；函数退出时（defer）`team.setActivity(eventID, .generating)`。
拼回灌文字时：`sub.steps > 0 && sub.steps % 40 == 0` → 追加「\n（你已经调用了 \(sub.steps) 次工具。检查一下是不是在绕圈子：能交付就先交付，没做完的写进「未完成」。）」（`steps` 是 SubRun 的 consumeStep 计数，含被预检 / 打转拦下的调用）。
回灌文字的拼接顺序：`toolResultText` → 打转提醒 → 步数提醒 → 新工具已载入提示（原有，仅 depth 0 且 succeeded）。

#### 1.4.5 速度与用量（e672b61）

- `AgentRunner.mainLiveSpeed(in: id, now:)`（:1402-1406）：`streamConversationID != id` → 0；否则 `AgentTeamState.windowSpeed(recentChunks, now)`。
- `AgentTeamState.windowSpeed(samples, now)`（AgentTeamControl.swift:115-120）：只留 `now - time <= 3s` 的；不足 2 个 → 0；`span = now - 第一个.time`；`span > 0 ? (除第一个外的 tokens 之和) / span : 0`。
- `AgentRunner.estimateTokens(in: chunk, local:)`（:1408-1412，static，子 Agent 用）：`<think>` / `</think>` → 0；本地 → 1；在线 → `max(1, round(utf8 字节数 / 3.5))`。
- `ToolEvent.usage: AgentUsage?`（ChatModels.swift:75-76、85-106）：由子 Agent 自己写（子 Agent 区），本区提供数据结构与持久化：
  `model: String`、`service: String?`、`thinking: String`（「关闭」「低」「中」「高」或「不支持」）、`temperature: Double`、`generatedTokens = 0`、`promptTokens = 0`、`generateTime: Double = 0`；
  `tokensPerSecond = generateTime > 0 ? generatedTokens / generateTime : 0`；
  `detail = ["深度思考：\(thinking)", "温度：\(temperature 保留 0–2 位小数)", service.map { "服务：\($0)" }].compactMap.joined("\n")`。
- 工具栏的会话合计（`ConversationUsageBadge`、`AgentTree.conversationUsage`）在 Views/AgentTreePanel.swift（界面 / 树区），本区只提供 `mainLiveSpeed`。

#### 1.4.6 其余 v1.8 提交

2084be2（深层卡片收窄）、9096118（Agent 树面板、`team.treeFocus`）是界面 / 树区；本区只有调试钩子 `FRIDAY_TREE_FOCUS`（§4.4）。

---

### 1.5 v1.9 菜单栏两行（5103122）

- `liveAgents: Int = 0`、`liveSpeed: Double = 0`（可观察，AgentRunner.swift:12-15）。
- `startMeter()`（:1378-1391）：`send()` 里设 `generatingConversationID` 之后调用；已在跑就不再起。循环：`guard isGenerating || !team.running.isEmpty else break`；`updateMeter()`；睡 1 s。退出循环后两者归 0。
- `updateMeter(now)`（:1393-1400）：
  `main = generatingConversationID.map { mainLiveSpeed(in: $0, now) } ?? 0`；`subs = Σ team.speed($0, now) over team.running`；
  `agents = (isGenerating ? 1 : 0) + team.running.count`；`speed = round((main + subs) * 10) / 10`；**值变了才赋值**（免得菜单栏每秒重画）。
- 托盘（FridayApp.swift:227-280，界面区）：图标右边两行 `"\(agents) AI"` 与 `speedText(speed)`；菜单里 `liveAgents > 0` 时多一行「\(n) AI 在工作 · \(speedText)」。
- `MenuBarLabel.speedText`（:235-244）：`<= 0` →「0 t/s」；依次 T(1e12) / G(1e9) / M(1e6) / K(1e3)，第一个 `speed >= scale` 的：`v = speed / scale`，`(v >= 100 ? "%.0f" : "%.1f") + " " + 单位 + "t/s"`；都不够 → `speed >= 10 ? "%.0f t/s" : "%.1f t/s"`。
  测试向量：`[0, 8.46, 179.4, 1234, 3_400_000, 1.1e12]` → `["0 t/s", "8.5 t/s", "179 t/s", "1.2 Kt/s", "3.4 Mt/s", "1.1 Tt/s"]`。

---

## 2. Qt 映射

### 2.1 `AgentRunner.h` 新增 / 修改（完整声明，放进现有类）

> **公开部分已由 PORT_V15_V19.md §3.2 取代**，差别：没有 `Q_PROPERTY team` / `teamState()`（C++ 用公有成员 `AgentRunner::team`，QML 用单例 `AgentTeam`）；
> `PendingTurnChanges` 改名 `TurnChanges`；`noteRepeat` 与三个常量、`pollingTools` 归 core（与本节一致）。私有部分仍作建议，`m_recentChunks` 用 `AgentTeamState::Sample`。

```cpp
// ===== Q_PROPERTY（加在现有两个之后）=====
/// == Swift isStopping：点了停止、这一轮还没收尾（停止键旋转、消息底栏「正在停止…」）
Q_PROPERTY(bool isStopping READ isStopping NOTIFY stoppingChanged)
/// 菜单栏 / 托盘：正在工作的 Agent 个数（主 + 运行中的子 Agent）与速度合计（t/s，保留 1 位小数）；生成期间每秒更新，结束归零
Q_PROPERTY(int liveAgents READ liveAgents NOTIFY liveMeterChanged)
Q_PROPERTY(double liveSpeed READ liveSpeed NOTIFY liveMeterChanged)
/// 排队状态每变一次加一（QML 绑定 queuedInputs(id) / isQueuePaused(id) 时带上它，ARCHITECTURE §8.1 的 revision 写法）
Q_PROPERTY(int queueRevision READ queueRevision NOTIFY queueRevisionChanged)
/// 父子 Agent 的往来状态（子 Agent 区的 AgentTeamState；QML 读 running / queues / activity / treeFocus）
Q_PROPERTY(AgentTeamState* team READ teamState CONSTANT)

public:
    // ===== 停止（v1.5）=====
    /// 任意线程（原子量）
    bool isStopping() const { return m_stopRequested.load(std::memory_order_acquire); }
    static QString stoppedNote();          // 「（这一轮被用户中途停止了，上面的回答和操作可能不完整。）」

    // ===== 工作中补充（v1.6）=====
    /// 主线程。== Swift submit（输入框、空状态建议都走它）
    void submit(const QString& text, const QStringList& attachments, const QUuid& conversationID,
                const std::optional<QUuid>& replacing = std::nullopt);
    void removeQueued(const QUuid& itemID, const QUuid& conversationID);
    void resumeQueue(const QUuid& conversationID);
    QList<QueuedInput> queuedInputs(const QUuid& conversationID) const;      // 主线程
    bool isQueuePaused(const QUuid& conversationID) const;                   // 主线程
    int queueRevision() const { return m_queueRevision; }
    /// 回合线程（引擎 interjection 回调 / drainBackgroundAgents）。== Swift takeInterjection
    QList<EngineMessage> takeInterjection(const QUuid& conversationID, bool supportsVision);
    static QString interjectionNote();     // 「（用户在你工作时补充：…有冲突以这条为准。）\n」
    /// "engine:<id>" / "local:<本地模型 id>"（主线程）
    QString engineKey(const QUuid& conversationID) const;
    struct PendingTurnChanges { bool model = false; bool thinking = false; };
    PendingTurnChanges pendingTurnChanges(const QUuid& conversationID) const;   // 主线程

    // ===== 父子协作（v1.7）=====
    AgentTeamState* teamState() const { return team; }
    AgentTeamState* const team;            // 构造时 new AgentTeamState(this)
    /// 这一轮写过的助手消息（任意线程；runOnMainSync 取最新内容）
    QList<ChatMessage> currentTurnMessages(const QUuid& conversationID) const;
    /// 这一轮写过的助手消息 id（任意线程，m_streamMutex）
    QList<QUuid> turnMessageIDs() const;
    /// 改一个工具事件：按这一轮的消息从新到旧找（== Swift updateEvent），返回排到主线程执行的闭包（给 RefreshThrottle 用）；没有在生成返回空
    std::function<void()> eventTask(const QUuid& eventID, std::function<void(ToolEvent&)> body) const;

    // ===== 原地打转（v1.8）=====
    static constexpr int repeatWarn = 3;
    static constexpr int repeatBlock = 5;
    static constexpr int stepNudge = 40;
    /// wait_agents、check_job、get_current_time
    static const QSet<QString>& pollingTools();
    /// 记下这次调用，返回同样的调用连续出现了几次（轮询类返回 0）。sink 是 SubRun 用它的 lastCall，否则用 m_mainLastCall
    int noteRepeat(const ToolCall& call, ToolSink& sink);

    // ===== 速度（v1.8 / v1.9）=====
    /// 主 Agent 最近 3 秒的速度；不是这段在生成返回 0（任意线程，m_liveMutex）
    double mainLiveSpeed(const QUuid& conversationID, const QDateTime& now = QDateTime::currentDateTimeUtc()) const;
    /// 一个 chunk 大约几个 token（子 Agent 用；成员 estimateTokens(chunk) 改成调用它）
    static int estimateTokens(const QString& chunk, bool local);
    int liveAgents() const { return m_liveAgents; }
    double liveSpeed() const { return m_liveSpeed; }

    // ===== QML =====
    Q_INVOKABLE void submit(const QString& text, const QStringList& attachments, const QString& conversationID,
                            const QString& replacing = QString());     // 附件 file:// 转本地路径，同现有 send 重载
    Q_INVOKABLE void removeQueued(const QString& itemID, const QString& conversationID);
    Q_INVOKABLE void resumeQueue(const QString& conversationID);
    Q_INVOKABLE QVariantList queuedInputs(const QString& conversationID) const;   // QueuedInput::toVariant 列表
    Q_INVOKABLE bool isQueuePaused(const QString& conversationID) const;
    Q_INVOKABLE QVariantMap pendingTurnChanges(const QString& conversationID) const;   // {model: bool, thinking: bool}
    Q_INVOKABLE double mainLiveSpeed(const QString& conversationID) const;

signals:
    void stoppingChanged();
    /// 某段对话的排队消息或暂停状态变了（主线程；ChatViewModel 据此刷新）
    void queuedInputsChanged(const QUuid& conversationID);
    void queueRevisionChanged();
    void liveMeterChanged();
    /// 回合中途换了一条新的助手消息（插入补充 / 汇报之后；主线程）。friday-cli 等跟着正在写的消息走的地方用它
    void turnMessageStarted(const QUuid& conversationID, const QUuid& messageID);

private:
    // 回合阶段（★ §2.2）
    enum class TurnPhase : int { Idle, Running, Stopping, Finishing };
    std::atomic<TurnPhase> m_phase { TurnPhase::Idle };
    std::atomic<bool> m_stopRequested { false };

    QSet<QUuid> m_stoppedConversations;                    // 主线程
    QHash<QUuid, QList<QueuedInput>> m_queuedInputs;       // 主线程
    QSet<QUuid> m_pausedQueues;                            // 主线程
    int m_queueRevision = 0;
    QTimer m_queueDelivery;                                // 主线程，单次 500 ms（§2.4.3）
    int m_queueDeliveryTicks = 0;
    std::optional<QUuid> m_queuePreparing;
    std::optional<QString> m_turnEngineKey;                // 主线程
    std::optional<ThinkingLevel> m_turnThinking;           // 主线程
    std::optional<QString> m_turnEngineName;               // send() 在起回合线程前写，回合线程只读
    bool m_turnExpectsThinking = false;                    // 同上
    QList<QUuid> m_turnMessageIDs;                         // m_streamMutex
    std::optional<std::pair<QString, int>> m_mainLastCall; // 回合线程
    mutable QMutex m_liveMutex;                            // 保护 m_recentChunks（主线程的 meter 也读）
    QTimer m_meterTimer;                                   // 主线程，1000 ms
    int m_liveAgents = 0;
    double m_liveSpeed = 0;

    void send(const QList<QueuedInput>& inputs, const QUuid& conversationID, const std::optional<QUuid>& replacing);
    QList<MediaAttachment> importAttachments(const QStringList& paths, const QUuid& conversationID);   // 任意线程
    /// 主线程：++m_queueRevision，发 queueRevisionChanged 与 queuedInputsChanged(id)
    void notifyQueue(const QUuid& conversationID);
    void scheduleQueueDelivery();
    void deliverQueueTick();
    void cancelTeam();                                     // 主线程
    void drainBackgroundAgents(ChatEngine& engine, const QUuid& conversationID);   // 回合线程
    void startMeter();
    void updateMeter();
    /// 回合线程：finish() 一开始调用。阶段切到 Finishing，返回这一轮是不是被停止的
    bool beginFinishing();
    // §2.11：替代 markRefold / refoldLocalSession / m_refoldPending
    enum class ResyncReason { Fold, Stopped };
    QHash<QUuid, ResyncReason> m_resyncPending;            // m_sessionsMutex
    bool markResync(const QUuid& conversationID, ResyncReason reason);
    void resyncLocalSession(const QUuid& conversationID, const QList<ChatMessage>& history);
```

修改的现有签名：
- `runTurn(const QUuid&, const QList<ChatMessage>& history, const QList<ChatMessage>& userMessages, const std::optional<QString>& foldNotice)`（单条 → 多条）。
- `describeAttachments(const ChatMessage&, bool supportsVision, const QUuid&)`（`ChatEngine&` → `bool`）。
- `expectsThinking(...)`、`engineNameForStats(...)` 删除，改读 `m_turnExpectsThinking` / `m_turnEngineName`（在**创建**闭包时捕获值）。
- `updateEvent` 改为 `if (auto task = eventTask(id, body)) runOnMain(this, task);`。

### 2.2 回合状态机与主线程闭包顺序（★ 必须照做）

```text
Idle --send()--> Running --stop()--> Stopping
Running|Stopping --finish() 开头 beginFinishing()--> Finishing --收尾闭包--> Idle
```
- `send()`（主线程）：`m_stopRequested = false; m_phase = Running;` 在 `setGenerating(id)` **之前**。
- `stop()`（主线程）：
  ```cpp
  if (!isGenerating()) return;
  TurnPhase expected = TurnPhase::Running;
  if (!m_phase.compare_exchange_strong(expected, TurnPhase::Stopping)) return;   // 已在停止 / 已在收尾：只认第一次
  m_stopRequested = true; emit stoppingChanged();
  updateMessage([](ChatMessage& m) { m.stoppedByUser = true; });
  #ifndef NDEBUG  FRIDAY_SLOW_STOP：QTimer::singleShot(秒*1000, this, [this, token = m_currentTask] { cancelTeam(); if (token) token->cancel(); });
                  立刻 resolve(false) 全部审批；return  #endif
  cancelTeam();
  if (m_currentTask) m_currentTask->cancel();
  resolve(false) 全部审批（现有代码）
  ```
- `beginFinishing()`（回合线程，`finish()` 第一句）：`return m_phase.exchange(TurnPhase::Finishing) == TurnPhase::Stopping;`。之后 `stop()` 的 CAS 必然失败（== Swift 里 stop 不可能插进 finish 中间）。
- 收尾闭包（主线程，`runOnMain`）里的顺序——**`setGenerating(std::nullopt)` 之前**把这一轮的状态全部收好，因为它同步发 `generatingChanged`，直连槽可能立刻重入 `send()`：
  1. `if (stopped) m_stoppedConversations.insert(id)`；`store->touch(id)`；有临时标题就 `generateTitle(id)`
  2. `team->reset(id)`
  3. `if (stopped && !m_queuedInputs.value(id).isEmpty()) { m_pausedQueues.insert(id); notifyQueue(id); }`
  4. `m_turnEngineKey.reset(); m_turnThinking.reset();`
  5. `const bool wasStopping = m_stopRequested.exchange(false); m_phase = Idle; if (wasStopping) emit stoppingChanged();`
  6. `setGenerating(std::nullopt)` ← 重入点
  7. `if (auto selected = store->selectedID()) prepareModel(*selected);`（有 `!isGenerating` 守卫，重入开始了新一轮就不动）
  8. `if (m_currentTask == turnToken) m_currentTask.reset();`（现有）
  9. `store->saveNow()`
  10. `if (!m_queuedInputs.isEmpty()) scheduleQueueDelivery();`（它第一次检查是异步的，§2.4.3）
- `send()` 里同理：`m_turnEngineKey / m_turnThinking / m_turnEngineName / m_turnExpectsThinking / m_streamEngineIsLocal` 在 `setGenerating(id)` **之前**定下，否则 `generatingChanged` 的槽（ChatViewModel）会在一瞬间算出「下一轮生效」。

### 2.3 `send` / `runTurn` / `finish` 改写要点

**`send(inputs)`（AgentRunner.cpp:823-905 改写）**——逐条对应 §1.2.4、§1.1.5：
```cpp
// trim、过滤空的；guard !inputs.isEmpty() && (!replacing || inputs.size() == 1) && !isGenerating() && canSend(id)
QList<ChatMessage> userMessages;   // 每条 ChatMessage(User, text, importAttachments(paths, id))
… history（replacing 时截到被替换的消息之前）、foldedTurns、userMessages.last().taskNote = TaskState::promptNote(history)
store->update(id, …)：conversation.isEmpty() 时临时标题取 userMessages.first()（plainText 空 → 第一个附件 fileName → 「附件」，prefixCharacters 24）；
                      replacing ? fork(*replacing, userMessages.first()) : 逐条 append；再 append(assistant)
const bool resync = foldedTurns > 0 || m_stoppedConversations.remove(id);       // C++ 的 || 同样短路
if (replacing || (resync && !markResync(id, foldedTurns > 0 ? ResyncReason::Fold : ResyncReason::Stopped))) 丢会话（加锁）
store->touch(id)
快照：m_streamEngineIsLocal、m_turnEngineKey、m_turnThinking、m_turnEngineName、m_turnExpectsThinking
m_stopRequested = false; m_phase = Running;
setGenerating(id); startMeter();
{ lock m_streamMutex: m_streamConversationID = id; m_streamMessageID = assistant.id; m_turnMessageIDs = { assistant.id }; }
foldNotice …（现有）; m_pass = new; resetStats(); beginPass();
m_currentTask = Worker::newToken(); Worker::run(m_currentTask, [=] { runTurn(id, history, userMessages, foldNotice); });
```
公开的 `send(text, attachments, id, replacing)` 只做 `send(QList<QueuedInput>{ QueuedInput{text, attachments} }, id, replacing)`。

**`runTurn`（AgentRunner.cpp:912-962 改写）**：
```cpp
try { MCP waitUntilSettled（现有） } catch (...) {}
try {
    ensureLocalModelLoaded(id);
    auto engine = this->engine(id, history);          // 内含 refoldLocalSession → 现在叫 resyncLocalSession（§2.11）
    QList<EngineMessage> outgoing;
    for (const ChatMessage& m : userMessages) { const ChatMessage d = describeAttachments(m, engine->supportsVision(), id);
        outgoing << EngineMessage::user(promptText(d), vision ? images(d) : {}, vision ? videos(d) : {}); }
    const EngineMessage& last = outgoing.last();     // 自愈按最后一条重发
    std::optional<QString> failure; std::optional<ContextOverflowError> overflow;
    try {
        m_mainLastCall.reset();
        if (outgoing.size() == 1) consume([&](auto& on) { engine->stream(last.content, last.images, last.videos, on); });
        else                      consume([&](auto& on) { engine->stream(outgoing, on); });
        verifyIfNeeded(*engine, id);
        drainBackgroundAgents(*engine, id);
    } catch (...) {
        // ★ 先判停止：停止导致的任何异常（网络中断、进程内运行时返回码、上下文超长…）都不 recover
        if (isStopping() || CancelToken::currentIsCancelled() || currentExceptionIsCancellation()) { /* 什么也不做 */ }
        else if (auto* e = 当前异常是 ContextOverflowError) { failure = e->message(); overflow = *e; }
        else failure = describeCurrentException();
    }
    if (failure) recover(*failure, id, last.content, last.images, last.videos, overflow, foldNotice);
} catch (...) {
    if (!(isStopping() || CancelToken::currentIsCancelled() || currentExceptionIsCancellation())) { 记「无法创建对话引擎」、写 error（现有） }
}
finish();
```
（现有代码用 `catch (const ContextOverflowError&)` 在前；改成「先判停止」时用一个 `catch (...)` + `std::rethrow_exception` 分类，或者在 `catch (const ContextOverflowError&)` 里也先判停止。）

**`verifyIfNeeded`（AgentRunner.cpp:964-1024）**：开头 guard 加 `isStopping()`；`runOnMainSync` 里 `blocks` 改为 `for (id : turnMessageIDs()) blocks += conversation->message(id)->blocks`（按顺序，找不到的跳过），其余不变。

**`finish()`（AgentRunner.cpp:1929-1974 改写）**：
```cpp
const bool stopped = beginFinishing();
const auto target = streamTarget();
const QList<QUuid> turnMessages = turnMessageIDs();
try {
    renderPass(true);
    if (stopped) {
        updateMessage([](ChatMessage& m) { m.stoppedByUser = true; MessageBlock::markStopped(m.blocks); });
        if (target) for (qsizetype i = 0; i + 1 < turnMessages.size(); ++i)
            runOnMain(this, [this, conv = target->first, mid = turnMessages[i]] {
                store->updateMessage(mid, conv, [](ChatMessage& m) { MessageBlock::markStopped(m.blocks); }); });
    }
    统计 / persistCache（现有）
    updateMessage([](ChatMessage& m) {
        if (m.blocks.isEmpty() && !m.error && m.stoppedByUser != true) m.blocks = { MessageBlock::makeText(QStringLiteral("（没有输出）")) };
    });
} catch (...) {}
{ lock m_streamMutex: m_streamConversationID.reset(); m_streamMessageID.reset(); m_turnMessageIDs.clear(); }
runOnMain(this, [… stopped …] { §2.2 的 1–10 });
```

**`stop()`、`cancelTeam()`**：见 §2.2；`cancelTeam()`：`if (!m_generatingConversationID) return; team->cancelRuns(*m_generatingConversationID); team->wake(AgentTeamState::mainKey());`。

**`replaceKeepingTranscript`（AgentRunner.cpp:1999-2007）**：再加 `stored.stopped = event.stopped;`。

**`historyMessages` / `toolResultText` / `promptText`**：照 §1.1.7、§1.2.7 改；三者都是纯函数（`toolResultText` 依赖 `SubAgentEnded::advice`，见 §3.2）。

### 2.4 `takeInterjection` / `drainBackgroundAgents` / 排队投递

#### 2.4.1 `takeInterjection`（回合线程）

```cpp
QList<EngineMessage> AgentRunner::takeInterjection(const QUuid& conversationID, bool supportsVision)
{
    const auto target = streamTarget();
    if (!target || target->first != conversationID || isStopping())   // isGenerating 隐含在 target 里
        return {};
    const QList<QueuedInput> items = runOnMainSync([&] {
        if (m_pausedQueues.contains(conversationID)) return QList<QueuedInput>();
        QList<QueuedInput> taken = m_queuedInputs.take(conversationID);
        if (!taken.isEmpty()) notifyQueue(conversationID);           // queueRevision++、两个信号
        return taken;
    });
    const QList<AgentTeamState::Notice> notices = team->takeNotices(AgentTeamState::mainKey());
    if (items.isEmpty() && notices.isEmpty()) return {};

    QList<ChatMessage> users;
    for (const auto& n : notices) { ChatMessage m(ChatRole::User, noticeText(n)); m.fromAgent = n.agentName; users << m; }
    for (const auto& item : items) { ChatMessage m(ChatRole::User, item.text, importAttachments(item.attachments, conversationID));
                                     m.interjected = true; users << m; }

    // 当前这条收尾（统计定稿）——这几步排到主线程时目标还是旧消息
    renderPass(true);
    if (m_turnGeneratedTokens > 0 || m_turnPromptTokens > 0) publishFinalStats(false);
    else updateMessage([](ChatMessage& m) { m.stats.reset(); });
    if (contextAssistEnabled()) users.last().taskNote = TaskState::promptNote(currentPath(conversationID));   // 读到的是收尾后的路径
    const ChatMessage assistant(ChatRole::Assistant, QString());
    runOnMain(this, [this, conversationID, users, assistant] {
        store->update(conversationID, [&](Conversation& c) { for (const auto& u : users) c.append(u); c.append(assistant); });
        emit turnMessageStarted(conversationID, assistant.id);
    });
    { QMutexLocker lock(&m_streamMutex); m_streamMessageID = assistant.id; m_turnMessageIDs.append(assistant.id); }
    startNewPass(0);          // 新消息从第 0 块写起；旧消息排着的流式刷新因段号变了不再生效
    resetStats();
    beginPass();              // 新消息进入「处理提示词」

    QList<EngineMessage> outgoing;
    for (const ChatMessage& user : users) {
        const ChatMessage d = describeAttachments(user, supportsVision, conversationID);
        outgoing << EngineMessage::user(promptText(d), supportsVision ? images(d) : QStringList(), supportsVision ? videos(d) : QStringList());
    }
    return outgoing;
}
```
注意：`MediaStore::importFile` 可以在工作线程调用（ARCHITECTURE §3.5 表：MediaStore「全部」）。
`describeAttachments` 期间提示「当前大脑不能直接看图，正在用看图模型描述附件…」落在**新**消息上（与 Swift 相同）。

#### 2.4.2 `drainBackgroundAgents`（回合线程）

```cpp
void AgentRunner::drainBackgroundAgents(ChatEngine& engine, const QUuid& conversationID)
{
    const QUuid key = AgentTeamState::mainKey();
    while (!CancelToken::currentIsCancelled() && !isStopping()) {
        const bool hasNotices = team->hasNotices(key);
        const bool hasUserInput = runOnMainSync([&] {
            return !m_queuedInputs.value(conversationID).isEmpty() && !m_pausedQueues.contains(conversationID); });
        if (!hasNotices) {
            if (team->backgroundChildren(key, conversationID).isEmpty()) return;
            if (!hasUserInput) { team->waitForNotice(key); continue; }   // 当前令牌取消、wake、post 都会返回；不抛异常
        }
        const QList<EngineMessage> messages = takeInterjection(conversationID, engine.supportsVision());
        if (messages.isEmpty()) continue;
        configure(engine, conversationID);
        renderPass(true);
        startNewPass(currentBlockCount());
        consume([&](const EngineEventHandler& onEvent) { engine.stream(messages, onEvent); });
    }
}
```

#### 2.4.3 排队投递（主线程）

`scheduleQueueDelivery()`：`m_queueDelivery.stop(); m_queueDeliveryTicks = 0; m_queuePreparing.reset(); QTimer::singleShot(0)` 风格地**异步**执行第一次 `deliverQueueTick()`（用 `m_queueDelivery`：单次触发、首次间隔 0、之后 500 ms）。
`deliverQueueTick()` 照 §1.2.8：`isGenerating()` → 停；选目标（`store->selectedID()` 优先，否则 `createdAt` 最早）；会话不存在 → 删键、**立即**再选（同一 tick 内循环）；`canSend` → `send(items, target, nullopt)`（items 从 `m_queuedInputs.take` 拿，`notifyQueue`）；否则 `prepareModel(target)`（同一目标只一次）、`++ticks`，`ticks < 2400` 时 500 ms 后再来。
`submit` / `removeQueued` / `resumeQueue` 照 §1.2.3；每次改了队列或暂停集合都 `notifyQueue(id)`（`++m_queueRevision; emit queueRevisionChanged(); emit queuedInputsChanged(id);`）。
`submit` 里叫醒主 Agent：`if (m_generatingConversationID == conversationID) team->wake(AgentTeamState::mainKey());`。

### 2.5 `dispatch` 改写（AgentRunner.cpp:1475-1611）

在现有骨架上按 Swift 顺序插入（伪代码，`sub = dynamic_cast<SubRun*>(&sink)`）：
```cpp
consumeStep / preflight（不变）
const int repeats = noteRepeat(call, sink);
if (repeats >= repeatBlock) { 追加 failed 卡片（result = problem，title/symbol）；journal Tool「拦下重复调用 %1（连续 %2 次）」, event.arguments；
                              return "调用被拒绝（未执行）：" + problem; }
needsApproval / event / 登记审批 / sink.append（不变）
if (needsApproval) {
    notePendingApproval(…, true);
    bool approved = false;
    try { waitingWithoutSlot(sink, AgentTeamState::Activity::waitingApproval(), [&] { approved = approval->wait(); }); }
    catch (const CancellationError&) { approved = false; }
    notePendingApproval(…, false); 撤登记（不变）
    if (!approved || CancelToken::currentIsCancelled() || isStopping()) {
        if (isStopping() || CancelToken::currentIsCancelled()) { event.status = Failed; event.stopped = true; event.result = "等待确认时被停止，没有执行"; }
        else event.status = Denied;
        sink.replace(event); return toolResultText(event);
    }
    event.status = Running; sink.replace(event);
}
if (sub) { team->setActivity(sub->eventID, Activity::tool(event.title.value_or(call.name))); team->noteStep(sub->eventID); }
const auto activityGuard = qScopeGuard([&] { if (sub) team->setActivity(sub->eventID, Activity::generating()); });   // 函数退出时
try {
    … 执行（不变）
    event.status = output.runsInBackground ? ToolStatus::Running : ToolStatus::Succeeded;
} catch (const SubAgentEnded& ended) {
    if (ended.stoppedBy()) { event.result = ended.cardText(); event.status = Failed; event.stopped = true; }
    else stoppedOrFailed(ended.message());        // 见下
} catch (...) {
    stoppedOrFailed(describeCurrentException());
}
// stoppedOrFailed(msg)：isStopping() || CancelToken::currentIsCancelled() || currentExceptionIsCancellation()
//   → result「已停止」、Failed、stopped = true（不记日志）；否则 result = msg、Failed、journal「工具 %1 失败」（现有文案）
sink.replaceKeepingTranscript(event);
QString text = toolResultText(event);
if (repeats == repeatWarn) text += QStringLiteral("\n（同样的调用已经连续 %1 次了。结果不会因为重复而变：换个方法，或者交付已有结果；再重复两次会被拦下。）").arg(repeats);
if (sub && sub->steps() > 0 && sub->steps() % stepNudge == 0)
    text += QStringLiteral("\n（你已经调用了 %1 次工具。检查一下是不是在绕圈子：能交付就先交付，没做完的写进「未完成」。）").arg(sub->steps());
新工具已载入（不变，仅 depth 0 且 Succeeded）
return text;
```
`noteRepeat`：`if (pollingTools().contains(call.name)) return 0; const QString sig = call.name + QChar(0x01) + call.prettyArguments();`，SubRun 用 `sub->lastCall`，主 Agent 用 `m_mainLastCall`（回合线程独占，不加锁）。

### 2.6 引擎层

#### 2.6.1 `ChatEngine.h`
```cpp
/// 一段生成的工具调用全部执行完、发起下一段请求之前调用一次（在引擎所在的工作线程）：返回要接在工具结果后面的消息
/// （主 Agent：用户在工作中补充的话、后台子 Agent 的汇报；子 Agent：交给它的指示和它下级的汇报）。空 = 不调
std::function<QList<EngineMessage>()> interjection;
```

#### 2.6.2 `OpenAIChatEngine`（两种模式的本地引擎都继承它，钩子只写一处）
- 新增 `protected: QJsonObject userObject(const EngineMessage& message) const;` = `userMessage(message.content, message.images, message.videos, supportsVision(), /*lenient*/ false)`（== Swift userObject；用虚函数 `supportsVision()`——`LocalLlamaEngine` 的视觉能力在它自己的 `m_supportsVision` 里，不一定在 `m_config`）。
  （**已由 PORT_V15_V19.md §3.7 / §6.B 定为骨架实现**：`userObject` 与下面 `run()` 里的 interjection 调用点由骨架写好，core 只改两个 `stream` 并测试。）
- `stream(const QList<EngineMessage>&, …)`（OpenAIChatEngine.cpp:306-311）：用户消息改走 `userObject`，其余 `convert`（== 1b760a4）。`LocalLlamaEngine::stream(messages)`（LocalLlamaEngine.cpp:114-123）的覆盖随之可以删掉（保留也行，行为相同）。
- `run()`（OpenAIChatEngine.cpp:320-413）：工具结果 `for` 循环（:396-411）**之后**、`while` 下一次迭代之前：
  ```cpp
  if (interjection) {
      CancelToken::checkCurrent();                     // == try Task.checkCancellation()
      for (const EngineMessage& message : interjection())
          m_history.append(message.role == EngineMessage::Role::User ? userObject(message) : convert(message, m_config.supportsVision));
  }
  ```
  收尾请求（wrapUp）、截断续写、`!keepGoing` 都不经过这里（与 Swift 一致：只有执行过工具才调）。
- 不变量：钩子**只在两次请求之间**调用，绝不在 `transport()` 的 chunk 回调里——所以 `takeInterjection` 里再发看图请求（本地 VLM 也走 `LlamaRuntime::chat`）不会重入运行时。

#### 2.6.3 `LocalLlamaEngine`
不用单独接钩子（继承 run）。interjection 消息的图片 / 视频：视觉模型按 `userMessage` 处理（图片 data URL、视频抽帧），纯文本模型只留文字（路径已经在 `promptText` 里）——与 Swift 本地会话把 `.user(text, images, videos)` 交给 VLM 的效果对应。

#### 2.6.4 与 v1.7 RequestGate（在线限流区）的交错
- Swift 里 v1.6 的改动在 `stream(prompt:)`、`stream(messages:)`、`userObject`、`run()` 末尾；33aff41 的改动在属性 `requestPriority` / `onQueued` 和 `requestPass` → `streamPass`（闸门 acquire / release、429 退避），**代码区域不重叠**。
- Qt 里限流区改 `OpenAIChatEngine::transport()`（在线 HTTP）。必须注意：**子进程模式的本地引擎也走基类 `transport()`**（`LocalLlamaEngine::transport` 只在进程内时改走运行时）——闸门要跳过本地引擎，Swift 的本地 MLX 不经过闸门。
  （跳过的写法**已由 PORT_V15_V19.md §3.7 取代**：虚函数 `requestGate()`，`LocalLlamaEngine` 覆盖返回 nullptr，骨架写好。）
- 主 Agent 用默认优先级（`RequestGate.mainPriority`），不设 `onQueued`；interjection 发生在两次请求之间，不持有闸门名额。

### 2.7 两种推理模式（`FRIDAY_INFERENCE=process` llama-server / `inproc` LlamaRuntime）的差异

| 项 | 子进程（llama-server） | 进程内（LlamaRuntime） | 应用层要做的 |
|---|---|---|---|
| 停止的响应时间 | 断开 HTTP：生成中 5.7–8.7 ms，预填中 49–132 ms | `should_stop` ≤ 50 ms 一查：生成中 ~3.6 ms，预填中 ~79 ms（≤ 1 个 ubatch） | 无；都满足「一般 0.1 秒内停下（本地模型预填时最多等完当前这一块）」 |
| 停止后的 KV | 槽位保留到停下的位置 | 已算的 token 留在序列里 | 都靠 §1.1.5 的重新同步：`replaceHistory`，下一次请求从分歧处接着算 |
| 停止后重新同步的代价 | 普通注意力从分歧处截断；SWA / 混合模型只有 llama-server 自己的检查点，可能整段重算 | `SequenceCache` 退到最近的 B / E 检查点（被停那一轮的 B 一定在） | §2.11：停止引起的同步**不要**像折叠那样先回到前缀 |
| 插入补充 / 汇报 | 下一次请求带完整历史，按公共前缀复用，只预填「工具结果 + 补充」 | 同；最后一条是 user 时设 B 检查点 | 无 |
| 后台本地子 Agent 与主 Agent 同时跑时 KV 池满 | **所有**在处理的槽位一起失败（包括主 Agent）→ 主 Agent 收到 `KVCacheFullError` | 只牺牲一个（非主序列里 token 最多的） | **已由 PORT_V15_V19.md §3.6.5（Q2）取代**：等一个本地子 Agent 结束（最多 2 秒）后原样重发这一段，每次 consume 最多 2 次，再不行才 recover；子 Agent 不落主序列见 §3.6.3（Q3） |
| 看图描述（大脑不能看图时） | LocalVLMBackend 借非主槽位 | 同，`LlamaRuntime::chat` 另一个 seq | 钩子在两次请求之间调用，不重入（§2.6.2） |
| meter 的 token 估算 | 每个 SSE chunk 记 1 | 每个 chunk 记 1（运行时偶尔把几个 token 合成一个 chunk，速度会偏低一点） | 照 Swift（本地 1 / chunk） |

### 2.8 数据模型

#### 2.8.1 `ChatModels.h / .cpp`

> **已由 PORT_V15_V19.md §3.1 取代**的几处：`AgentUsage::temperature` 缺省 0（不是 0.6）；`detail(const QLocale& locale = QLocale())` 带区域参数（测试传 `QLocale::c()`）；
> 新增 `ToolEvent::isFinished()`；`agentNoticeTitle / agentNoticeBody` **不进** `ChatMessage::toVariant`，改由 MessageListModel 的角色算（PORT §3.11.3）。
> 这些字段、JSON 与 toVariant 由骨架一次写好（PORT §6.B），core 只实现 `markStopped`。

```cpp
struct AgentUsage {                      // == Swift AgentUsage（ChatModels.swift:85-106）
    QString model;
    std::optional<QString> service;
    QString thinking;
    double temperature = 0.6;
    int generatedTokens = 0;
    int promptTokens = 0;
    double generateTime = 0;
    double tokensPerSecond() const { return generateTime > 0 ? generatedTokens / generateTime : 0; }
    /// 「深度思考：中\n温度：0.6\n服务：智谱 GLM」（温度保留 0–2 位小数、去掉末尾的 0；没有服务就两行）
    QString detail() const;
    QJsonObject toJson() const;          // 7 个键：model、service（nil 不写）、thinking、temperature、generatedTokens、promptTokens、generateTime——非可选的一定写（Swift 读时必填）
    static AgentUsage fromJson(const QJsonObject&);   // 读时宽松，缺的用默认值
    QVariantMap toVariant() const;       // JSON 的键 + tokensPerSecond、detail；service 缺省空字符串
    bool operator==(const AgentUsage&) const = default;
};
// ToolEvent 追加：
    /// 用户点了停止 / 被单独停掉，没跑完（状态记 Failed，界面灰色「已停止」）
    std::optional<bool> stopped;
    /// 子 Agent 用的大脑和用量（子 Agent 自己写）
    std::optional<AgentUsage> usage;
// ChatMessage 追加：
    std::optional<bool> stoppedByUser;
    std::optional<bool> interjected;
    /// 后台子 Agent 送给主 Agent 的汇报（值是子 Agent 名字）；以 user 身份交给模型，界面画成汇报卡片
    std::optional<QString> fromAgent;
// MessageBlock 追加：
    /// 用户点了停止：running / awaitingApproval 的调用（递归进运行记录）标成已停止（== Swift markStopped）
    static void markStopped(QList<MessageBlock>& blocks);
```
- JSON：`ToolEvent::toJson` 加 `Json::setIfPresent(object, "stopped", stopped)`、`usage` 有值时写对象；`fromJson` 对应读。`ChatMessage` 加 `stoppedByUser`、`interjected`、`fromAgent`（`setIfPresent` / `optBool` / `optString`）。
- `toVariant`：`ToolEvent` 加 `stopped`（bool，缺省 false）、`usage`（有值是 map，否则空 map）、`hasUsage`；`ChatMessage` 加 `stoppedByUser`、`interjected`（bool）、`fromAgent`（字符串，空 = nil）、`agentNoticeTitle`（`fromAgent` 有值时 = `AgentRunner::noticeTitle(第一行)`，否则空）、`agentNoticeBody`（第一个 `\n` 之后，否则空）。
  （`noticeTitle` 是纯函数，放在 core 里谁都能调；ChatModels 不依赖 AgentRunner 的话把它放进 `ChatMessage` 的一个静态辅助里，由子 Agent 区的 `noticeTitle` 转调同一个实现。）
- `detail()` 的温度格式：Swift 用当前区域的 `.formatted`；Qt 用 `QLocale()` 的 `toString(v, 'f', 2)` 去掉末尾的 0 和小数点。测试里固定 C / zh 区域（风险 §5-9）。
- 存档样例（给 `tests/fixtures` 用，Swift 格式）：
```json
{"id":"…","role":"assistant","createdAt":"2026-10-02T08:00:00Z","stoppedByUser":true,
 "blocks":[{"id":"…","kind":{"tool":{"_0":{"id":"…","name":"delegate_task","arguments":"{}","status":"failed","result":"已停止",
   "stopped":true,"agentName":"研究员","transcript":[],
   "usage":{"model":"glm-5.3-flash","service":"智谱 GLM","thinking":"中","temperature":0.6,"generatedTokens":300,"promptTokens":9000,"generateTime":6}}}}}]},
{"id":"…","role":"user","createdAt":"…","interjected":true,"blocks":[{"id":"…","kind":{"text":{"_0":"顺便看看评价"}}}]},
{"id":"…","role":"user","createdAt":"…","fromAgent":"研究员","blocks":[{"id":"…","kind":{"text":{"_0":"[子 Agent「研究员」（id：2f1e7c1a）完成了]\n结论：…"}}}]}
```

#### 2.8.2 `ToolTypes.h`
`ToolOutput` 加 `bool runsInBackground = false;`，构造函数加第三个参数 `bool runsInBackground = false`（`explicit ToolOutput(QString, QList<MediaAttachment> = {}, bool runsInBackground = false)`）。

#### 2.8.3 新文件 `core/agent/QueuedInput.h`（对应 Agent/QueuedInput.swift）
```cpp
struct QueuedInput {
    QUuid id = QUuid::createUuid();
    QString text;                          // 已 trim
    QStringList attachments;               // 本地文件路径（发出 / 交给 Agent 时才导入媒体目录）
    QDateTime createdAt = QDateTime::currentDateTimeUtc();
    std::optional<QString> from;           // nullopt = 用户；否则父 Agent 名字（子 Agent 的队列用）
    /// QML：{id, text, attachments: [{path, fileName, url}], fileNames（「、」连接）, createdAt, from（空 = 用户）}
    QVariantMap toVariant() const;
    bool operator==(const QueuedInput&) const = default;
};
```

### 2.9 `ProcessRunner` / `AgentTools::runShell`
- `ProcessRunner::run`（ProcessRunner.cpp:316-330）：Unix 上 `options.newSession` 时子进程是新会话 / 进程组的组长——取消或超时时改为 `::kill(-pid, SIGTERM)`（整组：shell 和它起的子孙），2 秒后还在 `::kill(-pid, SIGKILL)`；没设 newSession 时保持现状。Windows 作业对象不变。（== Swift `pkill -P` + terminate，2 秒后 KILL；整组覆盖得更全。）
- `AgentTools::runShell`（AgentTools.cpp:422-444）：`ProcessRunner::run` 返回后加 `CancelToken::checkCurrent();`（== Swift `if Task.isCancelled { throw CancellationError() }`：命令恰好自己结束也按「已停止」）。
- 脚本工具（ScriptRunner）同样走 ProcessRunner，自动受益。

### 2.10 `TurnCheck.cpp` 的 AgentLog
`AgentLog::render`（TurnCheck.cpp:286-312）的 `Kind::Tool` 分支开头：
```cpp
if (call.name == QLatin1String("agent_message")) {      // AgentRunner 的 agentMessageTool（子 Agent 区定义常量）
    lines << QStringLiteral("【") + call.title.value_or(QStringLiteral("补充")) + QStringLiteral("】") + clip(call.arguments, 800);
    break;
}
```

### 2.11 `LocalKVSession.cpp`：停止后的重新同步
现有 `markRefold` / `refoldLocalSession`（:218-253）只服务折叠。改成带原因：
```cpp
enum class ResyncReason { Fold, Stopped };
bool markResync(const QUuid& id, ResyncReason reason);   // 本地会话留着、记下原因（Fold 优先：两个都有时按 Fold）；不是本地会话返回 false
void resyncLocalSession(const QUuid& id, const QList<ChatMessage>& history);   // engine() 里在 reclaimMainSlot 之前调用（现 refoldLocalSession 的位置）
```
- 两种原因都 `local->replaceHistory(historyMessages(history))`。
- 只有 **Fold** 才走现有的「主槽位 KV 来自磁盘且不能从中间截断 → `restorePrefix` 回到前缀」：停止引起的分歧在最后一轮里，被停那一轮预填时运行时已经建了 B（用户消息之后），退到 B 就够；回到前缀会白白重算整段历史。
- 子进程模式下两种原因都只是换历史（llama-server 按公共前缀复用）。

### 2.12 friday-cli
`CliSession`（CliSession.cpp:431-438）把 `m_messageID` 定成发送时路径上最后一条；v1.7 起后台子 Agent 的汇报会让回合中途换到新的助手消息，CLI 要连 `AgentRunner::turnMessageStarted(conv, msg)` 改跟新消息（并把汇报卡片 / 补充以一行灰字打印）。CLI 不排队（仍用 `send`）。Ctrl+C 调 `stop()` 的行为不变（`stop()` 只认第一次）。

### 2.13 app 层：给界面区的数据契约（`ChatViewModel` / `MessageListModel`）

> **已由 PORT_V15_V19.md §3.11.2–§3.11.3 取代**（归 chat-ui 包）：属性名用 `queuedInputs / queuePaused / queueStatus`（不是 `queuedItems / queueStatusText`），
> 不要 `queueButtonHelp / stopButtonHelp`（提示在 QML 拼），`send` 改名 `submit`。下面的语义说明仍然有效。

`ChatViewModel`（ChatViewModel.h/.cpp）：
- `canSend` 语义改为 `runner->isGenerating() || runner->canSend(id)`；QML 侧再加 `hasContent && (!vm.isGenerating || editing === "")`（== Swift canSend）。
- `suggestionsEnabled` = `runner->canSend(id) || runner->isGenerating()`。
- `send(text, attachments, replacing)` 改调 `runner->submit(...)`。
- `placeholderText` / `placeholder(editing)` 按 §1.2.10 的顺序。
- 新属性（NOTIFY `changed`，排队相关的用新的 `queueChanged`）：
  ```cpp
  Q_PROPERTY(bool isStopping READ isStopping NOTIFY changed)                 // runner->isStopping()（全局：任何对话的停止键都能停正在跑的那段）
  Q_PROPERTY(bool pendingModelChange READ pendingModelChange NOTIFY changed)
  Q_PROPERTY(bool pendingThinkingChange READ pendingThinkingChange NOTIFY changed)
  Q_PROPERTY(QString pendingChangeHelp READ pendingChangeHelp NOTIFY changed)   // §1.2.10 的 help 全文
  Q_PROPERTY(QString queueButtonHelp READ queueButtonHelp NOTIFY changed)       // 「补充给 Agent：…」/「排队：…」
  Q_PROPERTY(QString stopButtonHelp READ stopButtonHelp NOTIFY changed)         // 「正在停止…」/「停止生成 (<修饰键>.)」（修饰键同现有 QML 的 App.shortcutModifier；也可以留在 QML 拼）
  Q_PROPERTY(QVariantList queuedItems READ queuedItems NOTIFY queueChanged)     // QueuedInput::toVariant 列表
  Q_PROPERTY(bool queuePaused READ queuePaused NOTIFY queueChanged)
  Q_PROPERTY(QString queueStatusText READ queueStatusText NOTIFY queueChanged)  // §1.2.10 的五种状态文字
  Q_INVOKABLE void removeQueued(const QString& itemID);
  Q_INVOKABLE void resumeQueue();
  ```
  连接：`AgentRunner::stoppingChanged` → refresh；`queuedInputsChanged(id)` 是这段 → 发 `queueChanged`；`queueStatusText` 还依赖 `generatingChanged`、模型状态（`canSend`），这些信号到来时也发 `queueChanged`。
- `MessageListModel`：新角色 `StoppedByUserRole`（"stoppedByUser"）、`InterjectedRole`（"interjected"）、`FromAgentRole`（"fromAgent"）、`AgentNoticeTitleRole`（"agentNoticeTitle"）、`AgentNoticeBodyRole`（"agentNoticeBody"），加进 `messageChanged` 时的 dataChanged 角色表（MessageListModel.cpp:47-57）。插入补充时路径末尾追加多行，现有 `onPathChanged` 已经会把上一条的 `isStreaming` 改掉（:351）。
- QML 里直接用 `AgentRunner.isStopping`（单例属性）也可以；界面区自定。图标名（交给设计系统补映射）：`stop.circle`、`pause.circle`、`clock.arrow.circlepath`、`arrow.uturn.backward`、`text.bubble`、`arrow.triangle.2.circlepath`（旋转）、`person.2.badge.gearshape`、`paperclip`。

### 2.14 文档同步（实现时一并改）
- ARCHITECTURE.md §3.4：回合线程新增 `takeInterjection` / `drainBackgroundAgents`；`m_turnMessageIDs` 由 `m_streamMutex` 保护；§2.2 的收尾顺序。
- §5.3：`ToolEvent` 键加 `stopped?`、`usage?{…}`；`ChatMessage` 键加 `stoppedByUser?`、`interjected?`、`fromAgent?`。
- §8.2：AgentRunner 的新属性 / 方法 / 信号。§9：`Agent/QueuedInput.swift → core/agent/QueuedInput.h`。
- DebugHooks.h 文件头的环境变量列表（§4.4）。

### 2.15 建议的实施顺序（每步都能单独编译、跑测试）

1. 纯数据与纯函数：`ChatModels`（新字段、JSON、`markStopped`、`AgentUsage`）、`ToolOutput::runsInBackground`、`QueuedInput.h`、`toolResultText` / `historyMessages` / `promptText`、`AgentLog` 的 agent_message；§4.1 的模型 / 纯函数用例。`SubAgentEnded::advice` 没落地前先在测试里只测非团队工具的分支。
2. v1.5：回合状态机（§2.2）、`stop()`、`finish()`、`dispatch` 的停止分支、`runTurn` 的「先判停止」、`markResync`、`ProcessRunner` / `runShell`；改 §4.3 的现有用例，加 §4.2 的 1–5。
3. v1.6：`ChatEngine::interjection`、`OpenAIChatEngine::run` 钩子与 `userObject`、`submit` / 队列 / 投递、`send(inputs)`、`takeInterjection`、这一轮的快照与 `pendingTurnChanges`、`verifyIfNeeded` 看整轮、`ChatViewModel` / `MessageListModel` 的数据契约；§4.2 的 6–12、15–16。
4. v1.7 / v1.8：（`AgentTeamState` 头文件与空实现由骨架提供，PORT_V15_V19.md §6.C）接 `team`、`drainBackgroundAgents`、`cancelTeam`、跨消息 `updateEvent` / `eventTask`、`waitingWithoutSlot`、`noteRepeat` 与提醒、活动与步数、`SubAgentEnded` 分支；§4.2 的 13。主 Agent 池满重发（PORT §3.6.5）也在这一步。
5. v1.9：meter（每秒顺带 `team->tickClock()`）、`mainLiveSpeed`；§4.2 的 14。（`Format::meterSpeed` 由骨架实现、归 tree-tray，**已由 PORT_V15_V19.md §3.9 取代**原来的 `Format::agentSpeed`。）
6. 调试钩子（§4.4）与文档同步（§2.14）。

---

## 3. 接口

### 3.1 本区提供（给子 Agent 区、界面区、CLI）

| 接口 | 线程 | 用途 |
|---|---|---|
| `bool AgentRunner::isStopping() const` | 任意 | 子 Agent 区：`takeAgentInterjection` 的 guard、后台子 Agent 结束时「整轮在收尾就不发通知」、`waitingWithoutSlot` 拿回名额前、`endedError`、`runSubAgent` 拿到名额后、并行子卡片收尾 |
| `void AgentRunner::updateEvent(const QUuid&, std::function<void(ToolEvent&)>)`（跨这一轮的消息） | 任意 | 子 Agent 区：卡片状态、`usage`、`queued`、publish |
| `std::function<void()> AgentRunner::eventTask(const QUuid&, std::function<void(ToolEvent&)>) const` | 任意 | 子 Agent 区：`SubRun::renderPass` 的节流刷新**必须**改用它（现在用的 `messageTask(eventBody(…))` 只认正在写的那条，插入补充后后台子 Agent 的卡片在更早的消息里，流式文字会刷不上去） |
| `QList<ChatMessage> AgentRunner::currentTurnMessages(const QUuid&) const`、`QList<QUuid> turnMessageIDs() const` | 任意 | 子 Agent 区：`backgroundGroup` 找并行卡片 |
| `static int AgentRunner::estimateTokens(const QString&, bool local)` | 任意 | 子 Agent 区：`SubRun::receive` 估 token |
| `double AgentRunner::mainLiveSpeed(const QUuid&, const QDateTime&) const` | 任意 | 树 / 界面区：工具栏合计、树顶部概览 |
| `int AgentRunner::noteRepeat(const ToolCall&, ToolSink&)`、`repeatWarn/repeatBlock/stepNudge`、`pollingTools()` | 调用者线程 | 子 Agent 区：测试（SelfTests liveness） |
| `static QString AgentRunner::toolResultText(const ToolEvent&)`、`historyMessages`、`promptText`、`interjectionNote()`、`stoppedNote()` | 任意 | 测试、CLI |
| `MessageBlock::markStopped`、`AgentUsage`、`ToolEvent::stopped/usage`、`ChatMessage::stoppedByUser/interjected/fromAgent` | — | 全部 |
| `ToolOutput::runsInBackground` | — | 子 Agent 区：后台派发返回 |
| `QueuedInput` | — | 子 Agent 区：`team.queues` 的元素类型 |
| `ChatEngine::interjection` | 引擎线程 | 子 Agent 区：子 Agent 引擎设 `takeAgentInterjection` |
| 信号 `stoppingChanged`、`queuedInputsChanged(QUuid)`、`queueRevisionChanged`、`liveMeterChanged`、`turnMessageStarted(QUuid, QUuid)` | 主线程 | 界面区、托盘、CLI |
| 属性 `isStopping`、`liveAgents`、`liveSpeed`、`queueRevision`、`team`；`Q_INVOKABLE submit / removeQueued / resumeQueue / queuedInputs / isQueuePaused / pendingTurnChanges / mainLiveSpeed` | 主线程 | QML |
| ~~`Format::agentSpeed(double)`~~ **已由 PORT_V15_V19.md §3.9 取代**：菜单栏 / 托盘的写法叫 `Format::meterSpeed`（`agentSpeed` 是树里的「tok/s」写法）；骨架实现，测试归 tree-tray | 任意 | 托盘 |

### 3.2 需要子 Agent 区提供（C++ 声明——两边按这个对齐；谁先落地谁先建头文件，主循环可以先用只含这些成员的空实现）

> **本节声明已由 PORT_V15_V19.md §3.3–§3.5 取代**（骨架一次加齐）。主要差别：`Activity` 是 `ActivityKind` 枚举 + `title` 字段 + 工厂 `tool(title)`；
> `waitForNotice(key, timeoutMs)` 不带令牌参数；整轮停止用 `AgentTeamState::cancelRuns(conv)`，`cancelTeam()` 由 core 实现；信号只有 `changed / treeFocusChanged / clockChanged`；
> 样本类型 `AgentTeamState::Sample`；`waitingWithoutSlot(ToolSink&, const Activity&, const std::function<void()>&)`。下面「主循环依赖的子 Agent 区行为」仍然有效。

```cpp
// core/agent/AgentTeamState.h（== Swift AgentTeamControl.swift:13-194 的 AgentTeamState）
// 线程：内部 QMutex，任意线程可调；界面观察的部分变化时在主线程发信号
class AgentTeamState : public QObject {
    Q_OBJECT
public:
    struct Activity {
        enum class Kind { Generating, Tool, WaitingChildren, WaitingApproval, Queued };
        Kind kind = Kind::Generating;
        QString title;                                     // Kind::Tool 的工具显示名
        static Activity generating() { return {}; }
        static Activity tool(const QString& title) { return { Kind::Tool, title }; }
        static Activity waitingChildren() { return { Kind::WaitingChildren, {} }; }
        static Activity waitingApproval() { return { Kind::WaitingApproval, {} }; }
        static Activity queued() { return { Kind::Queued, {} }; }
        bool operator==(const Activity&) const = default;
    };
    struct Notice {
        enum class Kind { Report, Finished, Failed, Stopped };
        QUuid agentID;
        QString agentName;
        Kind kind = Kind::Report;
        QString text;
    };
    /// 00000000-0000-0000-0000-00000000000A
    static QUuid mainKey();

    // —— 主循环要用的（缺一不可）——
    QList<Notice> takeNotices(const QUuid& key);                          // 先进先出，取走即清空
    bool hasNotices(const QUuid& key) const;
    QList<QUuid> backgroundChildren(const QUuid& key, const QUuid& conversationID) const;
    /// 阻塞到：key 的收件箱非空 / 被 wake / 超时 / token 取消。不抛异常（== Swift waitForNotice）
    void waitForNotice(const QUuid& key, std::optional<int> timeoutMs = std::nullopt,
                       const CancelToken* token = CancelToken::current());
    void wake(const QUuid& key);                                           // 叫醒等这个 key 的
    void cancelRuns(const QUuid& conversationID);                          // == for run where conversationID: run.task?.cancel()
    void reset(const QUuid& conversationID);                               // 取消并注销这段的 run、叫醒全部、清空全部通知、清 launched[id]
    QSet<QUuid> running() const;
    double speed(const QUuid& agentID, const QDateTime& now = QDateTime::currentDateTimeUtc()) const;
    static double windowSpeed(const QList<std::pair<QDateTime, int>>& samples, const QDateTime& now);   // §1.4.5，主循环也用
    void setActivity(const QUuid& agentID, const Activity& activity);    // 没注册的 id 忽略；也记一次「有动静」
    void noteStep(const QUuid& agentID);
    // —— 其余（register/unregister/post/queues/launched/activity/steps/started/lastOutput/deadline/noteTokens/endPass/
    //     treeFocus/watchdog 及其信号）由子 Agent 区定 ——
signals:
    void runningChanged();                                                 // 主线程（meter 不靠它，按秒轮询）
};

// core/agent/AgentLiveness.h（== AgentLiveness.swift:13-40）
class SubAgentEnded : public FridayError {               // message() == cardText()
public:
    SubAgentEnded(std::optional<QString> stoppedBy, QString failure, std::optional<QString> partial);
    const std::optional<QString>& stoppedBy() const;     // nullopt = 失败
    QString cardText() const;                            // §1.4.3
    static QString advice(const QString& cardText);      // §1.4.3 的四句，主循环 toolResultText 要用
};

// AgentRunner（子 Agent 区实现在 AgentTeamControl.cpp / SubAgentRunner.cpp）
    static QString noticeText(const AgentTeamState::Notice& notice);   // §1.3.2，主循环 takeInterjection 用
    static QString noticeTitle(const QString& text);                   // §1.3.2，界面汇报卡片用
    static QString agentMessageTool();                                 // "agent_message"，AgentLog 用
    /// == Swift waitingWithoutSlot(sink, as:, body)：sink 是 SubRun 时标成 waiting、让出本地名额，body 结束后（没被停止时）
    /// 以 resumeBoost + depth 的优先级拿回名额并标回 Generating；主 Agent 直接执行 body。body 的异常原样传出；拿回名额时被取消抛 CancellationError
    void waitingWithoutSlot(ToolSink& sink, AgentTeamState::Activity waiting, const std::function<void()>& body);

// AgentRunner::SubRun 追加
    int steps() const;                                    // consumeStep 的计数
    std::optional<std::pair<QString, int>> lastCall;      // 只在运行它的线程读写
    // replaceKeepingTranscript 也要复制 stopped

// ToolRegistry 追加 / 改
    QList<ToolSpec> mainAgentSpecs() const;               // activeDescriptors 去掉 report_to_parent（任意线程，自己转主线程）
    QString guidance(AgentTeamTools::Role team) const;    // 替换 guidance(bool includeTeam)
// AgentTeamTools 追加
    enum class Role { Main, Sub };
    static QString messageAgentName();   // "message_agent"
    static QString stopAgentName();      // "stop_agent"
    static QString waitAgentsName();     // "wait_agents"（pollingTools 用）
    static QString reportToParentName(); // "report_to_parent"
```
另外主循环依赖的子 Agent 区**行为**：
- `team.reset` 之后不再有该对话的运行中子 Agent；后台子 Agent 结束时若 `isStopping()` 就不发通知。
- 后台子 Agent 的卡片更新走 `updateEvent` / `eventTask`（跨消息搜索）。
- 子 Agent 的取消令牌不一定是回合令牌的子令牌（后台子 Agent 活得比派发它的 dispatch 久），所以 `stop()` 必须靠 `cancelRuns` 取消它们。
- `stopAgent(id, by)`、`sendToAgent(id, text, attachments, from)`、`team.treeFocus` 供调试钩子使用（§4.4）。

### 3.3 需要界面区 / 在线限流区注意的
- 界面：§1.1.10、§1.2.10、§1.3.2 的全部文案；`ModelStatusView.qml` 生成中不再禁用；停止键在 `isStopping` 时禁用并换旋转图标（Qt 原生控件里用一个旋转的 `Icon`，`RotationAnimator` 0.9 s）；排队气泡接在 `MessageList` 末尾（不进 `MessageListModel`，用 `ChatViewModel.queuedInputs`——名字**已由 PORT_V15_V19.md §3.11.2 取代**原来的 `queuedItems`）。
- 托盘两行字：Windows 通知区域图标是正方形，放不下「x AI / 179 t/s」两行（风险 §5-10）。
- 限流：§2.6.4（闸门跳过本地引擎）。

---

## 4. 测试与调试钩子

### 4.1 SelfTests.swift → QtTest（逐条断言）

| Swift（origin/main 行） | Qt 用例（文件 / 类 / 名） | 断言 |
|---|---|---|
| stopState 656-679 | tst_models.cpp `TestModels::markStopped` | 用 Swift 同样的结构：`done`(succeeded) / `parallel`(running，transcript：`runningChild`(running，transcript：`innerShell` running)、`queuedChild`(running, queued, transcript 空)) / `approval`(awaitingApproval)。`markStopped` 后：done 状态不变且 `stopped` 为 nullopt；parallel `Failed`、`stopped == true`、result「已停止」；两个子卡片 `stopped == true` 且 `queued` 为 nullopt；innerShell `stopped`、`Failed`；approval `stopped`、`Failed`、result 含「没有执行」；`containsPendingApproval == false` |
| stopState 681-686 | `TestModels::pendingApprovalAndInterrupted`（已有） | markInterrupted 结果 `["等待确认时应用退出，已中断", "应用退出时仍在运行，已中断"]`（Qt 已满足，保留） |
| stopState 688-700 | tst_runner.cpp `TestRunner::stoppedHistory` | `toolResultText(approval 停止后)` 含「不要自动重试」、不含「修正后重试」；`historyMessages([user「整理文件」, answer(text「我先列一下目录」+ approval 块, stoppedByUser)])`：最后一条是 assistant、content == `stoppedNote()`；含一条 tool、content == 上面的停止文字；`historyMessages([user「你好」, 空 assistant(stoppedByUser)])` 共 2 条、`[1].content == stoppedNote()` |
| queueState 641-652 | `TestRunner::interjectedPromptText` | `promptText(user「告诉我今天星期几」) == 原文`；`interjected = true` 后 `== interjectionNote() + 原文`；只有附件的补充：`interjectionNote() + "\n\n[用户附带的文件]\n…"`（不出现「（请看附件）」）；`pendingTurnChanges(随机 UUID)` 两项都 false |
| queueState 648-651 | `TestModels::legacyMessageKeys` | `ChatMessage::fromJson({"id":"2F1E7C1A-0000-4000-8000-000000000001","role":"user","blocks":[],"createdAt":0})` 成功、`interjected/stoppedByUser/fromAgent` 都是 nullopt；新键往返（只写 true / 非空） |
| teamState 404-410 | tst_context.cpp `TestContextAssist::agentLogAgentMessage` | transcript 里 `ToolEvent{name:"agent_message", arguments:"改成只查官方文档", title:"用户补充"}` → `AgentLog::render` 含「【用户补充】改成只查官方文档」；没有 title 时「【补充】」 |
| teamState 384-403 | （子 Agent 区的 `TestTeam`）| noticeText / noticeTitle / agentMessageNote / 收件箱 FIFO——本区只要求 `takeInterjection` 用的 `noticeText` 与之一致 |
| liveness 501-504 | `TestRunner::toolResultTextStoppedTeamTool` | `ToolEvent{name: delegate_task, status: Failed, stopped: true, result: SubAgentEnded("看门狗（5 分钟没有任何输出）", "", "做到一半的结果：…").cardText()}` → 文字以「这个子 Agent 被看门狗」开头、含「做到一半」「换个做法」；`result =「已停止」` → 「这个子 Agent 被停止了，没做完。…」；`run_parallel_tasks` 同；`read_agent_log` + stopped → 普通「用户停止了这一轮…」 |
| liveness 524-530 | `TestRunner::noteRepeatCounts` | SubRun sink：同一 `run_shell {"command":"echo same"}` 三次 → `[1,2,3]`；换参数 → 1；`wait_agents` → 0。主 Agent（FakeSink 不是 SubRun，走 m_mainLastCall）同样 `[1,2,3]` |
| agentUsage 598-609 | `TestModels::agentUsage` + `TestRunner::windowSpeedAndEstimate` | `AgentUsage{glm-5.3-flash, 智谱 GLM, 中, 0.6, 300, 9000, 6}`：`tokensPerSecond() == 50`、`detail() == "深度思考：中\n温度：0.6\n服务：智谱 GLM"`；JSON 往返且 7 个键齐全（service 为空时没有键）；`windowSpeed([(now-2,10),(now-1,20),(now,20)], now) == 20`、`now+5 → 0`；`estimateTokens("<think>", false) == 0`、`estimateTokens("你好", true) == 1` |
| run 44-45 | **已由 PORT_V15_V19.md §7.1 取代**：tree-tray 的 `TestTreeTray::formats`（tst_tree_tray.cpp），函数名 `Format::meterSpeed` | 向量不变：`[0, 8.46, 179.4, 1234, 3400000, 1.1e12]` → `["0 t/s","8.5 t/s","179 t/s","1.2 Kt/s","3.4 Mt/s","1.1 Tt/s"]` |
| teamDepth / agentTree / agentUsage 610-636 / requestGate / localScheduler | 子 Agent / 树 / 限流区 | — |

### 4.2 新增集成用例（tst_runner.cpp，用现有 `FakeChatServer`；tst_engines.cpp 用 `MockServer` / `FakeLlama`）

1. `stopShowsStoppingState`：hold 住请求 → `stopGenerating()`：`isStopping()` 立刻 true、`stoppingChanged` 1 次；再调一次 `stop()` 信号不增加；结束后 `isStopping()` false、`stoppingChanged` 共 2 次；最后一条助手消息 `stoppedByUser == true`、`blocks` **为空**、无 stats、无 error；不在生成时调 `stop()` 什么也不发生。
2. `stopRunningShellMarksStopped`：模型调 `run_shell`（Windows `ping -n 30 127.0.0.1`，其他 `sleep 30`）；等卡片 running 后停：1 秒内 `!isGenerating()`（`QElapsedTimer`），卡片 `Failed`、`stopped`、result「已停止」；子进程已不在（Windows 查不到进程 / Unix `kill(pid,0)` 失败）。
3. `stopDuringApproval`：自动模式下 `rm -rf …` 等确认时停：卡片 `Failed`、`stopped`、result「等待确认时被停止，没有执行」，`hasPendingApproval == false`。
4. `stoppedTurnResyncsHistory`：上面某个停掉的对话再发一句：服务端收到的 messages 里，被停那轮的助手消息内容以 `stoppedNote()` 结尾（或单独一条），停掉的工具结果 == 「用户停止了这一轮，…」；在线引擎这一轮新建了会话（第二个请求的历史是存档重建的）。
5. `noRecoverAfterStop`：hold 住后停止（连接被断）：消息上没有「遇到内部错误」notice、没有 error，没有第二个请求。
6. `queueInterjectsAfterTool`：`send` 之后**同步**马上 `submit("顺便看看评价")`（此时 `isGenerating()` 已为真，回合线程还没发请求，排队是确定的）；第一段回一个会被执行的工具调用（如 `read_file` 一个临时文件），第二段回正文：第二个请求的最后一条 user 消息 == `interjectionNote() + "顺便看看评价"`（排在 tool 消息之后）；存档顺序：user、assistant A（工具块 + 定稿 stats，`isLive() == false`）、user（`interjected == true`）、assistant B（第二段正文，有自己的 stats）；`queuedInputs(id)` 为空；`turnMessageStarted` 发过一次且 messageID == B。
7. `queueAfterLastStep`：`send` 后同步 `submit` 两条；第一段只回正文（没有工具调用，不经过 interjection）：这一轮结束后自动开新一轮，新请求末尾是**两条**连续的 user（不带 interjectionNote），只有一条助手回复；第二轮的临时标题不变（会话已非空）。
8. `queuePausedAfterStop`：hold 住请求、`submit` 一条、再停：`isQueuePaused(id)`，没有新请求；`resumeQueue(id)` 后发出。另：`removeQueued` 删空后 `isQueuePaused` 也清掉；暂停时再 `submit` 一条 → 解除暂停、两条一起发。
9. `submitWhileOtherConversationGenerating`：`send` A 后同步给 B、C `submit`（B 先），并把 C 设为选中：A 结束后先发 C、再发 B（选中的优先于更早排队的）。
10. `replacingWhileBusyIgnored`：生成中 `submit(…, replacing)`：不排队、不发、无信号。
11. `switchEngineMidTurn`：`send`（引擎 X）后同步 `setEngine(Y)`：`pendingTurnChanges(id).model == true`；这一轮的请求都打到 X，统计 `engineName` 是 X 的；空闲后两项 false；下一轮请求打到 Y。同理 `setThinking` → `.thinking == true`。
12. `verificationCheckpointAcrossInterjection`：A 段 `write_file` 一个 `.py`，插入补充后 B 段不运行它：仍触发验证提醒，notice 落在 B（当前消息）上。
13. `repeatedToolCallBlocked`：服务端连续 5 次回同一个 `read_file {"path": <临时文件>}`，第 6 段回正文（`get_current_time` 是轮询类、不计数；不存在的工具名在预检就被退回，不进打转计数）：第 3 次的 tool 结果以打转提醒结尾；第 5 次的 tool 结果 == 「调用被拒绝（未执行）：同样的调用已经连续 5 次了，…」且那张卡片 `Failed`；诊断日志有「拦下重复调用 read_file（连续 5 次）」；下一轮 `m_mainLastCall` 重新计数。
14. `liveMeter`：生成中 `liveAgents() == 1`，`liveMeterChanged` 发过；结束后 ≤ 1.5 s 归 0。
15. tst_engines `interjectionAfterToolResults`：`OpenAIChatEngine` 设 `interjection` 返回 `[user("补充")]`：第二个请求 messages 顺序 = …assistant(tool_calls)、tool、user「补充」；没有工具调用的轮次不调钩子；钩子前取消 → CancellationError。同一用例跑 `FakeTransportEngine`（进程内路径）。
16. tst_engines `streamMessagesFramesVideos`：视觉配置下 `stream(messages)` 里带视频的 user 消息被抽帧（复用 `videoFrames` 用例的样例视频），文字带「（视频已按时间顺序抽帧附在图片里）」。
17. tst_util `processGroupKilledOnCancel`（Unix，CI 上跑）：`sh -c 'sleep 30 & sleep 30'`，取消后两个 sleep 都不在。

### 4.3 必须改的现有用例

| 用例（tst_runner.cpp） | 原期望 | 新期望 |
|---|---|---|
| `toolResultText` 470-483 | Running 按失败处理 | **Running → `modelText`（成功文字）**；AwaitingApproval 仍按失败；另加 stopped 两种 |
| `dispatchApprovalStopped` 1055-1068 | 没有回合时 `stopGenerating()` → Denied | 新 `stop()` 不在生成时无效，会一直卡在审批 → 改成真实回合（同 4.2-3），或直接取消令牌（见下一行） |
| `dispatchApprovalCancelled` 1070-1085 | 令牌取消 → 「用户拒绝执行该操作。」、Denied | 返回「用户停止了这一轮，这个调用没有完成（结果未知，可能执行了一部分）。不要自动重试，等用户下一步的指示。」；`replaced().last()`：Failed、`stopped == true`、result「等待确认时被停止，没有执行」 |
| `parallelStopped` 1346-1380 | 父卡片 Failed「已停止」、子卡片 Failed | 父卡片另有 `stopped == true`；子卡片 `stopped == true`、result「已停止」；消息 `stoppedByUser == true` |
| `stopImmediately` 1710-1734 | 写「（已停止）」 | `blocks` 为空、`stoppedByUser == true`、无 stats、无 error；（新增）没被停止的空回复写「（没有输出）」另起用例 |
| `stopAfterReentrantSend` 1736-1775 | `messages.at(3)` 是「（已停止）」 | `messages.at(3).blocks` 为空、`stoppedByUser == true`；并断言重入的新一轮 `isStopping()` 一开始是 false（§2.2 的顺序） |
| `onlineTurn` 1087-1146 | 生成中 `send` 被忽略 | 不变（`send` 仍忽略；`submit` 才排队） |

### 4.4 调试钩子

**core `DebugHooks`（DebugHooks.cpp，调试构建）**——Swift DebugHooks.swift:12-19、45-62、111-233 一一对应，全部在主线程用定时器链轮询（现有 `waitUntil` 风格）：

| 环境变量 | 行为 | stdout / 日志（逐字） |
|---|---|---|
| `FRIDAY_AUTOSTOP=<条件>` | 第一轮里条件满足时 `runner->stop()`，测从点停止到 `!isGenerating()` 的秒数 | 点时打印 `FRIDAY_AUTOSTOP clicked`；停下后打印 `FRIDAY_AUTOSTOP stopped_in=%.2f`；日志「FRIDAY_AUTOSTOP：条件 <c> 满足后点了停止，<s> 秒后停下」 |
| `FRIDAY_QUEUE="<条件>\|<文字>\|<文字>…"` | 条件满足时对**正在生成的那段**逐条 `submit` | `FRIDAY_QUEUE submitted`；日志「FRIDAY_QUEUE：已提交 N 条，排队 M 条」 |
| `FRIDAY_SWITCH="<条件>\|engine:<id>\|thinking:<档位>"` | `engine:local` → 本地；`thinking:` 取 off/low/medium/high | `FRIDAY_SWITCH pending_model=<true\|false> pending_thinking=<true\|false>`；日志「FRIDAY_SWITCH：已切换，下一轮生效：模型 <b> 思考 <b>」 |
| `FRIDAY_AGENT_MESSAGE="<条件>\|<文字>"` | 条件满足后等到有运行中的子 Agent，`sendToAgent(id, 文字)` | `FRIDAY_AGENT_MESSAGE sent`；日志「FRIDAY_AGENT_MESSAGE：已发给子 Agent「<名>」」 |
| `FRIDAY_STOP_AGENT="<条件>[\|<名字>]"` | 等到有（指定名字的）运行中子 Agent，`stopAgent(id)` | `FRIDAY_STOP_AGENT stopped <名>` |
| `FRIDAY_TREE_FOCUS="<条件>\|<名字或「最深」>"` | 把 `team->treeFocus` 设成那个子 Agent（「最深」= depth 最大的） | `FRIDAY_TREE_FOCUS <名>` |
| `FRIDAY_SLOW_STOP=<秒>` | 在 `AgentRunner::stop()` 里（§2.2） | — |
| `FRIDAY_STALL_LIMIT`、`FRIDAY_LOCAL_SLOTS` | 子 Agent 区实现（看门狗时长、本地并发上限），只在 DebugHooks.h 的说明里列出 | — |

条件（`waitForCondition`，DebugHooks.swift:111-142）：先等第一次 `isGenerating()`，然后每 250 ms 看一次，满足后再等一段才算数：
- `approval`：当前助手消息 `containsPendingApproval`，+1 s；
- `tool:<名>`：当前消息的块里（递归进 transcript）有这个名字且 `Running` 的调用，+1.5 s；
- `text:<秒>`：当前消息有非空正文块（Qt 每 0.1 s 已把文字写进块，不需要 live 注册表），+<秒>；
- `after:<秒>`：从开始生成算起 ≥ <秒>，+0；
- 看不懂 → 日志「调试钩子：看不懂的条件 <c>」并放弃；第一轮在满足前结束 → 放弃。
「当前消息」= `generatingConversationID` 路径上的最后一条（或用 `turnMessageIDs().last()`）。

**app `DebugHarness`**：无须新增步骤即可截图（`until:AgentRunner.isStopping`、`eval:AgentRunner.queuedInputs(ConversationStore.selectedID).length`）。
~~建议加 `wait:stopping` / `wait:queued`~~——**已由 PORT_V15_V19.md D19 取代**：不加，用 `until:`；DebugHarness 只由 tree-tray 改（`drag:`、`traytip`、`#` 注释行）。
配合 `FRIDAY_SLOW_STOP=5` 截「正在停止…」、配合 `type:` + `key:Return` 截排队气泡。

---

## 5. 风险 / 待定问题

1. **重入顺序**（§2.2）：`setGenerating(nullopt)` 同步发信号，任何在它之后才清的本轮状态都会污染重入的新一轮。实现后必须跑 `stopAfterReentrantSend`（加 `isStopping()==false` 断言）。
2. **`stop()` 的语义变了**：不在生成时无效、只认第一次。现有直接调 `dispatch` + `stopGenerating()` 的用例会卡死（§4.3）。friday-cli 的「第二次 Ctrl+C 退出」不受影响（它自己计数）。
3. **流式刷新跨消息**：`SubRun::renderPass` 现在用 `messageTask(eventBody(...))` 只改正在写的那条。插入补充 / 汇报后，后台子 Agent 的卡片留在旧消息里，必须改用 `eventTask`（§3.1），否则流式文字刷不上去（只有 publish 时才更新）。
4. **自愈的边角（照搬 Swift，待定）**：插入补充之后马上出错且新消息还没有任何产出时，`recover` 会去掉「最后一条用户消息」（那条补充）再重发**这一轮开头的 prompt**——实际上重发错了消息。Swift 同样如此；建议先照搬并在 Swift 侧提 issue，或两边一起改成重发最后一批补充。
   **已定：照搬，列入 PORT_V15_V19.md §8 待用户决定（附两边同改的修法）。**
5. **任务状态的「像要求的原话」会抓到汇报卡片**：`TaskState.requirementMessages` 扫所有 role == user 的消息，停止通知里有「不要重新派发…」（含「不要」）。Swift 现状如此，照搬；建议两边一起让它跳过 `fromAgent` 消息（属于 TaskState，不在本区，记给负责人）。
   **已定：照搬，列入 PORT_V15_V19.md §8。**
6. **子进程模式下 KV 池满会连累主 Agent**：后台本地子 Agent 和继续工作的主 Agent 同时跑时，llama-server 让所有在处理的槽位一起失败，主 Agent 收到 `KVCacheFullError` → 走 `recover`（提示「遇到内部错误…」）。进程内只牺牲一个非主序列，问题小得多。
   **已定（PORT_V15_V19.md §3.6.5 / Q2、§3.6.3 / Q3）**：主 Agent 等一个本地子 Agent 结束（`scheduler.waitForLocalEnd(2000)`）后原样重发这一段，每次 consume 最多 2 次，再不行才 `recover`；
   子 Agent / 看图 / 起标题的请求不落主序列（子进程模式改用显式的非主槽位 `sharedSlot()`，不再发 `-1`）。
7. **工作中换思考强度的不一致**（§1.2.5）：同一轮后续的 `configure` 会把新思考开关带进旧引擎。照搬；如果要「严格这一轮照旧」，需要在 `configure` 里对正在跑的这一轮用 `m_turnThinking`——那样就和 Swift 不同了，先不做。
8. **在线引擎被停时的部分文字**：Qt 的 `requestPass` 取消时把半截正文留在 `m_history`（`cancelKeepsPartialText`），v1.5 起主 Agent 下一轮会按存档重建，这段逻辑对主 Agent 不再起作用（子 Agent 引擎用完即弃）。保留无害；不要依赖它。
9. **`AgentUsage.detail` 的小数点**：Swift 用当前区域格式化（德语区会是「0,6」）；Qt 用 `QLocale()` 对齐。单元测试要把区域固定成 C / zh_CN，否则在逗号小数点的机器上失败。
10. **托盘两行字**：Windows / Linux 的托盘图标是正方形，Swift「图标 + 两行文字画成一张模板图」做不到。
    **已定（PORT_V15_V19.md §3.11.6 / Q9）**：图标不变、不加角标；tooltip 三行「Friday / N AI / 速度」；托盘菜单状态行下一行「N AI 在工作 · 速度」（N > 0）；
    速度写法 `Format::meterSpeed`（不是 `agentSpeed`）。数据（`liveAgents` / `liveSpeed`）按本文提供。
11. **`|| ` 短路**（§1.1.5）：折叠和「上一轮被停」同时发生时 `stoppedConversations` 不会被移除，下一轮再同步一次。无害，照搬；Qt 里 `markResync` 按 Fold 处理（多一次回到前缀，仅限进程内、从磁盘恢复过、不能截断的模型）。
12. **并行派发父卡片的状态**：Swift 整轮停止后父卡片仍是 `succeeded`（只有子卡片被标停止），Qt 现有实现把父卡片标「已停止」（`CancelToken::checkCurrent()`，SubAgentRunner.cpp:342）。保留 Qt 的做法并加上 `stopped = true`（与「统一标成已停止」的描述一致），在文档里注明这是有意的差异。
13. **「重新输入」在忙或有暂停队列时被静默忽略**（`submit` 的 `guard replacing == nil`）：Swift 现状；界面上「重新输入」按钮生成中本来就禁用，只有「空闲但有暂停的排队」这一种情况会吞掉——照搬，记录在案。
14. **排队投递最多等 20 分钟**：本地模型缺失且没开自动下载时，`prepareModel` 只会标「未下载」，排队的消息一直显示「排队中：模型就绪后发出」，20 分钟后轮询停止、不会再自动发出（直到下一次 `submit` / `resumeQueue` / 回合结束重新安排）。照搬。
15. **meter 与 `m_recentChunks` 的锁**：主线程每秒读、回合线程每个 chunk 写；用 `m_liveMutex` 保护，临界区只做 append / removeIf / 复制，不要在锁里发信号或 `runOnMainSync`。
16. **CLI 跟消息**（§2.12）：不改的话后台子 Agent 汇报之后的回复在终端里看不到。
