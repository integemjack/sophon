# 子 Agent 系统移植规格（Swift v1.5–v1.9 → Qt）

> 只读分析的产物。来源：`origin/main`，merge-base `44116fb` 之后的五个合并：e71cf89（v1.5 停止状态）、76534ff（v1.6 工作中补充）、
> e3aa578（v1.7 父子协作 = 7f53e44 + 33aff41）、f1f6d58（v1.8 层数不限 = 95d4df0 + a3b7284 + 2084be2 + 9096118 + e672b61）、e2f8f41（v1.9 菜单栏计量）。
> 引用写成 `文件:行号`，路径相对 `Friday/Friday/`，行号按 origin/main。Qt 路径相对 `qt/`。
>
> **附录说明**：本文是 `qt/docs/PORT_V15_V19.md`（总契约）的附录，对应工作包 **team**。接口、归属、决定以总契约为准；标「**已由 PORT_V15_V19.md §x 取代**」的段落不再生效，
> 其余（行为、逐字文案、Swift 出处、测试向量）照做。

## 0. 范围、分工、约定

**本文负责**：父子 Agent 的团队状态（`AgentTeamControl.swift`）、活性（`AgentLiveness.swift`）、在线并发闸门（`LLM/Engine/RequestGate.swift` + `OpenAIChatEngine` 的 v1.7 部分）、
`SubAgentRunner.swift`、`SubAgentScheduler.swift`、`AgentProfiles.swift`、`ToolRegistry` / `ToolTypes` 的团队部分、`AppSettings.subAgentTurnLimit`、`ChatModels` 的子 Agent 字段
（`ToolEvent.stopped/usage`、`AgentUsage`、`ChatMessage.fromAgent`）、`EngineStore` 的「起步并发」、Agent 树的**数据**（`Views/AgentTreePanel.swift` 里的 `AgentNode / AgentTree / UsageSummary / AgentTreeRow`）、
托盘计量的**数据**（`AgentRunner.liveAgents / liveSpeed`、`MenuBarLabel.speedText`）、`Tools/Perf/mock_sse.py`、`DebugHooks` / `SelfTests` 的对应部分。

> **归属已由 PORT_V15_V19.md §5 取代**：计量（`liveAgents / liveSpeed`、`startMeter / updateMeter`、主 Agent 速度窗口、信号 `liveMeterChanged`）归 **core**；
> `MenuBarLabel.speedText` → `Format::meterSpeed`（骨架实现、归 tree-tray）；`ChatModels` 的新字段、`AppSettings.subAgentTurnLimit`、`ToolOutput.runsInBackground`、
> `AgentTree` 纯函数由**骨架**一次写好（AgentTree 之后归 team 维护）；原地打转 `noteRepeat` 与常量归 core；`AgentTreeModel` 归 tree-tray；DebugHooks 文件归 core
> （本文的 `FRIDAY_STALL_LIMIT / FRIDAY_LOCAL_SLOTS / FRIDAY_SLOW_AGENT_STOP` 在 team 的代码里读）。

**不负责**（另有分析；本文只给出它们要调用的接口，见 §3.2「给主循环的钩子」）：AgentRunner 主循环本体——v1.5 的 `isStopping` / `MessageBlock.markStopped` / `stoppedNote` / 停止后的历史同步，
v1.6 的 `queuedInputs` / `submit` / `takeInterjection`（主 Agent）/ `drainBackgroundAgents` / `turnMessageIDs` / `pendingTurnChanges`；全部界面（卡片、树面板、工具栏、托盘的绘制）。

**文案**：所有模型或用户看得到的文字逐字照抄；下文用代码块或「」给出，`{x}` 表示插值。Qt 用 `QStringLiteral(...).arg(a, b, …)` 一次替换（名字、id 里可能有 `%2`）。
Swift 的 `String.count / prefix(n) / suffix(n)` 按字位簇计，Qt 用现有的 `AgentRunner::prefixCharacters / characterCount`（QTextBoundaryFinder）。

**Qt 现状（读代码确认）**：qt/ 的子 Agent 停在 44116fb：`AgentTeamTools::maxDepth = 2`（`src/core/agent/AgentProfiles.h:44`）；`SubAgentScheduler` 还有 `acquireOnline / releaseOnline`；
嵌套子 Agent 靠 `thread_local t_heldLeases` 借父级名额防死锁（`src/core/agent/SubAgentRunner.cpp:54-73`）；`delegate_task` 在调用方线程里直接跑，`run_parallel_tasks` 每个任务一个 `Worker::spawn`；
没有后台、消息、单独停止、看门狗、用量、闸门、`ToolEvent.stopped/usage`、`ChatEngine.interjection`。

---

## 1. 行为规格（按版本）

### 1.1 v1.5（e71cf89）里和子 Agent 有关的部分

- `ToolEvent.stopped: Bool?`（`Models/ChatModels.swift:74`）：「用户点了停止，这个调用没有跑完（状态记为 failed，界面显示灰色「已停止」，回灌给模型时说明不要自动重试）」。
- `SubRun.replaceKeepingTranscript` 多拷一个字段 `stored.stopped = event.stopped`（`Agent/SubAgentRunner.swift:141-149`）。
- 排队期间整轮被停：拿到本地名额也不开跑（`SubAgentRunner.swift:486-487`：`if Task.isCancelled || isStopping { throw CancellationError() }`，名额由 defer 归还）。
- 整轮停止时，并行派发的子卡片按「已停止」收尾，不显示 CancellationError 英文（`SubAgentRunner.swift:290-301`，见 §1.2.2 的并行规则）。
- 主循环 `finish()` 在停止时 `MessageBlock.markStopped`（`ChatModels.swift:159`）：running / awaitingApproval 的调用（递归进运行记录）→ `status = .failed`、`stopped = true`、`queued = nil`、
  `result = awaitingApproval ? "等待确认时被停止，没有执行" : "已停止"`。（主循环负责；子 Agent 卡片依赖它把 `queued` 清掉。）
- 回灌给模型的文字（`AgentRunner.swift:997-1014`，主循环负责，这里给出与团队有关的两支）：
  - `stopped == true` 且 `isTeamTool(name)`（delegate / parallel）：`result` 是 `"已停止"` 或空 → 「这个子 Agent 被停止了，没做完。不要重新派发同样的任务；按已有信息继续，或告诉用户需要什么。」；
    否则 → `"这个子 Agent " + result + "\n\n" + SubAgentEnded.advice(forCardText: result)`。
  - `status == .running`（后台派发，工具已经正常返回）和 `.succeeded` 一样回灌 `ToolOutput(result, media).modelText`。

### 1.2 v1.6（76534ff）里和子 Agent 有关的部分

- `ChatEngine.interjection: (@Sendable () async -> [EngineMessage])?`（`LLM/Engine/ChatEngine.swift`，`OpenAIChatEngine.swift:19`）。`OpenAIChatEngine.run` 在**一段生成的全部工具调用执行完、
  发起下一段请求之前**调一次（`OpenAIChatEngine.swift:224-229`）：`try Task.checkCancellation()`，把返回的每条消息经 `userObject`（图片直接附、视频抽 6 帧，同 `stream(prompt:)`）接在工具结果后面。
  没有工具调用的最后一段之后**不调**。v1.6 只有主 Agent 设它；**v1.7 起子 Agent 也设**（ChatEngine.swift 里「子 Agent 不接收补充」的注释已过时，按代码）。
- `QueuedInput`（`Agent/QueuedInput.swift:5-12`）：`id`、`text`、`attachments: [URL]`、`createdAt`、**`from: String?`**（v1.7 加：nil = 用户；否则是父 Agent 的名字，如「主 Agent」「父 Agent「X」」「Friday」）。
- `stream(messages:)` 里 user 角色的消息也走 `userObject`（图片 / 视频帧），子 Agent 收到带附件的补充时用得到（`OpenAIChatEngine.swift:106-121`）。

### 1.3 v1.7（e3aa578）父子协作

#### 1.3.1 团队状态 `AgentTeamState`（`Agent/AgentTeamControl.swift:12-194`，`@MainActor @Observable`）

```
Run（:15-27）    name, parentKey(UUID：主 Agent = mainKey，子 Agent = 它的事件 id), background, conversationID,
                 assignment（父级交给它的任务原文）, task（它的 Task，可取消）, stoppedBy: String?, deadline: Date?
Activity（:30-36） generating | tool(String 标题) | waitingChildren | waitingApproval | queued
Notice（:39-45）  agentID, agentName, kind(report | finished | failed | stopped), text
mainKey（:47）    UUID "00000000-0000-0000-0000-00000000000A"
```

| 字段（:49-71） | 界面观察 | 含义 |
|---|---|---|
| `runs[id]` | 否 | 登记的子 Agent |
| `subRuns[id]` | 否 | 它的 SubRun（本地块副本、用量…），在 `runSubAgent` 建好引擎后登记 |
| `running: Set` | **是** | 正在运行、可停 / 可发消息的（卡片据此显示控件） |
| `queues[id]: [QueuedInput]` | **是** | 发给它、还没送到的消息（卡片上可撤回） |
| `notices[key]: [Notice]` | 否 | 父级（key）的收件箱 |
| `launched[conv]: Int` | 否 | 这一轮已派的子 Agent 总数（所有层） |
| `activity[id]`、`steps[id]` | **是** | 在做什么、调了几次工具 |
| `started[id]`、`lastOutput[id]` | 否 | 开始时间、最近一次有输出（或换状态）的时间；界面按秒自己读 |
| `deadlineWarned: Set` | 否 | 已经提醒过期限的 |
| `recentTokens[id]: [(time, tokens)]`、`passTokens[id]` | 否 | 最近 3 秒的 token（实时速度）、这一段还没结束的估算 token |
| `treeFocus: UUID?` | **是** | 要在 Agent 树面板里打开的子 Agent |
| `watchdog`、`waiters[key][token]` | 否 | 看门狗任务、等收件箱的 continuation |

操作：
- `register(id, run)`（:73）：`runs[id]=run`；`running += id`；`activity = .generating`；`started = lastOutput = now`。
- `unregister(id)`（:81）：清掉 runs / subRuns / running / queues / activity / steps / started / lastOutput / deadlineWarned / recentTokens / passTokens。
- `noteTokens(id, count, now)`（:96）：未登记或 count ≤ 0 忽略；追加 `(now, count)`，删掉早于 3 秒的；`passTokens += count`。
- `endPass(id)`（:106）：`passTokens = 0`（这一段的估算交给引擎的精确统计）。
- `speed(id, now)` = `windowSpeed(recentTokens[id])`；`windowSpeed(samples, now)`（:115-120）：取 `now - time <= 3` 的；少于 2 个为 0；
  `span = now - 第一个.time`；`speed = (除第一个外的 tokens 之和) / span`（span ≤ 0 为 0）。
- `setActivity(id, new)`（:123）：未登记忽略；`lastOutput = now`（换状态也算一次活动）；变了才写 `activity`。
- `noteOutput(id)`（:130）：`lastOutput = now`（每个文字块都调，不触发界面刷新）。`noteStep(id)`（:134）：`steps += 1`。
- `backgroundChildren(of key, in conv)`（:139）：`runs` 里 `parentKey == key && background && conversationID == conv` 的 id。
- `post(notice, to key)`（:143）：追加到 `notices[key]` 并 `wake(key)`。`takeNotices(key)`（:148）：取走全部（先进先出）。
- `waitForNotice(key, timeout?)`（:153-172）：已取消或收件箱非空 → 立即返回；否则登记 waiter，等 `wake(key)` / 超时 / 任务取消，**不抛**。
- `wake(key)`（:175）：叫醒等这个 key 的全部 waiter（有新通知、用户发了消息、要停止）。
- `reset(conversationID)`（:185-193）：这段对话的 run 全部 `task.cancel()` + `unregister`；叫醒**所有** waiter；`notices.removeAll()`（全部 key）；`launched[conv] = nil`。主循环 `finish()` 调（`AgentRunner.swift:1471`）。

#### 1.3.2 启动子 Agent（`launchSubAgent`，`AgentTeamControl.swift:218-264`）

每个子 Agent 在**自己的 Task** 里跑 `runSubAgent`（可单独取消）；`team.register(eventID, Run(name, parentKey, background, conv, assignment: task, task, deadline))`；
`team.launched[conv] += 1`；`startWatchdog()`。三种派法：

1. **delegate_task（等它做完）**（`SubAgentRunner.swift:172-201`）：
   `updateEventEverywhere(event.id, sink) { agentName = profile.name; transcript = [] }`；`key = parentKey(of: sink)`；
   `launched = launchSubAgent(…, deadline: childDeadline(seconds: args["deadline_seconds"], parentKey: key))`；
   `defer { team.unregister(event.id); team.notices[key]?.removeAll { $0.agentID == event.id } }`；
   `output = try await waitingWithoutSlot(sink) { try await launched.value（父任务被取消时 launched.cancel()） }`；返回 `withReports(output, of: event.id, parentKey: key)`；
   catch → `throw endedError(error, of: event.id)`（被单独停掉 / 失败：带着做到一半的结果交回）。
2. **run_parallel_tasks（等全部做完）**（`SubAgentRunner.swift:239-332`）：先把每项解析成 `TaskSpec` + `AgentProfile` + 子卡片
   （`name = delegate_task`、`arguments = JSONValue.object(item).jsonString()`、`status = .running`、`title = "子 Agent"`、`symbol = "person.2.badge.gearshape"`、`agentName`、`transcript = []`），
   父卡片 `agentName = (background ? "后台 ×" : "并行 ×") + "{N}"`、`transcript = 子卡片`；一个 `deadline` 给全部子 Agent；逐个 `launchSubAgent`。
   每个子任务的包装（:276-303）：`defer { unregister(child.id); notices[key] 去掉它的 }`；
   成功 → `output = withReports(run.value, …)`，**一完成就**写子卡片 `status = .succeeded, result = output.text, media = output.media 空则 nil`；
   失败 → `ended = endedError(error, of: child.id) as? SubAgentEnded`；
   `stopped = ended?.stoppedBy != nil || (ended == nil && (error is CancellationError || isStopping || Task.isCancelled))`；
   子卡片 `status = .failed, result = ended?.cardText ?? (stopped ? "已停止" : error.localizedDescription), stopped = stopped ? true : nil, queued = nil`；
   不是 stopped 才记日志「子 Agent「{name}」失败」（detail = error.localizedDescription）；结果 `.failure(ended ?? (stopped ? CancellationError() : error))`。
   等全部（`waitingWithoutSlot(sink) { 逐个 await }`，父任务取消时 cancel 全部），按任务顺序拼：
   - 成功：`### 任务 {i}（{name}）：{task.prefix(60)}\n{output.modelText}`（媒体并入）
   - `SubAgentEnded`：`### 任务 {i}（{name}）：{cardText}\n{advice(cardText)}`
   - `CancellationError`：`### 任务 {i}（{name}）：被停止了，没做完（不要重新派发同样的任务）`
   - 其他：`### 任务 {i}（{name}）：失败\n{error.localizedDescription}`
   段之间 `"\n\n"`。（Swift 在整轮停止时父卡片仍标成功；Qt 现有的「最后 `CancelToken::checkCurrent()`、父卡片按停止」保留，见 §2.8。）
3. **后台（`background: true`，delegate 和 parallel 都支持）**：立刻返回 `ToolOutput(backgroundStarted(...), runsInBackground: true)`（`SubAgentRunner.swift:182-184, 270-272`），
   dispatch 因此把卡片留在 `.running`（`AgentRunner.swift:1126-1127`）。子 Agent 结束时（`AgentTeamControl.swift:231-261`）：
   `stoppedBy = runs[id]?.stoppedBy`；`ended = 失败时 endedError(error) as? SubAgentEnded`（**注销前**取，要用半成品）；`unregister(id)`；
   - 成功：`updateAgentEvent(id, parentKey) { status = .succeeded; result = output.text; media = … }`；`Notice(.finished, text: output.modelText)`；
   - 失败：`stopped = stoppedBy != nil || error is CancellationError`；卡片 `status = .failed, result = ended?.cardText ?? (stopped ? "已停止" : error.localizedDescription), stopped = stopped ? true : nil, queued = nil`；
     通知文字 `stopped ? (ended?.cardText ?? "被{stoppedBy ?? "用户"}停止了，没做完。") : (ended.map { message + (partial.map { "\n\n" + $0 } ?? "") } ?? error.localizedDescription)`，kind = stopped ? `.stopped` : `.failed`；
   - `if !isStopping { team.post(notice, to: parentKey) }`（整轮在收尾就不通知）；然后 `finishBackgroundParentIfDone`。

`flag(value)`（:340）：`.bool(b)` → b；`.string(s)` → `s.lowercased() == "true"`；其余 false。
数字参数（`deadline_seconds`、`seconds`）按 `stringValue.flatMap(Double.init)` 读（JSON 数字或数字字符串都行）。

`backgroundStarted(agents)`（:348-352）：
```
已在后台启动：
- 子 Agent「{name}」，id：{shortID}
…（每个一行）
它们中途的汇报和最终结果会在你做完当前这一步时自动送到你这里，你可以先做别的。需要时用 message_agent 给它追加指示、stop_agent 停掉它、wait_agents 等它们的消息；这一轮结束前会等它们全部完成。
```
（第一行后是 `"\n"` 连接的条目，最后两句直接接在最后一个条目后面的 `"\n"` 之后，中间没有换行。）

`finishBackgroundParentIfDone(childID, parentKey, conv)`（:267-280）：在这一轮的消息里（`currentTurnMessages`，递归）找 `name == run_parallel_tasks` 且运行记录里含这个子卡片的父卡片；
其中所有子卡片都不是 `.running` 时：`updateAgentEvent(父卡片, parentKey) { status = .succeeded; result = "后台子 Agent 全部结束" + (有 stopped == true 的 ? "（有的被停止了）" : "") }`。

`updateAgentEvent(id, parentKey, body)`（:283-290）：父级是子 Agent（`subRuns[parentKey]` 在）且它本地块里找得到 → 改它本地副本并 `publish()`；否则 `updateEvent(id)`（存档）。

#### 1.3.3 新工具（全部 `permission: .readOnly`、`origin: .agents`；schema 由 `ToolDescriptor.schema` 生成，`required` 按键名排序）

`AgentProfiles.swift:31-35`：`message_agent`、`stop_agent`、`wait_agents`、`report_to_parent`（只给子 Agent）。`descriptors()` 返回顺序：
`[delegate_task, run_parallel_tasks, read_agent_log, message_agent, stop_agent, wait_agents, report_to_parent]`（:146）。

| 工具 | 标题 / 图标 | description（逐字） | 参数 |
|---|---|---|---|
| message_agent | 给子 Agent 追加指示 / text.bubble | 给一个还在运行的子 Agent 追加指示（比如从别的子 Agent 那里拿到了新信息、要求变了），它做完当前这一步就会看到，接着做。id 是派发时返回的那串（前 8 位即可）。 | `id` string「子 Agent 的 id」必填；`message` string「追加的指示」必填 |
| stop_agent | 停止子 Agent / stop.circle | 停掉一个还在运行的子 Agent（方向错了、已经不需要了），其他子 Agent 照常。它的结束通知会送到你这里。 | `id` string「子 Agent 的 id」必填 |
| wait_agents | 等子 Agent / hourglass | 手头没有别的事、只想等后台子 Agent 时用：等到它们有中途汇报或结果（默认最多 120 秒），返回收到的内容。不用它也行：汇报和结果会在你做完当前一步时自动送到。 | `seconds` number「最多等几秒，默认 120」（required 为空数组） |
| report_to_parent | 向父级汇报 / arrow.up.message | 做到一半有了阶段性结果、或者发现会影响全局的问题时，先交给派你的父级（它做完当前一步就会看到），然后你接着做。最终结果照常写在最后的回答里，不要用它交。 | `message` string「要先交给父级的内容」必填 |

delegate_task 参数加两项（`AgentProfiles.swift:78-79`）：
- `background` boolean：「true = 后台运行：立刻返回它的 id，你接着做别的；它中途的汇报和最终结果会自动送到你这里。不填 = 等它做完再返回」
- `deadline_seconds` number：「可选，最多给它多少秒：到点先提醒它交付已有结果，再过 60 秒还没交就停掉、交回做到一半的结果。不会超过你自己剩下的时间。不填就不限时」

run_parallel_tasks 顶层 properties 加两项（:105-116，不在 items 里）：
- `background` boolean：「true = 全部在后台运行：立刻返回各自的 id，你接着做别的；它们中途的汇报和最终结果会陆续送到你这里。不填 = 等全部做完再返回」
- `deadline_seconds` number：「可选，每个子 Agent 最多多少秒：到点先提醒交付，再过 60 秒还没交就停掉、交回做到一半的结果。不会超过你自己剩下的时间。不填就不限时」

执行（`SubAgentRunner.swift:203-234, 355-376`）：
- **message_agent**：缺 `id` / `message` 或 message 去空白后为空 → `invalid("需要 id 和 message")`；`runningChild(id, of: parentKey(sink), in: conv)` 找不到 →
  `invalid("没有 id 以「{id}」开头、由你派出、还在运行的子 Agent（它可能已经结束了）。")`；`sendToAgent(child, text: message, from: parentName(sink))`；
  返回「已排队：子 Agent「{name}」做完当前这一步就会看到。」
- **stop_agent**：找不到 → `invalid("没有这个 id、由你派出、还在运行的子 Agent（它可能已经结束了）。")`；`stopAgent(child, by: parentName(sink))`；返回「已停止子 Agent「{name}」。」
- **wait_agents**：`waitingWithoutSlot(sink) { waitForChildren(sink, seconds) }`。`limit = min(max(seconds ?? 120, 1), 600)`，`deadline = now + limit`；循环（任务没取消时）：
  1. `takeNotices(key)` 非空 → 返回 `notices.map(noticeText).joined("\n\n")`；
  2. 没有后台子 Agent → 返回「没有在后台运行的子 Agent，也没有新的汇报或结果。」；
  3. 到点 → 返回
     ```
     等了 {Int(limit)} 秒，还没有新的汇报或结果。还在跑的：
     - 「{name}」（id：{shortID}）：{agentStatusText(id) ?? "在运行"}
     …
     继续等就再调 wait_agents；觉得它走偏了或太慢，用 message_agent 让它先交已有结果，或 stop_agent 停掉（会交回做到一半的结果）。
     ```
  4. 否则 `waitForNotice(key, timeout: 剩余秒数)` 再循环。取消时返回「已停止等待。」
- **report_to_parent**：sink 不是 SubRun 或查不到它的 parentKey → `invalid("只有子 Agent 能用 report_to_parent")`；message 缺 / 空 → `invalid("需要 message")`；
  `team.post(Notice(agentID: run.eventID, agentName: profile.name, kind: .report, text: message), to: parent)`；
  返回「已交给父级（它做完当前这一步就会看到）。接着做你的任务，最终结果照常写在最后的回答里。」
- 预检（`ToolRegistry.swift:527-528`，放在「没有名为 … 的工具」之后、任务状态检查之前）：`report_to_parent` 且 `(sink?.depth ?? 0) == 0` →
  「report_to_parent 只给子 Agent 用；你是主 Agent，直接回答用户即可。」
- 主 Agent 的工具列表 `mainAgentSpecs` = activeDescriptors 去掉 `report_to_parent`（`ToolRegistry.swift:77`）；子 Agent 用 `subAgentSpecs`（含它）。

辅助（`AgentTeamControl.swift:198-213, 396-417`）：
- `agentMessageTool = "agent_message"`（子 Agent 收到消息时在它运行记录里留的卡片名；不是可调用的工具）。
- `agentMessageNote(from)`：from 非空 →「（{from}在你工作时追加了下面的指示：和你的任务一起考虑，有冲突以它为准。）\n」；nil →「（用户在你工作时补充了下面这段话：和你的任务一起考虑，有冲突以它为准。）\n」
- `parentKey(of sink)` = SubRun 的 eventID，主 Agent 为 mainKey。`parentName(of sink)` = 「父 Agent「{profile.name}」」或「主 Agent」。
- `sendToAgent(id, text, attachments, from)`：text 去空白；`running` 里没有它、或文字和附件都空 → false；否则 `queues[id].append(QueuedInput(text, attachments, from))` → true。
- `removeAgentQueued(itemID, agent)`：删掉；队列空了置 nil。
- `runningChild(prefix, of key, in conv)`：prefix 去空白、小写，**少于 4 个字符 → nil**；在 `runs` 里找 `parentKey == key && conversationID == conv && uuidString.lowercased().hasPrefix(prefix)` 的（只认直接下级）。
- `stopAgent(id, by who = "用户")`（:313-319）：未登记或已有 stoppedBy → false；否则记 `stoppedBy = who`、`task.cancel()` → true。

#### 1.3.4 交给子 Agent 的消息与通知文字

`takeAgentInterjection(for run, supportsVision)`（`AgentTeamControl.swift:423-439`，子 Agent 引擎的 interjection 钩子）：`isStopping` 或这个 run 已 stoppedBy → `[]`。
否则先取走 `queues[eventID]` 的每一条：运行记录里追加卡片 `ToolEvent(name: "agent_message", arguments: item.text, status: .succeeded, result: "已交给子 Agent")`，
`title = item.from.map { "{from}追加的指示" } ?? "用户补充"`，`symbol = "text.bubble"`（`run.append` 会先把正在写的文字定型）；
出站消息 `agentMessage(agentMessageNote(item.from) + item.text, attachments, supportsVision)`；再取走 `notices[eventID]`，每条 `.user(noticeText(notice))`。
`agentMessage`（:441-447）：无附件 → `.user(text)`；有 → `.user(text + "\n\n[附带的文件]\n" + 每个 "- {lastPathComponent}：{path}" 换行连接, images: supportsVision ? 其中的图片 : [])`。

`noticeText(notice)`（:450-458），`who = "子 Agent「{name}」（id：{shortID(agentID)}）"`：
- report：`[{who}中途汇报，它还在继续]\n{text}`
- finished：`[{who}完成了]\n{text}`
- failed：`[{who}失败了]\n{text}`
- stopped：`stoppedNotice`（:461-465）：text 按第一个 `"\n"` 切两半（`omittingEmptySubsequences: false`），后半去首尾空白 →
  `"[{who}{第一行}]" + (后半空 ? "" : "\n" + 后半) + "\n" + SubAgentEnded.advice(forCardText: text)`
- `noticeTitle(text)`（:467-472，界面用第一行判断）：含「中途汇报」→「中途汇报」；含「完成了]」→「完成」；含「失败了]」→「失败」；否则「已停止」。
- 主 Agent 那里汇报 / 结果作为**用户消息**进上下文，`ChatMessage.fromAgent = 子 Agent 名字`（界面画成灰色汇报卡片；主循环的 `takeInterjection` 负责，`AgentRunner.swift:690-700`）。

#### 1.3.5 子 Agent 的收尾循环与孤儿（`SubAgentRunner.swift:494-553`）

- 建好引擎后 `team.subRuns[eventID] = run`；`defer` 里把它派到后台、还没结束的下级全部 `stopAgent(child, by: "上级「{profile.name}」")`（不留没人收消息的孤儿）。
- `engine.interjection = { takeAgentInterjection(for: run, supportsVision) }`。
- 第一次 `stream(prompt: brief)` 结束后循环（任务没取消时）：`hasPending = notices[eventID] 非空 || queues[eventID] 非空`；
  没有 pending：没有后台下级 → 退出循环；有 → `waitingWithoutSlot(run) { waitForNotice(eventID) }` 后 continue；
  有 pending：`messages = takeAgentInterjection(run)`，空则 continue；`run.finishPass()`；`stream(messages:)` 接着做。
  之后 `if Task.isCancelled { throw CancellationError() }`。

#### 1.3.6 运行记录合并（`SubRun.merge`，`SubAgentRunner.swift:103-122`）

「谁写的听谁的」：遍历本地块；不是工具块、或存档里（同一层）没有同 id 的 → 原样；否则：
- 本地是 `run_parallel_tasks` 且本地有运行记录 → 运行记录 = `merge(本地子卡片, 存档子卡片)`（递归：子卡片的**状态以本地为准**，各子卡片的**运行记录取存档**）；
- 否则存档有运行记录且条数 ≥ 本地 → 用存档的；
- `queued = saved.queued`（排队标记由下级自己和调度器写进存档）；`usage = saved.usage ?? event.usage`（用量由子 Agent 自己写）。

#### 1.3.7 停止

- 卡片上的停止键 / `stop_agent` / 看门狗 / 期限 → `stopAgent(id, by:)`：只停这一个（连同它正在跑的命令、它的同步下级——取消传播；它的后台下级由它退出时的 defer 停掉），其他照常。
- 整轮停止（主循环 `stop()` → `cancelTeam()`，`AgentRunner.swift:935-939`）：这段对话所有 run `task.cancel()`（不记 stoppedBy），`wake(mainKey)`。

#### 1.3.8 调度修死锁：本地（`SubAgentScheduler.swift`，`AgentTeamControl.swift:321-347`）

Swift v1.7 的 `SubAgentScheduler` 只管本地，在线的 `acquireOnline/releaseOnline` 删除：
- 常量：`headroom = 1500 << 20`、`expectedTokens = 12_000`、`activationOverhead = 600 << 20`、**`resumeBoost = 10_000`**（:13-19）；`kvBytesPerToken` 初值 40 KB；`localBudget = kvBytesPerToken × 12000 + 600 MB`。
- `canStartLocal()`（:36-43）：`runningLocal == 0` → true（**没有正在跑的就一定放行**）；DEBUG 下 `FRIDAY_LOCAL_SLOTS=<n>` 且 `runningLocal >= n` → false；否则 `availableMemory >= localBudget + headroom`。
- `acquireLocal(priority)`（:50-67）：`waiters 空 && canStartLocal()` → `runningLocal += 1`、返回 false；否则排队（`sequence += 1` 当 id），排队时被取消抛 CancellationError（从队列撤下），轮到返回 true。
- `releaseLocal()`：`runningLocal = max(0, -1)`；`pump()`。
- `pump()`（:80-88）：`while waiters 非空 && canStartLocal()`：取 **priority 最大、同 priority 中 id 最小**（先来）的放行，`runningLocal += 1`。
- 优先级：新开跑的子 Agent = **它的层数**（深的先走，先把已经开始的子树做完）；等完下级 / 确认要接着跑的 = **`resumeBoost + 层数`**（KV 已经在内存里，最先）。
- `diagnostics`（:90-94）：「本地子 Agent 运行中 {running} 个，排队 {waiting} 个；当前可用内存 {free}，每个本地子 Agent 预估占用 {budget}（按 {kvBytesPerToken/1024} KB/词元 × 12000 词元 + 激活值）」（在线那段删掉）。

`runSubAgent` 里（`SubAgentRunner.swift:467-485`）只有本地才排：卡片 `queued = true`、`activity = .queued` → `acquireLocal(priority: depth)`（被取消：`queued = false` 后抛）→
`holdsLocalSlot = true`、`activity = .generating`、`queued = false`；排过队记日志「子 Agent「{name}」排队后开始运行」（detail = diagnostics）；`defer { if holdsLocalSlot { releaseLocal() } }`。

`waitingWithoutSlot(sink, as waiting = .waitingChildren, body)`（`AgentTeamControl.swift:324-347`）——等下级、等用户确认时让出本地名额：
1. sink 是 SubRun → `setActivity(waiting)`；defer 结束时 `setActivity(.generating)`。
2. 不是 SubRun 或它没占本地名额 → 直接 `body()`。
3. 否则 `holdsLocalSlot = false`、`releaseLocal()`；执行 body（记下结果或错误）；
4. 没被取消且不在整轮停止 → `activity = .queued`、卡片 `queued = true`（defer 回 false）→ `acquireLocal(priority: resumeBoost + depth)` → `holdsLocalSlot = true`；
   重新排队时出错：body 成功则抛这个错，body 失败则忽略（下一步抛 body 的错）。
5. 返回 / 抛出 body 的结果。

用到它的地方：delegate 等子 Agent、parallel 等全部、wait_agents、子 Agent 收尾循环等通知、**dispatch 里等用户确认**（`AgentRunner.swift:1083-1086`：`as: .waitingApproval`，`(try? …) ?? false`）。
在线子 Agent 不占名额（`holdsLocalSlot` 永远 false），只在请求途中占闸门名额（§1.3.9）。

#### 1.3.9 在线：并发闸门 `RequestGate`（`LLM/Engine/RequestGate.swift`，全文 132 行）

按引擎 id 共用（`gate(for: engineID, initial: config.maxConcurrency)`，:20-27；第一次取时建，**之后 initial 不再起作用**）；主 Agent 与所有子 Agent 的请求都过它。
- 常量：`mainPriority = 1_000`（主 Agent 的排队优先级；子 Agent 用层数），`defaultInitial = 4`；`limit: Double = max(1, initial ?? 4)`；`allowed = max(1, Int(limit))`（截断）。
- `acquire(priority, onQueue)`（:50-73）：`waiters 空 && inFlight < allowed` → `inFlight += 1` 返回；否则 `onQueue(true)`、入队（id 递增）、`pump()`、挂起；被取消 → 从队列撤下、`onQueue(false)`、抛 CancellationError；放行 → `onQueue(false)`。
- `release(outcome)`（:75-100）：`inFlight = max(0, inFlight - 1)`，然后
  - `.rateLimited`：距上次减半超过 2 秒（或从没减过）→ `limit = max(1, min(limit, Double(inFlight + 1)) / 2)`、记 `lastCut = now`，日志「{engineID} 被限流：并发 {before} → {allowed}，在途 {inFlight}，排队 {waiters}」；
  - `.ok`：有请求在排队 且 距上次减半超过 10 秒（或从没减过）→ `limit += 1 / limit`（加性增长，大约每轮 +1），`allowed` 变了记「{engineID} 没被限流、有请求在排队：并发 {before} → {allowed}」；
  - `.failed`：不变。
  - 最后 `pump()`。
- `pump()`（:112-121）：`while inFlight < allowed && waiters 非空`：放行 priority 最大、同级 id 最小的，`inFlight += 1`。
- `snapshot()` = `(allowed, inFlight, waiters.count)`（诊断 / 自检）。日志类别 subsystem `com.example.Friday`、category `ratelimit`。
- `isRetryableRateLimit(body)`（:125-131）：小写后含任一 `["insufficient_quota", "exceeded_current_quota", "余额不足", "欠费", "套餐", "无权", "权限", "permission"]` → false；
  匹配正则 `"code"\s*:\s*"?(1113|1304|1308|1309|1310|1311)"?` → false；否则 true（额度用完 / 没权限的 429 重试没用，直接报错让模型换引擎）。

`OpenAIChatEngine` 的请求循环（`OpenAIChatEngine.swift:294-355`），每段请求：
```
throttled = 0；rateLimits = 0；busyRetries = 0
loop:
  gate.acquire(priority: requestPriority, onQueue: onQueued)
  发请求；网络层出错 → release(.failed)，抛
  2xx → 读完整个流：成功 release(.ok) 返回；读的过程中出错（含取消）release(.failed) 抛
  非 2xx：正文按行读，超过 4000 字停
  status == 429 且 isRetryableRateLimit(正文)：
      release(.rateLimited)；rateLimits += 1
      wait = Retry-After 能按数字解析 ? 那么多秒 : (min(30, 2^min(rateLimits, 5)) 秒 + 随机 0..<1000 毫秒)
      throttled += wait；throttled > 600 秒 → 抛 CapabilityError.http(429, "持续被限流超过 10 分钟，放弃这次请求。" + 正文)
      sleep(wait)；continue（重新排队）
  release(.failed)
  status ∈ {502, 503, 529} 且 busyRetries < 3：busyRetries += 1；sleep(Retry-After ?? 2^busyRetries 秒)；continue
  抛 CapabilityError.http(status, 正文)        （额度 / 权限类 429 走这里：直接报错）
```
（429 退避序列 2 / 4 / 8 / 16 / 30 / 30… 秒 + 抖动；服务繁忙 2 / 4 / 8 秒；两个计数器分开。）
引擎属性（:37-39）：`requestPriority = RequestGate.mainPriority`；`onQueued: (@Sendable (Bool) async -> Void)?`。子 Agent（`SubAgentRunner.swift:516-522`）：
`requestPriority = depth`；`onQueued = { waiting in updateEvent(eventID) { queued = waiting }; team.setActivity(eventID, waiting ? .queued : .generating) }`。

#### 1.3.10 提示词与说明的变化（v1.7 部分）

- 在线引擎在 engine 可选项里的说明（`AgentProfiles.swift:69`）：「{id}（{title} · {model}：在线，快、并发不设上限（按限流自动调整），按量计费{能看图 ? "，能看图" : ""}）」（原来是「可并发 {n} 个」）。
- `parallelDescription`（:72）改成「…同时跑几个由系统决定（本地看剩余内存，在线按接口限流自动调整），其余自动排队。…」，全文见 §1.4.6。
- 主 Agent `guidance` 加「### 后台子 Agent」一段；子 Agent 前言加两条（report_to_parent、追加的指示）。全文见 §1.4.6。
- `EngineConfig.maxConcurrency` 的含义改成「起步并发：在线请求从这么多个同时在途开始，之后不设上限、按 429 自适应（RequestGate）；nil 按 4」（`EngineStore.swift:51-56`）；
  诊断里每个引擎行的尾巴改成「，起步并发 {concurrency}（不设上限，按限流自动调整）」（`EngineStore.swift:141`）。

### 1.4 v1.8（f1f6d58）层数不限、活性、用量、Agent 树

#### 1.4.1 层数不限：拆小规则、每轮上限、上级链

- 删 `maxDepth`；`deepLevel = 4`（到这一层起提示「除非明显还能拆，否则自己做」）；`shrinkThreshold = 0.8`（`AgentProfiles.swift:37-40`）。
- `runTeamTool` 一开始（`SubAgentRunner.swift:166-170`）：`depth = sink.depth + 1`；工具是 delegate 或 parallel 时先 `checkDispatch`（**在参数校验之前**）。不再有「已经是第 N 层…不能再派发」。
- `subAgentSpecs`（:563-570）：activeDescriptors 去掉 `update_task_state`；有 `profile.tools` 时只留其中的；**不再按深度去掉派发类工具**。
- `similarity(a, b)`（`AgentProfiles.swift:43-59`）：各自 `lowercased()` 后按 Unicode 标量过滤掉 `whitespacesAndNewlines`、`punctuationCharacters`、`symbols`；
  标量数 ≤ 1：空 → `[:]`，一个 → `[那个字: 1]`；否则统计相邻两标量组成的 bigram 次数；`total = 两边次数和`，为 0 → 0；`shared = Σ min(x[k], y[k])`；返回 `2 × shared / total`（Dice 系数）。
- `checkDispatch(tool, args, sink)`（`AgentTeamControl.swift:350-372`）：
  - `tasks`：parallel 取 `args.tasks` 里每项（`asObject`）的 `task.stringValue`（compactMap）；delegate 取 `[args.task.stringValue]`（compactMap，0 或 1 个；不去空白）。
  - `limit = settings.subAgentTurnLimit`；`launched = team.launched[conv] ?? 0`；`limit > 0 && launched + tasks.count > limit` → `left = max(0, limit - launched)`，抛 `AgentToolError.failed`：
    left > 0：「这一轮最多派 {limit} 个子 Agent（设置 › 子 Agent），还能派 {left} 个，这次要派 {tasks.count} 个。少派几个（把相近的合并），其余自己做。」
    left == 0：「这一轮已经派满 {limit} 个子 Agent（设置 › 子 Agent），不能再派了：用已经拿到的结果收尾，没做完的自己做或在结果里说明。」
  - `tasks.count == 1` 且 sink 是 SubRun 且查得到它的 `assignment`：`score = similarity(tasks[0], own)`；`score >= 0.8` → 抛 `AgentToolError.invalid`：
    「交给下级的任务和你自己的任务几乎一样（相似度 {Int(score * 100)}%），这是把整个任务原样往下传。自己做；或者先想清楚能拆成哪几个更小的部分，再分别派出去。」
  - 被拒的派发没有运行记录（`transcript` 没写），**不进 Agent 树**。
- `ancestryNote(eventID, conv)`（`AgentTeamControl.swift:375-394`，第 2 层起附在任务后面）：`clip(text, n)` = 换行换成空格、去首尾空白、超过 n 字取前 n 字 + 「…」。
  从它的 parentKey 往上走到 mainKey：每个祖先插到链首一行「{giver}派给「{祖先名}」：{clip(祖先.assignment, 120)}」，giver = 祖先的父级在 runs 里 ? 「「{父名}」」: 「主 Agent」。
  链为空（第 1 层）→ nil。`request` = 对话里**最后一条 `role == .user && fromAgent == nil`** 的 `plainText`。结果：
  ```
  [你在整个任务里的位置（Friday 自动附上，只帮你理解背景；你只做上面交给你的那部分）]
  用户的请求：{clip(request, 200)}        ← request 为 nil 时整行不要
  {链，每行一个}
  再往下就是你「{me}」（第 {chain.count + 1} 层）。
  ```
  （me = 自己的 run.name，查不到为「你」。）
- 子 Agent 的任务正文 `brief = [task, TaskState.subAgentNote(currentPath), ancestryNote].compactMap.joined("\n\n")`（`SubAgentRunner.swift:532-533`）。
- 日志「派发子 Agent「{name}」（第 {depth} 层）」detail = task（:529）。

#### 1.4.2 活性：状态、看门狗、期限、原地打转（`Agent/AgentLiveness.swift`）

常量（:43-58）：`stallLimit = 300` 秒（DEBUG 下 `FRIDAY_STALL_LIMIT=<秒>` 覆盖，每次读）；`deadlineGrace = 60`；`repeatWarn = 3`；`repeatBlock = 5`；`stepNudge = 40`；
`pollingTools = [wait_agents, check_job, get_current_time]`（重复调用不算打转）。

`duration(seconds)`（:86-91）：`total = max(0, Int(seconds))`；< 60 →「{total} 秒」；< 3600 →「{total/60} 分 {total%60} 秒」；否则「{total/3600} 小时 {total%3600/60} 分」。

`agentStatusText(id, now, compact)`（:64-84），没有 activity → nil；用「 · 」连接：
1. 在做什么：generating「在思考 / 写」、tool(t)「在执行「{t}」」、waitingChildren「在等它的下级」、waitingApproval「在等用户确认」、queued「在排队」；
2. 非 compact：「{duration(now - lastOutput)}前有动静」；
3. 「第 {steps ?? 0} 步」；
4. 有 started：compact 时 `duration(now - started)`，否则「已运行 {duration}」；
5. 非 compact 且有 deadline：剩余 > 0 →「期限还剩 {duration(剩余)}」，否则「已过期限」。compact 版到第 4 项为止。

dispatch 里（`AgentRunner.swift:1104-1110`，审批通过之后、执行之前）：sink 是 SubRun → `setActivity(.tool(event.title ?? call.name))`、`noteStep`；defer 回 `.generating`。
所以 `team.steps` 只数真正执行的调用；`SubRun.steps`（`consumeStep` 计数，含被退回的）另算，用在步数上限和步数提醒。

看门狗（`startWatchdog`，:96-106）：有子 Agent 在跑就每 5 秒巡查一次（第一次在 5 秒后），`running` 空了就停。`watchdogTick`（:108-127）对每个 running 且没被停的：
- 有 deadline 且 `now >= deadline`：
  - `now >= deadline + deadlineGrace` → 日志「子 Agent「{name}」过了期限还没交付，已停掉」（detail = agentStatusText）；`stopAgent(id, by: "期限（到点后又等了 {Int(deadlineGrace)} 秒）")`；continue；
  - 没提醒过 → 记下，`sendToAgent(id, text: "派发时给的时间到了：立刻停下手上的事，交付已有结果，没做完的写进「未完成」。{Int(deadlineGrace)} 秒内不交就会被停掉。", from: "Friday")`。
  （两处秒数是 `deadlineGrace` 插值，默认 60；工具说明和 guidance 里的「再过 60 秒」是写死的文字。提醒走子 Agent 的消息队列，要等它做完当前一步才看得到。）
- `activity == .generating` 且 `now - lastOutput > stallLimit` → 日志「子 Agent「{name}」{duration(stallLimit)}没有任何输出，看门狗停掉了它」；
  `stopAgent(id, by: "看门狗（{duration(stallLimit)}没有任何输出）")`（默认就是「看门狗（5 分 0 秒没有任何输出）」）。
  只有「生成中」长时间没输出才算卡住：工具有自己的超时，等下级、等确认、排队都不算。

`childDeadline(seconds, parentKey)`（:130-139）：`own = seconds > 0 ? now + seconds : nil`；`inherited = runs[parentKey]?.deadline`；两个都有取早的，否则有哪个用哪个，都没有 nil（下级的期限不超过上级剩下的时间）。

原地打转（`noteRepeat`，:144-155；dispatch `AgentRunner.swift:1043-1053, 1147-1153`）：轮询类工具返回 0；签名 = `name + "\u{1}" + prettyArguments`；
和上一次相同则计数 +1，否则重置为 1（子 Agent 记在 `SubRun.lastCall`，主 Agent 记在 `mainLastCall`，每轮开始时清空 `AgentRunner.swift:619`）。在 dispatch 里**预检通过之后、审批之前**：
- `repeats >= 5`：追加失败卡片 `result = problem`（title / symbol 照常），日志「拦下重复调用 {name}（连续 {repeats} 次）」detail = arguments，返回「调用被拒绝（未执行）：{problem}」，
  problem =「同样的调用已经连续 {repeats} 次了，结果不会变，这次没有执行。换个方法（换参数、换工具、先读错误信息），或者交付已有结果，把没做完的写进「未完成」。」
- 执行后回灌文字（toolResultText 之后）追加：`repeats == 3` →「\n（同样的调用已经连续 3 次了。结果不会因为重复而变：换个方法，或者交付已有结果；再重复两次会被拦下。）」；
  sink 是 SubRun 且 `sub.steps > 0 && sub.steps % 40 == 0` →「\n（你已经调用了 {sub.steps} 次工具。检查一下是不是在绕圈子：能交付就先交付，没做完的写进「未完成」。）」

#### 1.4.3 半成品与建议（`AgentLiveness.swift:12-40, 157-216`）

`SubAgentEnded: LocalizedError`（`stoppedBy: String?`（nil = 失败）、`message`、`partial: String?`），`errorDescription = cardText`：
`cardText = (stoppedBy.map { "被{$0}停止了，没做完。" } ?? "失败了：{message}") + (partial.map { "\n\n" + $0 } ?? "")`。

`advice(forCardText:)`（按开头判断）：
- 「失败了」开头 →「判断失败原因：能修就换个做法（拆小一点、换个大脑）再派，不行就自己做或告诉用户。」
- 「被看门狗」开头 →「它卡住了：上面做到一半的结果可以直接用；剩下的换个做法（拆小一点、换个大脑）再派，或者自己做。」
- 「被期限」开头 →「时间到了：用上面做到一半的结果收尾，没做完的在回答里说明。」
- 其余（用户、父级、上级停的）→「不要重新派发同样的任务；按已有信息继续（上面做到一半的结果可以直接用），或告诉用户需要什么。」

`endedError(error, of id)`（:209-216）：整轮在停止 → 原样；`runs[id].stoppedBy` 有 → `SubAgentEnded(stoppedBy, "", partialResult(id))`；
error 是 CancellationError 或已经是 SubAgentEnded → 原样；否则 `SubAgentEnded(nil, error.localizedDescription, partialResult(id))`。
（**所有**来自子 Agent 的错误都过它，包括「没有 id 为 X 的在线引擎」这类开跑前的错误——这时还没有 SubRun，partial 为 nil，卡片是「失败了：…」。）

`partialResult(of id)`（:169-206）：没有 `subRuns[id]` → nil。`clip(text, n)` = 去首尾空白，超过 n 字取「…」+ **最后** n 字。遍历它的块（递归进 parallel）：只看 `.succeeded` 的工具卡片：
delegate → 下级结果「「{agentName ?? "子 Agent"}」：{clip(result, 400)}」；parallel → 递归；`agent_message`、`report_to_parent`、`read_agent_log`、`wait_agents`、`message_agent`、`stop_agent` 跳过；
其余按 `title ?? name` 计数。拼：
```
做到一半的结果（运行记录 id：{shortID}）：
它停下前已经写出的内容：
{clip(finalText, 1500)}            ← finalText 空则无此段

它的下级已经交回的结果：
- 「…」：…                          ← 没有则无此段

它已经执行过的操作：{按名字升序，次数 > 1 写成「{名} ×{次}」，用「、」连接}（改动可能只做了一半，要核对）
```
（各段之间 `"\n\n"`；一段都没有 → nil。）

`withReports(output, of id, parentKey)`（:160-166，等它做完再返回的子 Agent）：取 `notices[parentKey]` 里这个子 Agent 的 `.report` 文字，并把它的**全部**通知删掉；
有汇报 → `text + "\n\n（它工作中途向你汇报过：\n" + 每条 "- {r}" 换行连接 + "）"`。

dispatch 的错误分支（`AgentRunner.swift:1129-1143`）：`SubAgentEnded` 且 stoppedBy 非空 → `result = cardText, status = .failed, stopped = true`；
`isStopping || Task.isCancelled || CancellationError || URLError.cancelled` → `result = "已停止", status = .failed, stopped = true`；其余失败照旧（失败型 SubAgentEnded 的 result 就是它的 cardText）。

#### 1.4.4 用量与速度（`ChatModels.swift:85-106`、`SubAgentRunner.swift:31-62, 74-76, 509-515`）

`AgentUsage: Codable`：`model`（在线引擎的 model，本地模型的显示名）、`service: String?`（在线服务名，本地 nil）、`thinking`（「关闭 / 低 / 中 / 高」，不支持时「不支持」）、
`temperature: Double`、`generatedTokens = 0`、`promptTokens = 0`（本地：新处理的提示词；在线：各段请求的输入合计）、`generateTime: Double = 0`（不含等首 token）。
`tokensPerSecond = generateTime > 0 ? generated / generateTime : 0`；`detail = ["深度思考：{thinking}", "温度：{temperature 0–2 位小数}", service.map { "服务：{$0}" }].compactMap.joined("\n")`。

- 开跑时 `run.startUsage(AgentUsage(model: config?.model ?? engine.displayName, service: config?.title, thinking: thinks ? level.title : "不支持", temperature: settings.temperature))`，
  `thinks = config.map { thinkingSupport != .none } ?? preset(loadedModelID ?? settings.modelID).supportsThinking`；立刻写进卡片（`updateEvent(eventID) { usage = … }`，存档那份）。
- 每个 chunk：`team.noteOutput(eventID)`；`team.noteTokens(eventID, estimateTokens(chunk, local: isLocal))`。
- 每段结束（`.done(stats)`）：`team.endPass(eventID)`；`usage.generatedTokens += stats.generatedTokens; promptTokens += stats.promptTokens; generateTime += stats.generateTime`；写进卡片。
- `estimateTokens(chunk, local)`（`AgentRunner.swift:1409-1412`）：`"<think>"`/`"</think>"` → 0；本地 → 1；在线 → `max(1, round(utf8 字节数 / 3.5))`。

#### 1.4.5 Agent 树数据（`Views/AgentTreePanel.swift` 的非界面部分）

- `AgentNode`（:8-19）：`event`、`depth`（主 Agent 直接派的是 1）、`parallel`（同一次并行派发出来的）、`children`；`name = agentName ?? "子 Agent"`；`isRunning = status == .running`。
- `build(blocks, depth = 1, parallel = false)`（:30-44）：delegate 且**有运行记录** → 节点（children = build(transcript, depth + 1)）；parallel → `build(transcript, depth, parallel: true)`（**分组不算一层**）；其余忽略。
- `turns(messages)`（:47-57）：`role == .user && interjected != true && fromAgent == nil` 开一轮（request = plainText）；助手消息的节点并进当前轮；去掉没有节点的轮，**新的在前**。
- `all`（前序展开）、`path(to id)`（根到它，找不到为空）。
- `stats`（:85-102）：每个节点 `deepest = max(depth)`；running / awaitingApproval → running；succeeded → done；failed 且 stopped → stopped；failed / denied → failed；`total = 四者和`。
- `UsageSummary`（:109-121）：generated、prompt、speed、agents、running，可相加。
- `usage(of node, now)`（:125-136）：`generated/prompt = usage ?? 0`、agents = 1；运行中：`generated += team.passTokens[id]`、`speed = team.speed(id)`、running = 1；否则 `speed = usage.tokensPerSecond`（平均）。
- `conversationUsage(messages, conv, now)`（:139-158）：主 Agent = 各助手消息 `stats.tokenCount` / `stats.promptTokenCount` 之和，agents = 1，`speed = mainLiveSpeed(conv)`，在生成这段对话时 running = 1；
  子 Agent = 所有节点 usage 之和，**非运行中的速度不计**（只把正在生成的加起来）。
- `compact(n)`（:161-167）：≥ 1,000,000 →「%.1fM」；≥ 1000 →「%.1fk」；否则整数。`speedText(speed)`（:170-173）：≤ 0 →「— tok/s」；≥ 100 →「%.0f tok/s」；否则「%.1f tok/s」。
- `flatten(nodes, level = 0, guides = [], collapsed)`（:504-514）：每个节点一行 `(node, level, guides, isLast)`；未收起的递归 `flatten(children, level + 1, guides + [!isLast])`。
- `NodeCard.conclusion(text)`（:682-685）：结果第一行去空白，「结论：」开头则去掉。
- 树面板自动打开（`AgentTreeButton`，:187-218）：最新一轮 `stats.deepest >= 3 && stats.running > 0` 第一次成立时打开一次（每个视图一次）；`team.treeFocus` 被设置时打开并选中该节点。
- 卡片 / 节点详情里的工具调用数 = 运行记录里工具卡片数（不含 `agent_message`）。

#### 1.4.6 提示词全文（`AgentProfiles.swift`，逐字）

`delegateDescription`（:71）：
```
派一个临时子 Agent 独立完成一个子任务，返回它的最终结果。何时派发由你自己判断，不需要用户要求：需要多步工具操作、需要独立视角审查、或者是串联中的一段时就派。交给它的必须是你任务里的一部分，不能把整个任务原样交出去。子 Agent 用完即销毁，看不到这段对话，task 必须自包含：背景、输入（文件路径等）、期望输出。{engineHint}
```
`engineHint`：没有可选大脑为空，否则「engine 可选（只能用这些）：」+ 各项用「；」连接 +「。不填跟随你自己的大脑；你的大脑不在上面时用第一个。」；本地项「local（本机模型：免费、离线，但每次冷启动要重新处理约 6–8k tokens 的提示，较慢）」；在线项见 §1.3.10。
v1.20：`run_parallel_tasks` 的说明末尾也接 `{engineHint}`（以前只有 delegate_task 有，并行派发时模型只能照搬历史里的引擎 id，用户不许子 Agent 用的也照填），每项的 engine 说明是「local 或在线引擎 id（可选的见工具说明）」。
派了不能用的大脑时报错里的「可用：」每项写成「{id}（{title} · {model}）」/「local（本机模型）」，被拒的那个写成「用户不允许子 Agent 使用 {id}（{title} · {model}）」——同一家的引擎标题相同，只给 id 或标题分不清哪个能用。

`parallelDescription`（:72）：
```
同时派发多个互不依赖的子任务给各自的临时子 Agent，等全部完成后一起返回。派几个由你按任务需要决定，不限数量；同时跑几个由系统决定（本地看剩余内存，在线按接口限流自动调整），其余自动排队。请求能拆成两个以上独立部分时优先用它，而不是逐个串行做。每个任务的 task 同样要自包含。
```
task / role / name / engine 的说明与 tasks 数组的 items 说明不变（Qt 现有文字正确）。

`splitGuidance`（:150-157，前后都没有换行）：
```
### 拆不拆、怎么拆
- 能分成两个以上互不依赖的部分（查几件事、处理几个文件、从几个角度分析）→ 并联：run_parallel_tasks 一次派出，需要几个派几个，多出来的会排队。并联的几个不能同时改同一批文件、在同一个构建目录里编译（会互相覆盖、锁住文件）：编译交给其中一个，或各用各的构建目录。
- 是先后几段、每段有明确产出、过程细节后面用不上（先调研再写、先改再测）→ 串联：用 delegate_task 一段一段派，上一段的结论写进下一段的 task。每段的过程留在它自己那里，你只拿结论。
- 串里可以并、并里可以串：子 Agent 自己也能按同样的标准再往下拆，层数不限，直到每一块都小到可以直接做。
- 一两步工具调用就能做完、几部分要大量共享上下文、或者写清楚任务比直接做还费事 → 自己做，不派发。
- 派下去的必须比你自己的任务小：只派一个下级时，不能把你的整个任务原样交给它（会被拒绝）。
```

`guidance`（:160-178，以一个空行开头、末尾没有换行；第 4 行是 splitGuidance 原样插入）：
```

## 多 Agent 协作
你是主 Agent。收到请求后先自己判断要不要派发子 Agent，不要等用户提出。
{splitGuidance}
- 结论重要、需要独立视角复核（方案、代码、事实）→ 派一个审稿角色检查后再回答。
子 Agent 都是临时的：每次派发时用 role 写清它的职责、方法和输出格式；它看不到这段对话，也不会记得任何事，task 里要写全背景、输入和期望输出；用完即销毁。
子 Agent 用哪个大脑由你按任务选（engine 参数，可选项见工具说明）：要快、要并行多个、工具步骤多 → 在线引擎；涉及隐私文件、离线或简单任务 → 本机模型；拿不准就不填，跟随你自己。
任务状态里的用户要求、决定和关键信息会自动附在子 Agent 的任务后面，task 里不用重抄，但要写清这次具体做什么。
### 后台子 Agent
- 子任务要跑很久、而等的时候你还能做别的，或者想边跑边看它们的阶段性结果 → 派发时设 background: true，立刻返回 id，你接着做别的。
- 它们的中途汇报（report_to_parent）和最终结果会在你做完当前一步时自动送到，标着「子 Agent「名字」（id）中途汇报 / 完成了」。
- 要给某个子 Agent 追加指示（比如从另一个子 Agent 拿到了它需要的信息）→ message_agent；方向错了、不需要了 → stop_agent；手头没别的事只想等 → wait_agents。
- 这一轮结束前会等后台子 Agent 全部结束；它们的结果到了要整合进最终回答。
- 用户也可能直接给某个子 Agent 发消息或停掉它：被停掉的不要重新派发同样的任务。
- 子 Agent 一直不回来：wait_agents 等不到时会给你一张状态表（在做什么、多久没动静、第几步、跑了多久），由你决定继续等、追加指示还是停掉。被停掉、卡住（5 分钟没有任何输出会被自动停掉）或失败的，都会交回做到一半的结果，能用的直接用。
- 有时间要求时派发带上 deadline_seconds：到点先提醒它交付，再过 60 秒还没交就停掉。
子 Agent 按「结论 / 依据 / 改动 / 未完成与风险」回报。结果回来后由你核对：依据站不住、改动没验证、或者和你掌握的情况对不上时，用 read_agent_log 看它的完整过程，或者自己读文件、跑命令确认；关键结论记进任务状态（facts、decisions）。整合后再回答用户，不要原样转发。
```

`subGuidance`（:181-188，以一个空行开头、末尾没有换行）：
```

## 再往下派
你的任务还能拆时，可以像主 Agent 一样派自己的子 Agent（你就是它们的父级），用同样的标准：
{splitGuidance}
- 下级看不到你的任务和背景：task 里写全它要知道的；它们的结果交给你，由你核对、整合后写进你自己的最终结果。
- 下级也可以放后台（background: true），用 message_agent、stop_agent、wait_agents 管理；你结束前会等它们全部结束。
```

`subAgentPreamble(name, depth)`（:191-210，末尾没有换行；回报格式四行多缩进两个空格）：
```
你是临时子 Agent「{name}」，由主 Agent 派发任务，用户看不到你的过程，任务结束后你会被销毁。
- 只做交给你的任务，做完直接给出最终结果；结果要自包含，主 Agent 会拿它继续工作。
- 不要向用户提问；需要用户决定的事，写进结果里说明。
- 工具失败时读错误信息、换方法重试；实在做不成就如实报告做到了哪一步。
- 任务后面附的「主任务的背景」是用户的要求，必须遵守。
- 做到一半有了阶段性结果、或发现会影响全局的问题，可以用 report_to_parent 先交给派你的父级，然后接着做。
- 工作中可能收到用户或父级追加的指示（会标明来源），和你的任务一起考虑，有冲突以它为准。
- 改了文件或系统，改完要运行、测试或读回确认。
- 最终结果按这个格式写，主 Agent 要靠它核对（没有的项写「无」）：
  结论：一两句话回答交给你的任务
  依据：文件路径（带行号）、网址、命令和输出的关键几行
  改动：改了哪些文件、执行了哪些会改变系统的命令，改完怎么验证的
  未完成与风险：没做完的、没验证的、需要用户决定的
{depth >= 4 ? "- 你已经在第 {depth} 层了：除非任务明显还能拆成几块独立的部分，否则自己做，不要再往下派。" : "- 你的任务若还能拆成更小的部分，也可以再派临时子 Agent（层数不限）。"}
```

子 Agent 系统提示：`subAgentPreamble(name, depth) + "\n\n" + profile.role + (enableTools ? tools.guidance(team: .sub) : "")`（`SubAgentRunner.swift:490-492`）；
`ToolRegistry.guidance(team:)`（`ToolRegistry.swift:100-106`）：`WorkingMethod + (自扩展 ? MetaTools.guidance) + (isTeamEnabled ? (main ? guidance : subGuidance) : "") + capabilities + lessons`。
主 Agent 用 `.main`（`AgentRunner.swift:323`）。

#### 1.4.7 设置

`subAgentTurnLimit: Int`（`Models/AppSettings.swift:35, 78, 105`）：一轮里最多派多少个子 Agent（所有层加起来），只防失控，0 = 不限；默认 **100**；读入时 `max(0, …)`。
设置页（界面负责）：开关「限制每轮派发的子 Agent 总数」（开 = 100、关 = 0，help「所有层加起来。默认 100 个，只防失控：到了就不再派，用已有结果收尾」），开着时步进器「最多 {n} 个」10…1000 步长 10。

### 1.5 v1.9（e2f8f41）托盘计量（`AgentRunner.swift:13-14, 1378-1412`、`App/FridayApp.swift:227-277`）

- `liveAgents` = (主 Agent 在生成 ? 1 : 0) + `team.running.count`（含排队、在等下级的子 Agent，所有对话）；`liveSpeed = round((mainLiveSpeed + Σ team.speed(running)) × 10) / 10`。
- `startMeter()`（send 时调）：已在跑就返回；循环：`isGenerating || !team.running.isEmpty` 时 `updateMeter()`（值变了才写）然后睡 1 秒；条件不成立时两者归零、结束。
- `mainLiveSpeed(conv, now)`：`streamConversationID != conv` → 0；否则主 Agent 最近 3 秒 chunk 的 `windowSpeed`（`beginPass` 清空窗口，预填阶段速度为 0）。
- `MenuBarLabel.speedText(speed)`（:235-244）：≤ 0 →「0 t/s」；按 T（1e12）/ G（1e9）/ M（1e6）/ K（1e3）第一个 `speed >= 量级` 的：`value = speed / 量级`，
  `value >= 100 ? "%.0f" : "%.1f"` + 「 {单位}t/s」；都不够 → `speed >= 10 ? "%.0f t/s" : "%.1f t/s"`。图标右边两行「{n} AI」「{speedText}」；菜单里「{n} AI 在工作 · {speedText}」（liveAgents > 0 时）。

### 1.6 持久化字段（Codable 键与 Swift 完全相同）

| 类型 | 新键 | 类型 / 规则 |
|---|---|---|
| `ToolEvent` | `stopped` | Bool?，nil 不写 |
| `ToolEvent` | `usage` | AgentUsage?，nil 不写 |
| `AgentUsage` | `model`、`thinking`、`temperature`、`generatedTokens`、`promptTokens`、`generateTime`；`service` | 前六个**必写**（Swift 合成的 Decodable 对带默认值的 var 也要求键存在）；`service` nil 不写；temperature / generateTime 是 Double，tokens 是 Int |
| `ChatMessage` | `fromAgent` | String?（子 Agent 名字），nil 不写 |
| `ChatMessage` | `stoppedByUser`、`interjected` | Bool?（主循环负责，列在这里便于核对） |

`queued`、`agentName`、`transcript` 不变。运行时状态（runs、队列、收件箱、activity、开始时间、速度窗口）不落盘；Swift 没有持久化子 Agent 的开始 / 结束时间。

### 1.7 设置键（`settings.ini`，与 UserDefaults 同名同默认值）

| 键 | 默认 | 负责 |
|---|---|---|
| `subAgentTurnLimit` | 100 | 本文 |
| `agentTreeVisible` | false（`@AppStorage`，`Views/ChatView.swift`） | 界面 |
| `agentTreeWidth` | 320.0（拖动范围 260…560） | 界面 |

### 1.8 诊断文字

- ToolRegistry 的诊断扩展（`ToolRegistry.swift:39`）：「## 子 Agent\n临时创建、用完即销毁；用 delegate_task / run_parallel_tasks 派发，数量和层数都不限（每轮总数上限见设置，只防失控），按系统承载排队。\n」+ schedulerDiagnostics。
- EngineStore：见 §1.3.10。SubAgentScheduler：见 §1.3.8。

---

## 2. Qt 映射

### 2.1 文件清单

新增：

| Qt 文件 | 对应 Swift | 内容 |
|---|---|---|
| `src/core/agent/AgentTeamState.{h,cpp}` | `AgentTeamControl.swift:12-194` | 团队状态（内部加锁、任意线程可调；界面观察的变化在主线程发信号） |
| `src/core/agent/AgentTeamControl.cpp` | `AgentTeamControl.swift:196-473` | `AgentRunner` 的团队方法（启动、停止、发消息、让出名额、派发检查、上级链、通知文字） |
| `src/core/agent/AgentLiveness.{h,cpp}` | `AgentLiveness.swift` | `SubAgentEnded`；`AgentRunner` 的状态文字、看门狗、期限、打转、半成品 |
| `src/core/agent/QueuedInput.h` | `QueuedInput.swift` | 和主循环共用（主循环负责 v1.6，本文要求有 `from`） |
| `src/core/agent/AgentTree.{h,cpp}` | `AgentTreePanel.swift` 的数据部分 + `MenuBarLabel.speedText` | 纯函数，可单测 |
| `src/core/llm/engine/RequestGate.{h,cpp}` | `RequestGate.swift` | 在线并发闸门 |
| ~~`src/app/viewmodels/AgentTreeModel.{h,cpp}`~~ | `AgentTreePanel` 的数据供给 | **已由 PORT_V15_V19.md §3.11.8 / §5.4 取代**：归 tree-tray，声明以 ui.md §3.9 为准 |
| `tests/tst_team.cpp` | `SelfTests.swift` 的团队 / 树 / 闸门部分 | 见 §4（另有 `tst_team_runner.cpp`、`tst_team_inproc.cpp`、`tst_request_gate.cpp`，PORT §5.2 / §6.A） |

修改：`agent/AgentProfiles.{h,cpp}`（7 个工具、新提示词、similarity、去 maxDepth）；`agent/SubAgentRunner.{h,cpp}`（SubRun、runTeamTool、runSubAgent）；
`agent/SubAgentScheduler.{h,cpp}`（优先级队列、让出 / 拿回、删在线部分）；`agent/ToolRegistry.{h,cpp}`（guidance(Role)、mainAgentSpecs、预检、诊断）；`agent/ToolTypes.h`（runsInBackground）；
`agent/TurnCheck.cpp`（AgentLog 渲染 agent_message）；`agent/AgentRunner.{h,cpp}`（成员 + 钩子，见 §3.2）；`agent/DebugHooks.{h,cpp}`；
`models/ChatModels.{h,cpp}`（AgentUsage、ToolEvent.stopped/usage、ChatMessage.fromAgent、toJson/fromJson/toVariant/operator==）；`models/AppSettings.{h,cpp}`（subAgentTurnLimit）；
`llm/engine/ChatEngine.h`（interjection）；`llm/engine/OpenAIChatEngine.{h,cpp}`（闸门、429、requestPriority、onQueued、interjection 调用点）；`llm/engine/LocalLlamaEngine.{h,cpp}`（不过闸门）；
`llm/engine/EngineStore.{h,cpp}`（注释、诊断「起步并发」）。文档：ARCHITECTURE.md §3.3 / §3.5 / §5.3 / §5.4 / §9，LOCAL_INFERENCE.md（v2.0 已删除）§A.3 / §E.4，BACKENDS.md（v2.0 已删除）§2.10 第 4 条（本地名额规则改成 §2.5）。

删除：`SubAgentScheduler::acquireOnline / releaseOnline`；`SubAgentRunner.cpp` 的 `t_heldLeases` / `HeldLeasesScope`；`retryLocalAfterPoolFull(holding=false)` 分支与 `poolFullWaitMs`；`AgentTeamTools::maxDepth`
（`maxDepth` 由**骨架**删并同步改 FridayApp / AgentsSettingsView / ToolRegistry / tst_registry，PORT_V15_V19.md §6.D）。

另外（**PORT_V15_V19.md §3.6.3、§3.6.4、§3.10 新增**，归 team）：`llm/LoadedModel.{h,cpp}`、`llm/LlamaServer.{h,cpp}`（`sharedSlot()`，子进程模式借不到槽位时不再发 `-1`）；
`llm/ModelManager.{h,cpp}`（planContext 的 `baseExtraSlots = 3` + 第 4 步、`maxExtraSlots = 32`）；`util/Worker.{h,cpp}`（阻塞等待让出线程池名额）。

### 2.2 线程与等待模型（沿用 Qt 现有做法，ARCHITECTURE.md §3）

Swift 全在 `@MainActor` 上靠 async 挂起；Qt 是「工作线程里阻塞式顺序书写 + 主线程只做界面状态」：

| Swift | Qt |
|---|---|
| `Task { @MainActor in runSubAgent }`（每个子 Agent 一个 Task） | 同步 delegate：**就在调用方线程**跑，套一个子令牌（`CancelScope`）；parallel：每个子 Agent 一个 `Worker::spawn`（子令牌）；后台：`Worker::run`（**独立令牌** `Worker::newToken()`） |
| `task.cancel()` / `Task.isCancelled` | `CancelToken::cancel()` / `CancelToken::currentIsCancelled()`；阻塞辅助函数（Http、ProcessRunner、Promise::wait、Worker::sleep）自动响应 |
| `CheckedContinuation` 的 waiters / 队列 | `Promise<bool>`（`waitFor(ms, token)`）；`AgentTeamState`、`SubAgentScheduler`、`RequestGate` 内部 `std::mutex`，**持锁时绝不阻塞、绝不 runOnMainSync** |
| `@Observable` 属性 | `AgentTeamState` 的 `revision` + `changed()`（合并发，主线程）；~~`AgentRunner` 的 `liveAgents / liveSpeed` + `liveChanged()`~~（**已由 PORT_V15_V19.md §3.2 取代**：计量归 core，信号 `liveMeterChanged`；另有 `AgentTeamState::clock` + `clockChanged()`，core 的计量每秒 `tickClock()`） |
| 看门狗 / 计量的 `Task.sleep` 循环 | 主线程 `QTimer`（5000 ms / 1000 ms），从工作线程 `runOnMain` 启动；回调只做非阻塞的事（cancel 令牌、入队、记日志） |
| `@MainActor` 上直接改 `parent.blocksForUpdate` | SubRun 的本地块只在它自己的线程读写；别的线程经它的**信箱** `postEdit` 投递修改，同时 `updateEvent` 改存档（§2.4） |

主线程从不等待工作线程（`stopAgent`、`sendToAgent`、`cancelTeam`、看门狗、计量都不阻塞）。

**线程池（PORT_V15_V19.md §3.10 新增，归 team）**：层数 / 数量不限之后，父线程阻塞等子线程可能把 Worker 池的 256 个线程占满而死锁。定稿：`Promise::waitImpl`
真正阻塞时，若当前是 Worker 池线程就先 `QThreadPool::releaseThread()`、等完 `reserveThread()`（RAII、可嵌套），所有等待（审批、并行父级、`waitForNotice`、调度器、闸门、
`Worker::sleep`）都经过它；`Worker::shutdown` 把让出中的线程算进去；再加 `AgentTeamState::maxLiveRuns = 1000` 的安全上限（`checkDispatch`，PORT §3.6.6）。

### 2.3 三种派法在 Qt 里怎么跑

> 下面伪代码里 `waitingWithoutSlot(…, [&]{ …; return x; })` 的「返回 body 的结果」写法**已由 PORT_V15_V19.md §3.3（D5）取代**：签名是
> `void waitingWithoutSlot(ToolSink&, const AgentTeamState::Activity&, const std::function<void()>& body)`，body 里把结果写进外面按引用捕获的变量。
> `Activity::waitingChildren()` 等工厂名不变，工具名的工厂是 `Activity::tool(title)`（不是 `toolNamed`）。

共同步骤（== `launchSubAgent`）：先 `team->registerRun(id, Run{…, token, deadline, depth})`（**在线程开始前登记**，否则 runSubAgent 里的 `setActivity` 会落空）、`team->addLaunched(conv, 1)`、`startWatchdog()`。

1. **同步 delegate**：`waitingWithoutSlot(sink, WaitingChildren, [&]{ token = current->makeChild(); register; CancelScope scope(token); return runSubAgent(...); })`；
   `qScopeGuard { team->unregister(id); team->removeNotices(key, id); }`；成功 `withReports`；`catch (...)` → `std::rethrow_exception(endedError(std::current_exception(), id))`。
   父级取消 → 子令牌随之取消（== Swift 的 `onCancel: launched.cancel()`）；`stopAgent(子)` 只取消子令牌，父级照常继续。
   不另起线程的理由同现有注释（不多占池线程）；线程局部的槽位租约栈支持同线程嵌套借还。
2. **parallel（等全部）**：每项 `Worker::spawn(current->makeChild(), …)`，线程里：`runSubAgent` → 成功 `withReports` + `updateEventEverywhere(child.id, sink, 成功)`；
   失败 → `endedError` → 分类（§1.3.2）→ `updateEventEverywhere(child.id, sink, 失败)` → 不是 stopped 才记日志；最后 `unregister` + `removeNotices`。
   父线程 `waitingWithoutSlot(sink, WaitingChildren, [&]{ 逐个 promise->wait(nullptr) })`（被取消时也等它们真的停下），按顺序拼 sections。
   最后保留 Qt 现有的 `CancelToken::checkCurrent()`（整轮停止时父卡片按停止处理，见 §2.8）。
3. **后台**：`Worker::run(Worker::newToken(), …)`：`runSubAgent`（try/catch 存下结果）→ `stoppedBy = team->stoppedBy(id)` → `ended = 失败 ? endedError(…) : null`（注销前）→
   `wasRegistered = team->unregister(id)` → 卡片（`updateAgentEvent`）与通知（§1.3.2）→ `if (!isStopping() && wasRegistered) team->post(notice, parentKey)`（Qt 加固，见 §2.8）→ `finishBackgroundParentIfDone`。
   令牌**不是**父级令牌的子令牌（== Swift 的独立 Task）：父级退出时由 runSubAgent 的收尾以「上级「{name}」」显式停掉（通知文字才对）；整轮停止由 `cancelTeam()`、收尾由 `team->reset()`、退出应用由 rootToken 取消。

`runTeamTool` 骨架（Qt）：
```cpp
const int depth = sink.depth() + 1;
if (call.name == delegateName() || call.name == parallelName())
    checkDispatch(call.name, call.arguments, sink);   // 先于参数校验（== Swift）
if (call.name == delegateName()) { … }                 // 上面 1 / 3
if (call.name == messageAgentName()) { … }             // §1.3.3
if (call.name == stopAgentName()) { … }
if (call.name == waitAgentsName()) return ToolOutput(waitingWithoutSlot(sink, WaitingChildren, [&] { return waitForChildren(sink, seconds); }));
if (call.name == reportToParentName()) { … }
if (call.name == readLogName()) return ToolOutput(readAgentLog(args, sink.conversationID()));
if (call.name == parallelName()) { … }                 // 上面 2 / 3
throw AgentError::unknownTool(call.name);
```

`runSubAgent` 的 Qt 结构（在现有代码上改）：
```cpp
// 大脑选择、Brain 读取：不变
SubRun run(this, conversationID, depth, eventID, profile, brain.assumeThinking);
// 本地排队（§1.3.8）：queued=true / activity Queued → scheduler.acquireLocal(depth) → holdsLocalSlot=true …；被取消：queued=false 后抛
const auto release = qScopeGuard([&] { if (local) scheduler.releaseLocal(run.holdsLocalSlot); run.holdsLocalSlot = false; });
if (CancelToken::currentIsCancelled() || isStopping()) throw CancellationError();      // 排队期间被停：不开跑
std::optional<LoadedSlotLease> lease; if (local) lease.emplace(runOnMainSync([this] { return models->loadedModel(); }));  // 序列从开跑借到结束（等下级时也不还）
engine = …; instructions = preamble + "\n\n" + role + (enableTools ? tools->guidance(AgentTeamTools::Role::Sub) : ""); tools = subAgentSpecs(...)
team->registerSubRun(eventID, &run);
try {
    const auto orphans = qScopeGuard([&] { for (const QUuid& child : team->backgroundChildren(eventID, conversationID))
                                               stopAgent(child, QStringLiteral("上级「%1」").arg(profile.name)); });
    engine->interjection = [this, &run, vision = engine->supportsVision()] { return takeAgentInterjection(run, vision); };
    maxTokens / temperature / enableThinking；run.isLocal = local；run.startUsage(…)
    if (online) { online->requestPriority = depth; online->onQueued = [this, eventID](bool waiting) {
                      updateEvent(eventID, [waiting](ToolEvent& e) { e.queued = waiting; });
                      team->setActivity(eventID, waiting ? Activity::queued() : Activity::generating()); };
                  reasoningEffort；maxToolRounds = maxSteps > 0 ? maxSteps + 2 : 0; }
    journal「派发子 Agent「{name}」（第 {depth} 层）」；brief = join([task, subAgentNote, ancestryNote], "\n\n")
    streamWithPoolRetry(brief)                 // 现有 KVCacheFullError 重试循环，改成 scheduler.retryLocalAfterPoolFull(resumeBoost + depth)
    while (!CancelToken::currentIsCancelled()) {   // §1.3.5
        if (!team->hasNotices(eventID) && !team->hasQueue(eventID)) {
            if (team->backgroundChildren(eventID, conversationID).isEmpty()) break;
            waitingWithoutSlot(run, Activity::waitingChildren(), [&] { team->waitForNotice(eventID); return true; });
            continue;
        }
        const QList<EngineMessage> messages = takeAgentInterjection(run, vision);
        if (messages.isEmpty()) { if (isStopping() || team->stoppedBy(eventID)) throw CancellationError(); continue; }   // Qt：防空转（令牌取消前的空档）
        run.renderPass(true);
        streamWithPoolRetry(messages);
    }
    CancelToken::checkCurrent();
} catch (...) {
    run.renderPass(true);                                    // == defer { run.finishPass() }：已经写出来的文字定型、写回运行记录
    team->setPartial(eventID, partialResult(run));           // 半成品在 SubRun 还活着时算好，留给调用方的 endedError
    team->unregisterSubRun(eventID);
    throw;                                                   // 原样抛（== Swift：runSubAgent 不包装，调用方 endedError）
}
team->unregisterSubRun(eventID);
run.renderPass(true);
return ToolOutput((text 空 ? "（子 Agent 没有给出文字结果）" : text) + logNote, run.media());
```
（引擎建好之前就抛的错误不经过 catch：没有 SubRun 登记、partial 为空 == Swift 的 `subRuns[id] == nil`。现有测试 `subAgentEngineChoice` 照旧拿到原始文字。）

### 2.4 SubRun 在 Qt 的变化

- 新成员：`steps()`（只读）、`lastCall`、`holdsLocalSlot`、`isLocal`、`usage`、`startUsage / completePass / consume / receive`（§1.4.4）。`consume` 就是给引擎的 `EngineEventHandler`：
  Chunk → `receive`（noteOutput、noteTokens、passBuffer、renderPass）；Done → `completePass`；**Progress（Qt 版的工具参数进度）→ noteOutput + noteTokens**（Qt 版新增，§2.8）。
- `replaceKeepingTranscript` 多拷 `stopped`。
- **信箱**：`postEdit(eventID, body)` 任意线程调用，压进带锁的待办表；`applyPendingEdits()` 在它自己的线程上、`publish / append / replace / replaceKeepingTranscript / renderPass(final) / finalText / partialResult` 之前执行。
  `updateAgentEvent(id, parentKey, body)`：`team->postEdit(parentKey, id, body)`（父级是登记着的 SubRun 才投）**并且**`updateEvent(id, body)`（存档，界面马上看到）。
  `updateEventEverywhere(id, sink, body)`：sink 是 SubRun 且当前就在它的线程 → 直接改本地 + publish；在别的线程 → `postEdit` + `updateEvent`；sink 是主 Agent → `updateEvent`。
  （Swift 里这些都在主 actor 上直接改父级的本地副本；`merge` 对子卡片状态「以本地为准」，所以本地副本必须拿到下级的终态，否则父级下次发布会把存档改回「运行中」，`partialResult` 也看不到下级的结果。）
- **merge（§1.3.6）+ Qt 加固「终态不回退」**：本地状态是 running / awaitingApproval、存档是 succeeded / failed / denied 时，status / result / stopped / media 取存档。
  合法的状态只会从运行走到终态，这条不改变任何正常结果，只挡住两种 Qt 特有的竞态：信箱还没应用时的发布、`markStopped` 之后还在退出的子 Agent 线程的发布。
- `transcriptUpdate` / `messageTask(eventBody(...))` 现在只找「正在写的这条消息」：v1.6 起一轮会拆成几条助手消息（工作中补充、汇报），后台子 Agent 的卡片可能在更早的那条里，
  **必须**改成按 Swift `updateEvent`（`AgentRunner.swift:1279-1290`）在这一轮全部助手消息里从新到旧找（主循环提供 `turnMessageIDs`，§3.2）。

### 2.5 「本地按剩余内存」在 Qt 上的精确规则（两种推理模式通用）

事实（读代码）：
- MLX 的 KV 按需分配，Swift 用「当前可用内存 ≥ 每个子 Agent 预估 + 1.5 GB」决定能否再起一个。Qt 的 KV 池在**加载时**一次性分配：`ModelManager::planContext`（`src/core/llm/ModelManager.cpp:485-547`）
  按当时的可用内存 / 显存算额外序列数 `extra = min(3, floor(spare / (kvPerToken × 12000)))`（异常退出记录时减半），`-np = 1 + extra`；
  `AgentRunner::updateLocalSlotLimit` 设 `scheduler.localSlotLimit = slotCount − 1 = extra`（进程内不含内部工作序列）。也就是说「剩余内存」在加载时已经折算成了 **S = localSlotLimit 个非主序列**。
- 每个本地子 Agent 用 `LoadedSlotLease` 借一个非主序列（借不到 = `id_slot -1`）。`-1` 时：进程内挑**空闲的非主序列**（公共前缀最长、其次最久没用，`src/runtime/llama/Scheduler.cpp:306-323`），
  只有一个序列时才用主序列；子进程模式 llama-server 按相似度 / 最久未用在**所有空闲槽位**里挑，**可能挑中主对话的 0 号槽位**（BACKENDS.md §2.10，v2.0 已删除）。
  （**已由 PORT_V15_V19.md §3.6.3（Q3）改掉**：子进程模式借不到时改用 `LoadedModel::sharedSlot()`——最久没被借出的非主槽位的显式编号，不再发 `-1`；进程内仍发 `-1`。）
- 池满时两边都先清空闲序列的 KV（进程内：非主优先），所以「在等下级」的子 Agent 的 KV 是第一批被让出去的，它拿回名额后从分歧处重新预填（cache_prompt / 账本自动处理）。

规则（把 Swift 的「名额 = 正在算的子 Agent，内存 = 还占着 KV 的子 Agent」映射到「名额 = 活跃数，内存 = 序列」）：
1. **名额（running）**= 正在生成或跑工具的本地子 Agent。等下级、等确认、等收件箱（`waitingWithoutSlot`）时让出名额，**序列不还**（KV 留在序列里，== Swift「KV 缓存已经在内存里」）。
2. **序列份额（alive）**= 已经开跑、还没结束的本地子 Agent（含让出名额的）。结束时还。
3. 放行判定（按顺序）：
   - `running == 0` → 一定放行（Swift 的「没有正在跑的就一定放行」：嵌套多少层都不会互相卡死；这时它可能借不到空闲序列，用 `-1` 交给后端，可能挤掉一个在等的子 Agent 的 KV）；
   - 调试构建 `FRIDAY_LOCAL_SLOTS=<n>`：`running >= n` → 否；
   - `running >= S` → 否；
   - **新开的**还要 `alive < S`（有空闲序列；在等下级的子 Agent 的序列算占用，== Swift 里它们的 KV 还占着内存）；**拿回名额的**不看这条（它自己有序列）；
   - 池满降档：`poolLimit > 0 && running >= poolLimit` → 否（Qt 现有规则保留）；
   - `Platform::availableMemory() >= localBudget + headroom`（Swift 原条件，保留；预分配池下它基本只防系统内存吃紧）。
4. 排队：严格按 `(priority 大, 序号小)` 取队首判定，队首放不进就停（不越过它）。新开 = `depth`；拿回 = `resumeBoost + depth`；池满重排 = `resumeBoost + depth`（它也占着序列）。
   拿回的优先级 ≥ 10000，永远排在新开的前面，所以「队首是新开的、需要空闲序列」时后面也不会有能拿回的被挡住。
5. 无死锁论证：在等的子 Agent 不占名额；任何时刻若所有活着的子 Agent 都在等，`running == 0` 放行队首；队首（最深、最先）要么做完（释放），要么也去等（running 又回 0）——总有进展。
   序列不够时退化成 `-1` 串行，正确性不受影响，只多重新预填。
6. 池满重试（`retryLocalAfterPoolFull`）：上限降到「当时在跑的个数 − 1」（至少 1；3 秒窗口内只降一次），让出名额，**不唤醒别人**、能马上拿回就拿回，否则按 `resumeBoost + depth` 排队；
   本地子 Agent 全部结束（`alive == 0` 且队列空）或换模型后恢复。嵌套借名额的分支删除（不再有借名额）。
7. ~~子进程模式的额外风险：规则 3 第一条放行的 `-1` 请求可能落到主对话的 0 号槽位~~——**已由 PORT_V15_V19.md §3.6.3（Q3）取代**：子进程模式改用 `sharedSlot()`
   （显式的非主槽位，忙就由 llama-server 排队），不再落 0 号槽位；只剩「只有一个序列（S == 0）」时两种模式都只能用主序列（现状，进程内清掉主序列归属，下一轮从磁盘 / 前缀恢复）。
   llama-server 池满时自己清空闲槽位（可能含主槽位）是服务端行为，记录在案不处理。
8. 诊断文字：Swift 句子 + Qt 现有补充「；本地模型可给子 Agent 用的槽位 {S} 个」+（`alive > running` 时）「，其中 {alive − running} 个被在等下级 / 确认的子 Agent 占着」+（降档时）「（KV 池刚满过，暂时最多同时跑 {poolLimit} 个）」；
   **删掉**「；在线引擎运行中：…」。

~~`maxExtraSlots = 3` 是 Qt 现有上限；是否放宽见 §5。~~ **已由 PORT_V15_V19.md §3.6.4 取代**：`maxExtraSlots = 3` 改名 `baseExtraSlots`（保底），
planContext 加第 4 步（主对话到顶、份额到 16384 后，用富余那一半里剩下的再加序列），技术上限 `maxExtraSlots = 32`。规则 3 加一条「主 Agent 池满重发」的配套：
`waitForLocalEnd(timeoutMs)`（PORT §3.6.2、§3.6.5）。

### 2.6 RequestGate 接进 OpenAIChatEngine

- `OpenAIChatEngine::transport` 是每段请求一次（Swift 的 `streamPass`），闸门就放在它里面，**名额一直占到读完整个流**。见 §3.5 的伪代码；
  Qt 的 `send()` 已经在一次阻塞调用里读完整个 SSE（`Http::streamLines`），所以「读流出错」= `send()` 抛异常、或回调里存下的 `failure`、或 `handlerStopped`（调用方不要了 == Swift 的取消）——三者都 `release(Failed)`。
- `LocalLlamaEngine` 子进程模式也走基类 `transport`：`requestGate()` 返回空 → **不过闸门**（Swift 的本地引擎根本不经过 OpenAIChatEngine），429 / 502 / 503 / 504 / 529 仍按现有共用计数退避（最多 10 次）；进程内不走 HTTP。
- 起标题、主 Agent 的在线请求也过闸门（priority = mainPriority）；看图 / 能力提供方（OpenAIStyleAPI）不过（== Swift）。
- 测试钩子：退避一律经过虚函数 `backoff(seconds)`（现有），抖动经过 `rateLimitJitterMs()`；10 分钟上限按**请求的**等待秒数累计，测试可以不真睡就验证。

### 2.7 看门狗与计量的定时器

> 计量（`m_meter`、主 Agent 速度窗口）**已由 PORT_V15_V19.md §3.2 / D7 取代：归 core**（成员名由 core 定，信号 `liveMeterChanged`，每秒顺带 `team->tickClock()`）。
> team 只有看门狗：`m_watchdog` 在 `startWatchdog()` 里懒建（team 不改 AgentRunner 的构造函数）。

- `AgentRunner` 持有两个主线程 `QTimer`：`m_watchdog`（5000 ms，`startWatchdog()` 从工作线程 `runOnMain(this, …)` 启动，已在跑不重启；超时回调里 `team->runningCount() == 0` 就停，否则 `watchdogTick()`），
  `m_meter`（1000 ms，`send()` 里 `startMeter()`：立刻 `updateMeter()` 一次再启动；条件不成立时归零并停）。
- 主 Agent 的速度窗口：回合线程的 `noteChunk` 每个 chunk 往一个带锁的 `m_liveChunks` 追加 `(now, tokens)`、删 3 秒前的，`beginPass` 清空；主线程的 `mainLiveSpeed` 在锁内拷一份算 `windowSpeed`。
- `stallLimitSeconds()`：调试构建每次读 `FRIDAY_STALL_LIMIT`；测试另提供 `AgentRunner::setLivenessTimingForTests(tickMs, graceSeconds, stallSeconds)`（只在测试 / 调试构建里），否则期限 / 看门狗测试要跑一分钟以上。

### 2.8 与 Swift 的取舍（Qt 版新增 / 保留的偏差，全部写进代码注释）

1. 并行派发在**整轮停止**时父卡片按「已停止」（Qt 现有，测试 `parallelStopped`）；Swift 仍标成功。保留。
2. `EngineEvent::Progress`（工具参数在生成）算子 Agent 的「有输出」并计入估算 token：否则模型一口气写几分钟的大文件参数会被看门狗误停。Swift 在线引擎没有这个事件。
3. 后台子 Agent 结束时，若它已被 `reset()` 注销（整轮已经收尾），不再往收件箱投通知（Swift 有竞态：`isStopping` 已被清掉后迟到的「被停止」通知会漏进下一轮）。
4. 子 Agent 收尾循环在「有待处理但取不到」（`isStopping` / 已被停，令牌还没取消的空档）时直接抛取消，不空转。
5. merge「终态不回退」（§2.4）。
6. 本地名额的序列规则（§2.5）是 Qt 对「剩余内存」的落地；~~`maxExtraSlots = 3` 不变~~（**已由 PORT_V15_V19.md §3.6.4 取代**：保底 3、富余再加、技术上限 32）。
7. （PORT_V15_V19.md 新增）子进程模式子 Agent 借不到槽位时用 `sharedSlot()` 不发 `-1`（Q3）；阻塞等待让出线程池名额 + 同时活着的安全上限 1000（Q5，§3.10）。

---

## 3. 接口（C++ 声明）

### 3.1 `AgentTeamState`（`src/core/agent/AgentTeamState.h`）

> **已由 PORT_V15_V19.md §3.4 取代**（骨架按那里写全头文件）。差别：`Activity` 是 `ActivityKind kind` + `QString title` + 工厂 `tool(title)`（不是字段 `tool` + `toolNamed`）；
> QML 方法用同名 QString 重载 `isRunning / queue / speed / passTokens`（不是 `isRunningID / queueVariant / speedOf / passTokensOf`），另有 `isStopRequested / hasActivity`；
> 新增 `clock` 属性 + `clockChanged()` + `tickClock()`、`cancelRuns(conversationID)`、`runCount()`、`maxLiveRuns = 1000`。其余与下面相同。

```cpp
// 对应 Swift：Agent/AgentTeamControl.swift 的 AgentTeamState（父子 Agent 往来的状态，按子 Agent 的事件 id）
// 线程：内部一把 std::mutex，任意线程可调；持锁时不阻塞、不 runOnMainSync。界面观察的部分（running、activity、steps、queues、treeFocus）
// 变了就把 revision 加一，并在主线程合并发 changed()（同一事件循环内多次变化只发一次）。
#pragma once
#include "agent/QueuedInput.h"
#include "util/CancelToken.h"
#include "util/Worker.h"

#include <QDateTime>
#include <QHash>
#include <QList>
#include <QObject>
#include <QSet>
#include <QString>
#include <QUuid>
#include <QVariantList>

#include <functional>
#include <mutex>
#include <optional>

struct ToolEvent;

/// 子 Agent 本地块副本的信箱（AgentRunner::SubRun 实现）。单独成接口：嵌套类 SubRun 不能前置声明，
/// 而 AgentRunner.h 要包含本头文件（waitingWithoutSlot 的参数是 Activity）
class AgentEditMailbox {
public:
    virtual ~AgentEditMailbox() = default;
    /// 任意线程：投递对它本地块里某个事件的修改，它自己的线程下次发布 / 读块前应用
    virtual void postEdit(const QUuid& eventID, std::function<void(ToolEvent&)> body) = 0;
};

class AgentTeamState : public QObject {
    Q_OBJECT
    Q_PROPERTY(int revision READ revision NOTIFY changed)
    Q_PROPERTY(int runningCount READ runningCount NOTIFY changed)
    /// 要在 Agent 树面板里打开的子 Agent（大写 UUID；空 = 没有）。界面打开后写回空
    Q_PROPERTY(QString treeFocus READ qmlTreeFocus WRITE setQmlTreeFocus NOTIFY treeFocusChanged)

public:
    enum class ActivityKind { Generating, Tool, WaitingChildren, WaitingApproval, Queued };
    struct Activity {
        ActivityKind kind = ActivityKind::Generating;
        QString tool;   // ActivityKind::Tool 的标题
        static Activity generating() { return {}; }
        static Activity toolNamed(const QString& title) { return { ActivityKind::Tool, title }; }
        static Activity waitingChildren() { return { ActivityKind::WaitingChildren, {} }; }
        static Activity waitingApproval() { return { ActivityKind::WaitingApproval, {} }; }
        static Activity queued() { return { ActivityKind::Queued, {} }; }
        bool operator==(const Activity&) const = default;
    };
    struct Notice {
        enum class Kind { Report, Finished, Failed, Stopped };
        QUuid agentID;
        QString agentName;
        Kind kind = Kind::Report;
        QString text;
    };
    struct Run {
        QString name;
        QUuid parentKey;                       // 主 Agent = mainKey()，子 Agent = 它的事件 id
        bool background = false;
        QUuid conversationID;
        QString assignment;                    // 父级交给它的任务原文
        CancelTokenPtr token;                  // == Swift Run.task（stopAgent / cancelTeam / reset 取消它）
        std::optional<QString> stoppedBy;
        std::optional<QDateTime> deadline;     // UTC
        int depth = 1;                         // Qt 版补充（调试钩子「最深」用；Swift 从 subRuns 读）
        std::optional<QString> partial;        // Qt 版补充：异常退出时 runSubAgent 算好的半成品（endedError 用）
    };

    /// 00000000-0000-0000-0000-00000000000A
    static QUuid mainKey();

    explicit AgentTeamState(QObject* parent = nullptr);

    // MARK: 登记（== register / unregister）
    void registerRun(const QUuid& id, Run run);
    /// 返回之前是否登记着（后台收尾据此判断是否已被 reset）
    bool unregister(const QUuid& id);
    std::optional<Run> run(const QUuid& id) const;
    QHash<QUuid, Run> runsSnapshot() const;
    bool isRunning(const QUuid& id) const;
    QList<QUuid> running() const;
    int runningCount() const;
    /// stopAgent 用：在、且还没被停过 → 记下 who 并返回它的令牌；否则返回空。调用方在锁外 cancel
    /// （CancelToken::cancel 会同步跑回调；本类所有取消都是「锁内收集令牌、锁外 cancel」，reset 同理）
    CancelTokenPtr markStoppedBy(const QUuid& id, const QString& who);
    std::optional<QString> stoppedBy(const QUuid& id) const;
    void setPartial(const QUuid& id, const std::optional<QString>& partial);
    std::optional<QString> partial(const QUuid& id) const;

    // MARK: SubRun（只活在 runSubAgent 的栈帧里：建好引擎后登记，退出前撤下；撤下在本类的锁内，之后不会再有人投递）
    void registerSubRun(const QUuid& id, AgentEditMailbox* run);
    void unregisterSubRun(const QUuid& id);
    /// == updateAgentEvent 的前半：parentKey 是登记着的 SubRun → 把修改投进它的信箱，返回 true
    bool postEdit(const QUuid& parentKey, const QUuid& eventID, std::function<void(ToolEvent&)> body);

    // MARK: 每轮总数
    int launched(const QUuid& conversationID) const;
    void addLaunched(const QUuid& conversationID, int count);

    // MARK: 活动、步数、速度（== setActivity / noteOutput / noteStep / noteTokens / endPass / speed）
    void setActivity(const QUuid& id, const Activity& activity);
    std::optional<Activity> activity(const QUuid& id) const;
    void noteOutput(const QUuid& id);
    void noteStep(const QUuid& id);
    int steps(const QUuid& id) const;
    std::optional<QDateTime> started(const QUuid& id) const;
    std::optional<QDateTime> lastOutput(const QUuid& id) const;
    void noteTokens(const QUuid& id, int count, const QDateTime& now = QDateTime::currentDateTimeUtc());
    void endPass(const QUuid& id);
    int passTokens(const QUuid& id) const;
    double speed(const QUuid& id, const QDateTime& now = QDateTime::currentDateTimeUtc()) const;
    struct Sample { QDateTime time; int tokens = 0; };
    static double windowSpeed(const QList<Sample>& samples, const QDateTime& now);
    /// 第一次返回 true（== deadlineWarned.insert）
    bool markDeadlineWarned(const QUuid& id);

    // MARK: 后台下级、收件箱
    QList<QUuid> backgroundChildren(const QUuid& key, const QUuid& conversationID) const;
    void post(const Notice& notice, const QUuid& key);
    QList<Notice> takeNotices(const QUuid& key);
    bool hasNotices(const QUuid& key) const;
    /// == notices[key].removeAll { agentID == id }
    void removeNotices(const QUuid& key, const QUuid& agentID);
    /// withReports 用：取出这个子 Agent 的 .report 文字，并删掉它在 key 收件箱里的全部通知
    QStringList takeReports(const QUuid& key, const QUuid& agentID);
    /// 等 key 的收件箱里有东西（或被 wake、超时、当前令牌取消）。不抛取消（== Swift），调用方自己查
    void waitForNotice(const QUuid& key, std::optional<int> timeoutMs = std::nullopt);
    void wake(const QUuid& key);

    // MARK: 发给子 Agent 的消息（界面观察）
    bool enqueue(const QUuid& id, const QueuedInput& item);   // 不在 running 返回 false
    QList<QueuedInput> queue(const QUuid& id) const;
    QList<QueuedInput> takeQueue(const QUuid& id);
    bool hasQueue(const QUuid& id) const;
    void removeQueued(const QUuid& id, const QUuid& itemID);

    // MARK: 收尾（== reset）：这段对话的 run 取消并注销、叫醒全部等待、清空全部收件箱、launched 归零
    void reset(const QUuid& conversationID);

    std::optional<QUuid> treeFocus() const;
    void setTreeFocus(const std::optional<QUuid>& id);
    int revision() const;

    // QML（参数 / 返回值是大写 UUID 字符串）
    QString qmlTreeFocus() const;
    void setQmlTreeFocus(const QString& id);
    Q_INVOKABLE bool isRunningID(const QString& id) const;
    Q_INVOKABLE bool hasActivity(const QString& id) const;
    /// [{id, text, from, attachments, createdAt}]
    Q_INVOKABLE QVariantList queueVariant(const QString& id) const;
    Q_INVOKABLE double speedOf(const QString& id) const;
    Q_INVOKABLE int passTokensOf(const QString& id) const;

signals:
    void changed();
    void treeFocusChanged();

private:
    void bumpLocked();   // revision += 1；第一次在这个事件循环里时 runOnMain 发 changed()
    mutable std::mutex m_mutex;
    QHash<QUuid, Run> m_runs;
    QHash<QUuid, AgentEditMailbox*> m_subRuns;
    QSet<QUuid> m_running;
    QHash<QUuid, QList<QueuedInput>> m_queues;
    QHash<QUuid, QList<Notice>> m_notices;
    QHash<QUuid, int> m_launched;
    QHash<QUuid, Activity> m_activity;
    QHash<QUuid, int> m_steps;
    QHash<QUuid, QDateTime> m_started, m_lastOutput;
    QSet<QUuid> m_deadlineWarned;
    QHash<QUuid, QList<Sample>> m_recentTokens;
    QHash<QUuid, int> m_passTokens;
    QHash<QUuid, QHash<QUuid, PromisePtr<bool>>> m_waiters;   // key → token → 叫醒用的 Promise
    std::optional<QUuid> m_treeFocus;
    int m_revision = 0;
    bool m_changePosted = false;
};
```

### 3.2 `AgentRunner` 新增（`AgentRunner.h`，实现分在 `AgentTeamControl.cpp` / `AgentLiveness.cpp` / `SubAgentRunner.cpp`）

> **已由 PORT_V15_V19.md §3.2–§3.3 取代**（team 的部分放在 AgentRunner.h 的两段 team 区里，骨架加齐）。差别：没有 `Q_PROPERTY team / liveAgents / liveSpeed`、`teamObject()`、
> `liveChanged`（`team` 是公有成员、QML 用单例 `AgentTeam`；计量归 core、信号 `liveMeterChanged`）；`waitingWithoutSlot` 不是模板，是
> `void waitingWithoutSlot(ToolSink&, const AgentTeamState::Activity&, const std::function<void()>&)`；`noteRepeat`、`repeatWarn / repeatBlock / stepNudge`、`pollingTools()`、
> `cancelTeam()`、`mainLiveSpeed`、`estimateTokens`、`startMeter / updateMeter`、`m_mainLastCall`、`m_liveChunks` 归 core；`conversationUsage` 删掉（AgentTreeModel 算）；
> `stallLimitSeconds()` 是非静态成员（测试可改）。下面「给主循环的钩子」表仍然有效（由 core 实现）。

```cpp
class AgentRunner : public QObject, public ToolSink {
    Q_OBJECT
    // …现有…
    Q_PROPERTY(QObject* team READ teamObject CONSTANT)
    /// 托盘：正在工作的 Agent 个数（主 Agent + 运行中的子 Agent）和速度合计（t/s，一位小数）；生成期间每秒更新，都停下归零
    Q_PROPERTY(int liveAgents READ liveAgents NOTIFY liveChanged)
    Q_PROPERTY(double liveSpeed READ liveSpeed NOTIFY liveChanged)

public:
    AgentTeamState* const team;     // 构造时 new AgentTeamState(this)

    // MARK: 父子往来（AgentTeamControl.cpp；任意线程）
    static QString agentMessageTool();                                    // "agent_message"
    static QString agentMessageNote(const std::optional<QString>& from);
    QUuid parentKey(const ToolSink& sink) const;                          // SubRun → eventID，否则 mainKey
    QString parentName(const ToolSink& sink) const;                       // 「父 Agent「X」」/「主 Agent」
    /// == stopAgent(_:by:)：只停这一个（令牌取消），其他照常
    bool stopAgent(const QUuid& id, const QString& by = QStringLiteral("用户"));
    bool sendToAgent(const QUuid& id, const QString& text, const QStringList& attachments = {},
                     const std::optional<QString>& from = std::nullopt);
    void removeAgentQueued(const QUuid& itemID, const QUuid& agentID);
    std::optional<std::pair<QUuid, QString>> runningChild(const QString& prefix, const QUuid& key, const QUuid& conversationID) const;
    /// 子 Agent 引擎的 interjection（它自己的线程）
    QList<EngineMessage> takeAgentInterjection(SubRun& run, bool supportsVision);
    static QString noticeText(const AgentTeamState::Notice& notice);
    static QString noticeTitle(const QString& text);
    /// 每轮总数上限 + 原样往下传（抛 AgentToolError）
    void checkDispatch(const QString& tool, const QJsonObject& args, const ToolSink& sink);
    std::optional<QString> ancestryNote(const QUuid& eventID, const QUuid& conversationID) const;
    /// == waitingWithoutSlot：sink 是占着本地名额的 SubRun 时让出、等完按 resumeBoost + depth 拿回；活动标成 waiting
    template <class F>
    auto waitingWithoutSlot(ToolSink& sink, const AgentTeamState::Activity& waiting, F&& body) -> std::invoke_result_t<F&>;
    /// 改一个子 Agent 的卡片：父级子 Agent 的信箱 + 存档（§2.4）
    void updateAgentEvent(const QUuid& id, const QUuid& parentKey, std::function<void(ToolEvent&)> body);
    /// 整轮停止：这段对话的子 Agent 全部取消（含后台），叫醒主 Agent 的等待（主循环 stop() 调）
    void cancelTeam();

    // MARK: 活性（AgentLiveness.cpp）
    static double stallLimitSeconds();          // 300；调试构建 FRIDAY_STALL_LIMIT=<秒>
    static constexpr int deadlineGraceSeconds = 60;
    static constexpr int repeatWarn = 3;
    static constexpr int repeatBlock = 5;
    static constexpr int stepNudge = 40;
    static const QSet<QString>& pollingTools(); // wait_agents、check_job、get_current_time
    std::optional<QString> agentStatusText(const QUuid& id, const QDateTime& now = QDateTime::currentDateTimeUtc(),
                                           bool compact = false) const;
    static QString duration(double seconds);
    void startWatchdog();                       // 任意线程（runOnMain 启动主线程 QTimer）
    std::optional<QDateTime> childDeadline(std::optional<double> seconds, const QUuid& parentKey) const;
    /// 记下这次调用，返回同样的调用连续出现了几次（轮询类返回 0）。主 Agent 的计数在 m_mainLastCall（回合线程）
    int noteRepeat(const ToolCall& call, ToolSink& sink);
    ToolOutput withReports(const ToolOutput& output, const QUuid& id, const QUuid& parentKey);
    /// 半成品（在 SubRun 自己的线程、退出前算）
    std::optional<QString> partialResult(SubRun& run) const;
    /// == endedError(_:of:)：整轮在停止不包装；被停 → SubAgentEnded(stoppedBy, "", partial)；取消 / 已是 SubAgentEnded 原样；其余 → SubAgentEnded(nullopt, 文字, partial)
    std::exception_ptr endedError(std::exception_ptr error, const QUuid& id) const;

    // MARK: 计量（v1.9）
    int liveAgents() const;
    double liveSpeed() const;
    double mainLiveSpeed(const QUuid& conversationID, const QDateTime& now = QDateTime::currentDateTimeUtc()) const;
    static int estimateTokens(const QString& chunk, bool local);
    /// 工具栏的会话合计：{speed, generated, prompt, running, main:{generated, prompt, speed, running}, sub:{generated, prompt, speed, agents, running}}
    Q_INVOKABLE QVariantMap conversationUsage(const QString& conversationID) const;

    // MARK: QML（卡片控件、树详情；参数是大写 UUID 字符串）
    Q_INVOKABLE bool stopAgent(const QString& eventID);
    Q_INVOKABLE bool sendToAgent(const QString& eventID, const QString& text, const QStringList& attachments = {});
    Q_INVOKABLE void removeAgentQueued(const QString& itemID, const QString& agentID);
    Q_INVOKABLE QString agentStatusText(const QString& eventID, bool compact = false) const;

    // MARK: 测试钩子（只在测试 / 调试构建里有）
    void setLivenessTimingForTests(int tickMs, int graceSeconds, double stallSeconds);

signals:
    void liveChanged();

private:
    QObject* teamObject() const;
    void watchdogTick(const QDateTime& now);
    void startMeter();         // 主线程
    void updateMeter();        // 主线程
    void finishBackgroundParentIfDone(const QUuid& childID, const QUuid& parentKey, const QUuid& conversationID);
    /// 三种派法（§2.3）
    ToolOutput runChildHere(const AgentProfile& profile, const TaskSpec& spec, int depth, const QUuid& eventID,
                            const QUuid& conversationID, const QUuid& parentKey, const std::optional<QDateTime>& deadline);
    void launchBackground(const AgentProfile& profile, const TaskSpec& spec, int depth, const QUuid& eventID,
                          const QUuid& conversationID, const QUuid& parentKey, const std::optional<QDateTime>& deadline);
    QString waitForChildren(ToolSink& sink, std::optional<double> seconds);
    static QString backgroundStarted(const QList<std::pair<QString, QUuid>>& agents);
    static bool flag(const QJsonValue& value);
    static QString stoppedNotice(const QString& who, const QString& text);
    static EngineMessage agentMessage(const QString& text, const QStringList& attachments, bool supportsVision);

    QTimer* m_watchdog = nullptr;
    QTimer* m_meter = nullptr;
    int m_liveAgents = 0;
    double m_liveSpeed = 0;
    std::optional<std::pair<QString, int>> m_mainLastCall;   // 回合线程
    mutable QMutex m_liveChunksMutex;
    QList<AgentTeamState::Sample> m_liveChunks;              // 主 Agent 最近 3 秒的 chunk（回合线程写、主线程读）
};
```

**给主循环（AgentRunner 主体，另一位分析负责）的钩子**——它必须在这些位置调用：

| 位置 | 调用（Swift 出处） |
|---|---|
| `configure`（主 Agent） | `tools->guidance(AgentTeamTools::Role::Main)`；`engine.tools = tools->mainAgentSpecs()`；`refreshTools` 也用 mainAgentSpecs（`AgentRunner.swift:323, 342, 1171`） |
| `send()` | `startMeter()`（:576）；回合开始清 `m_mainLastCall`（:619） |
| 主 Agent 的 interjection（`takeInterjection`） | `team->takeNotices(mainKey)` → 每条一个 `ChatMessage(User, noticeText(n))`，`fromAgent = n.agentName`，放在用户补充前面（:690-700） |
| `drainBackgroundAgents` | `team->hasNotices(mainKey)`、`team->backgroundChildren(mainKey, conv)`、`team->waitForNotice(mainKey)`（:1195-1210） |
| `submit()` 这段对话正在生成 | `team->wake(mainKey)`（:669） |
| `stop()` | 设 `isStopping` 后 `cancelTeam()`（:927） |
| `finish()` | `team->reset(conversationID)`（:1471）；markStopped 清子卡片的 queued |
| `dispatch()` | 预检后 `noteRepeat` + 拦截（:1043-1053）；审批等待包 `waitingWithoutSlot(sink, waitingApproval(), …)`，出错当作未批准（:1083-1086）；执行前 `setActivity(Activity::tool(title))`（**PORT_V15_V19.md D2**：不是 `toolNamed`）+ `noteStep` + 守卫回 generating（:1104-1110）；`runsInBackground ? Running : Succeeded`（:1126-1127）；`catch SubAgentEnded（stoppedBy 有）` → cardText / Failed / stopped（:1129-1133）；取消 → 「已停止」/ stopped（:1134-1138）；回灌后追加打转 / 步数提醒（:1148-1153） |
| `toolResultText` | 被停的子 Agent 两支、`.running` 当成功（§1.1） |
| `noteChunk` / `beginPass`（回合线程） | 维护 `m_liveChunks`（§2.7） |
| `updateEvent` / `messageTask` | 在这一轮的**全部**助手消息里从新到旧找事件（`turnMessageIDs`，:1279-1290）；SubRun 的节流刷新也走它 |
| `isStopping()` | 原子量，任意线程可读（本文多处读） |

### 3.3 `SubRun`（`SubAgentRunner.h`）

```cpp
class AgentRunner::SubRun : public ToolSink, public AgentEditMailbox {
public:
    SubRun(AgentRunner* runner, const QUuid& conversationID, int depth, const QUuid& eventID, const AgentProfile& profile, bool assumeThinking);
    // …现有：runner、eventID、agentProfile、passBuffer、blocks()/setBlocks()/blocksForUpdate()、media()、renderPass、discardPass、publish、finalText、ToolSink…

    int steps() const { return m_steps; }                     // consumeStep 计数（步数提醒用）
    std::optional<std::pair<QString, int>> lastCall;          // noteRepeat：签名、连续次数（只在自己的线程用）
    bool holdsLocalSlot = false;                              // 正占着本地名额（自己的线程）
    bool isLocal = false;
    const std::optional<AgentUsage>& usage() const { return m_usage; }
    void startUsage(const AgentUsage& usage);                 // 写进卡片（updateEvent，存档那份）
    void completePass(const EngineStats& stats);              // team->endPass + 累计 + 写进卡片
    /// 给引擎的 EngineEventHandler：Chunk → receive；Done → completePass；Progress → noteOutput + noteTokens（Qt 版新增）
    bool consume(const EngineEvent& event);
    void receive(const QString& chunk);

    /// 别的线程改这个子 Agent 本地块副本里的事件（下级结束时写状态）：进信箱，自己的线程下次发布 / 取快照 / 读块前应用
    void postEdit(const QUuid& eventID, std::function<void(ToolEvent&)> body) override;   // 任意线程
    void applyPendingEdits();                                                                // 自己的线程
    bool isOwnerThread() const;

    static QList<MessageBlock> merge(const QList<MessageBlock>& local, const QList<MessageBlock>& stored);   // §1.3.6 + 终态不回退

private:
    std::optional<AgentUsage> m_usage;
    QThread* m_thread;                                        // 构造时的线程
    QMutex m_editsMutex;
    QList<std::pair<QUuid, std::function<void(ToolEvent&)>>> m_pendingEdits;
};
```

### 3.4 `SubAgentScheduler`（`SubAgentScheduler.h`，只管本地）

```cpp
class SubAgentScheduler {
public:
    static constexpr quint64 headroom = quint64(1500) << 20;
    static constexpr quint64 expectedTokens = 12000;
    static constexpr quint64 activationOverhead = quint64(600) << 20;
    /// 等完下级 / 确认、要接着跑的子 Agent 的排队优先级加成（== Swift resumeBoost）
    static constexpr int resumeBoost = 10000;
    static constexpr int poolFullWindowMs = 3000;

    std::atomic<quint64> kvBytesPerToken { quint64(40) << 10 };
    /// S：非主序列数（ModelManager 加载完成时 = slotCount − 1）
    std::atomic<int> localSlotLimit { 0 };
    quint64 localBudget() const;

    /// 新开的子 Agent 现在能不能直接跑（§2.5 规则 3）
    bool canStartLocal() const;
    /// 新开：拿名额 + 序列份额（running+1、alive+1）。返回 true 表示排过队；排队时取消抛 CancellationError（已轮到却不要了：退回并转给下一个）
    bool acquireLocal(int priority, const CancelToken* token = CancelToken::current());
    /// 等下级 / 确认：让出名额，序列份额留着（running−1）并放行队首
    void yieldLocal();
    /// 等完：拿回名额（running+1，不看序列份额）。返回 true 表示排过队；取消抛 CancellationError
    bool resumeLocal(int priority, const CancelToken* token = CancelToken::current());
    /// 子 Agent 结束：还序列份额（alive−1），holdingSlot 时一并还名额；alive==0 且没人排队时池满降档恢复
    void releaseLocal(bool holdingSlot);
    /// KV 池满：降档（3 秒窗口内只降一次）、让出名额；能马上拿回就拿回（不唤醒别人），否则按 priority 排队。返回 true 表示排过队
    bool retryLocalAfterPoolFull(int priority, const CancelToken* token = CancelToken::current());
    int localPoolLimit() const;
    void resetLocalPoolLimit();
    QString diagnostics() const;   // §2.5 规则 8

private:
    struct Waiter { quint64 id; int priority; bool resuming; PromisePtr<bool> promise; };
    bool canAdmitLocked(bool resuming) const;
    void pumpLocked(QList<PromisePtr<bool>>& admitted);   // 取队首判定，放不进就停；解锁后再 resolve
    mutable std::mutex m_mutex;
    int m_runningLocal = 0;
    int m_aliveLocal = 0;
    int m_poolLimit = 0;
    qint64 m_poolFullAt = 0;
    QList<Waiter> m_waiters;
    quint64 m_sequence = 0;
};
```
`waitingWithoutSlot` 的 Qt 实现（模板，放 `AgentRunner.h` 末尾）——**已由 PORT_V15_V19.md §3.3（D5）取代**：改成 `std::function<void()>` 版、实现放 `AgentTeamControl.cpp`，
逻辑与下面相同（body 的结果由调用方按引用捕获）。另：调度器再加 `bool waitForLocalEnd(int timeoutMs, const CancelToken* = CancelToken::current())`（core 的主 Agent 池满重发用，PORT §3.6.5）；
放行判定与优先级以 PORT §3.6.2 为准（与 §2.5 一致）。
```cpp
template <class F>
auto AgentRunner::waitingWithoutSlot(ToolSink& sink, const AgentTeamState::Activity& waiting, F&& body) -> std::invoke_result_t<F&>
{
    using R = std::invoke_result_t<F&>;
    SubRun* sub = dynamic_cast<SubRun*>(&sink);
    if (sub) team->setActivity(sub->eventID, waiting);
    const auto back = qScopeGuard([&] { if (sub) team->setActivity(sub->eventID, AgentTeamState::Activity::generating()); });
    if (!sub || !sub->holdsLocalSlot) return body();
    sub->holdsLocalSlot = false;
    scheduler.yieldLocal();
    std::optional<R> result; std::exception_ptr error;           // R = void 时用 bool 占位（或要求 body 返回值）
    try { result.emplace(body()); } catch (...) { error = std::current_exception(); }
    if (!CancelToken::currentIsCancelled() && !isStopping()) {
        team->setActivity(sub->eventID, AgentTeamState::Activity::queued());
        updateEvent(sub->eventID, [](ToolEvent& e) { e.queued = true; });
        const auto unqueue = qScopeGuard([&] { updateEvent(sub->eventID, [](ToolEvent& e) { e.queued = false; }); });
        try {
            scheduler.resumeLocal(SubAgentScheduler::resumeBoost + sub->depth());
            sub->holdsLocalSlot = true;
        } catch (...) {
            if (!error) throw;                                    // body 成功：抛排队的错；body 失败：抛 body 的错
        }
    }
    if (error) std::rethrow_exception(error);
    return std::move(*result);
}
```

### 3.5 `RequestGate` 与引擎增量

```cpp
// src/core/llm/engine/RequestGate.h —— 对应 Swift：LLM/Engine/RequestGate.swift
class RequestGate {
public:
    enum class Outcome { Ok, RateLimited, Failed };
    static constexpr int mainPriority = 1000;
    static constexpr int defaultInitial = 4;
    /// 按引擎 id 共用；第一次取时按 initial（nullopt → 4，至少 1）建，之后 initial 不再起作用
    static std::shared_ptr<RequestGate> gate(const QString& engineID, std::optional<int> initial);
    /// 拿一个名额；要排队时先 onQueue(true)，排到 / 取消时 onQueue(false)；排队时 token 取消抛 CancellationError
    /// （已经被放行却不要了：in-flight 退回并 pump）。onQueue 在调用线程上同步调用
    void acquire(int priority, const std::function<void(bool)>& onQueue, const CancelToken* token = CancelToken::current());
    void release(Outcome outcome);
    struct Snapshot { int limit = 0; int inFlight = 0; int waiting = 0; };
    Snapshot snapshot() const;
    static bool isRetryableRateLimit(const QString& body);

private:
    RequestGate(QString engineID, int initial);
    int allowedLocked() const { return std::max(1, int(m_limit)); }
    void pumpLocked(QList<PromisePtr<bool>>& admitted);
    struct Waiter { quint64 id; int priority; PromisePtr<bool> promise; };
    QString m_engineID;
    mutable std::mutex m_mutex;
    double m_limit;
    int m_inFlight = 0;
    QList<Waiter> m_waiters;
    quint64 m_sequence = 0;
    std::optional<std::chrono::steady_clock::time_point> m_lastCut;
};
// 日志：Q_LOGGING_CATEGORY(lcRateLimit, "com.example.Friday.ratelimit")，qCInfo，文字同 §1.3.9
```

```cpp
// ChatEngine.h
/// 一段生成的工具调用全部执行完、发起下一段之前调用一次（引擎所在的工作线程）：返回补充的消息，接在工具结果后面。
/// 主 Agent（用户的工作中补充 + 后台子 Agent 的汇报）和子 Agent（用户 / 父级追加的指示 + 它的后台下级的汇报）都设
std::function<QList<EngineMessage>()> interjection;

// OpenAIChatEngine.h
int requestPriority = RequestGate::mainPriority;              // 子 Agent = 层数
std::function<void(bool waiting)> onQueued;                    // 闸门前开始 / 结束排队（引擎线程）
static constexpr int rateLimitGiveUpSeconds = 600;
/// 第 rateLimits 次（1 起）可重试的 429 之后等多久：Retry-After 能按数字解析就用它（负数按 0），否则 min(30, 2^min(rateLimits, 5)) + jitterMs/1000
static double rateLimitDelaySeconds(int rateLimits, const QByteArray& retryAfter, int jitterMs);
protected:
virtual std::shared_ptr<RequestGate> requestGate() const;     // 缺省 RequestGate::gate(m_config.id, m_config.maxConcurrency)；LocalLlamaEngine 返回空
virtual int rateLimitJitterMs() const;                          // 缺省 QRandomGenerator 0–999；测试覆盖成 0
```
`OpenAIChatEngine::run`：工具结果全部追加之后 `if (interjection) { CancelToken::checkCurrent(); for (m : interjection()) m_history.append(m.role == User ? userMessage(m.content, m.images, m.videos, m_config.supportsVision, false) : convert(m, …)); }`。
`OpenAIChatEngine::transport`（Qt 伪代码，`onLine` / `send` / `errorText` 同现有）：
```cpp
const std::shared_ptr<RequestGate> gate = requestGate();
double throttled = 0; int rateLimits = 0; int busyRetries = 0;
while (true) {
    if (gate) gate->acquire(requestPriority, onQueued);
    failure = nullptr; handlerStopped = false;
    HttpResponse response;
    try { response = send(); } catch (...) { if (gate) gate->release(RequestGate::Outcome::Failed); throw; }
    const auto fail = [&] { if (gate) gate->release(RequestGate::Outcome::Failed); };
    if (failure) { fail(); std::rethrow_exception(failure); }
    if (handlerStopped) { fail(); return; }
    const int status = response.status;
    if (status <= 0) { fail(); throw FridayError(response.describe()); }
    if (status >= 200 && status < 300) {
        if (!response.errorString.isEmpty()) { fail(); throw FridayError(response.errorString); }
        if (gate) gate->release(RequestGate::Outcome::Ok);
        return;
    }
    const QString text = errorText(response.body);
    const QByteArray retryAfter = response.header("Retry-After");
    if (status == 429 && gate && RequestGate::isRetryableRateLimit(text)) {
        gate->release(RequestGate::Outcome::RateLimited);
        rateLimits += 1;
        const double wait = rateLimitDelaySeconds(rateLimits, retryAfter, rateLimitJitterMs());
        throttled += wait;
        if (throttled > rateLimitGiveUpSeconds)
            throw CapabilityError::http(status, QStringLiteral("持续被限流超过 10 分钟，放弃这次请求。") + text);
        backoff(wait);
        continue;
    }
    fail();
    if ((status == 502 || status == 503 || status == 529 || (!gate && status == 429)) && busyRetries < 3) {
        busyRetries += 1;
        backoff(retryDelaySeconds(busyRetries, retryAfter));   // Retry-After ?? 2^n
        continue;
    }
    if (const auto overflow = ContextOverflowError::fromResponse(status, response.body)) throw *overflow;
    throwHTTPError(status, response.body, text);
}
```

### 3.6 其余增量

> 其中 `ToolOutput`、`QueuedInput`、`ChatModels`（AgentUsage、ToolEvent.stopped / usage、ChatMessage.fromAgent）、`AppSettings.subAgentTurnLimit`、`SubAgentEnded`
> **已由 PORT_V15_V19.md §3.1、§3.5 定稿并由骨架实现**（QueuedInput::toVariant 另有 `attachmentNames`；AgentUsage::detail 带 locale 参数）；
> `AgentTeamTools` 的新名字与常量、`ToolRegistry::mainAgentSpecs / guidance(Role)` 由骨架加（PORT §3.8），实现归 team。

```cpp
// AgentProfiles.h
class AgentTeamTools {
public:
    static QString delegateName();        // delegate_task
    static QString parallelName();        // run_parallel_tasks
    static QString readLogName();         // read_agent_log
    static QString messageAgentName();    // message_agent
    static QString stopAgentName();       // stop_agent
    static QString waitAgentsName();      // wait_agents
    static QString reportToParentName();  // report_to_parent（只给子 Agent）
    static constexpr int deepLevel = 4;
    static constexpr double shrinkThreshold = 0.8;
    /// Dice 系数：toLower 后按码点过滤 QChar::isSpace / Punctuation_* / Symbol_*，相邻两码点为一组
    static double similarity(const QString& a, const QString& b);
    static bool isTeamTool(const QString& name);   // delegate / parallel
    enum class Role { Main, Sub };
    /// 7 个，顺序见 §1.3.3
    static QList<ToolDescriptor> descriptors(const QList<EngineConfig>& engines, bool localAllowed);
    static QString splitGuidance();
    static QString guidance();
    static QString subGuidance();
    static QString subAgentPreamble(const QString& name, int depth);
};

// ToolRegistry.h
QList<ToolSpec> mainAgentSpecs() const;                        // 任意线程：activeDescriptors 去掉 report_to_parent
QString guidance(AgentTeamTools::Role team) const;             // 替换 guidance(bool includeTeam)
// preflight：report_to_parent 且 depth == 0 → 「report_to_parent 只给子 Agent 用；你是主 Agent，直接回答用户即可。」
// 诊断扩展文字：§1.8

// ToolTypes.h
struct ToolOutput {
    QString text; QList<MediaAttachment> media;
    /// 后台派发的子 Agent：工具已经返回，但它们还在跑（卡片保持运行中，完成后再定稿）
    bool runsInBackground = false;
    explicit ToolOutput(QString text, QList<MediaAttachment> media = {}, bool runsInBackground = false);
    QString modelText() const;
};

// QueuedInput.h（和主循环共用）
struct QueuedInput {
    QUuid id = QUuid::createUuid();
    QString text;
    QStringList attachments;                       // 本地文件路径
    QDateTime createdAt = QDateTime::currentDateTimeUtc();
    /// 谁发的：nullopt = 用户；否则是父级名字（「主 Agent」「父 Agent「X」」「Friday」）
    std::optional<QString> from;
    QVariantMap toVariant() const;                 // {id, text, attachments, from, createdAt}
};

// ChatModels.h
struct AgentUsage {
    QString model;
    std::optional<QString> service;
    QString thinking;
    double temperature = 0;
    int generatedTokens = 0;
    int promptTokens = 0;
    double generateTime = 0;
    double tokensPerSecond() const;
    /// 「深度思考：{thinking}\n温度：{0–2 位小数，去掉末尾 0}[\n服务：{service}]」
    QString detail() const;
    QJsonObject toJson() const;                    // 六个必写键 + service 可选
    static AgentUsage fromJson(const QJsonObject& object);
    QVariantMap toVariant() const;                 // 键同 JSON + tokensPerSecond + detail
    bool operator==(const AgentUsage&) const = default;
};
// ToolEvent 加：std::optional<bool> stopped; std::optional<AgentUsage> usage;（toJson / fromJson / toVariant（"stopped"、"usage"）/ operator== 都要带上）
// ChatMessage 加：std::optional<QString> fromAgent;（+ 主循环的 stoppedByUser、interjected）
// MessageBlock::markStopped（主循环）：清 queued、设 stopped

// AppSettings.h
Q_PROPERTY(int subAgentTurnLimit READ subAgentTurnLimit WRITE setSubAgentTurnLimit NOTIFY subAgentTurnLimitChanged)
// 默认 100；读入 std::max(0, intValue("subAgentTurnLimit", 100))；工作线程经 runOnMainSync 读

// AgentLiveness.h
class SubAgentEnded : public FridayError {   // message() == cardText()
public:
    SubAgentEnded(std::optional<QString> stoppedBy, QString message, std::optional<QString> partial);
    const std::optional<QString>& stoppedBy() const;
    const QString& detail() const;
    const std::optional<QString>& partial() const;
    QString cardText() const;
    static QString advice(const QString& cardText);
};

// TurnCheck.cpp AgentLog::render：工具块 name == "agent_message" → 一行「【{title ?? "补充"}】{clip(arguments, 800)}」（不再写【调用】/【结果】）
```

### 3.7 Agent 树数据（`src/core/agent/AgentTree.h`，纯函数）

> **已由 PORT_V15_V19.md §3.9 取代**（骨架实现全部纯函数，之后归 team）：`compact / speedText / menuBarSpeedText` 不在 AgentTree 里，改为 `Format::agentTokens / agentSpeed / meterSpeed`；
> 新增 `AgentTree::statusKind(event)` 与 `AgentTree::chips(transcript)`（MessageListModel 与 AgentTreeModel 共用）。下面的 `AgentTreeModel` 声明**已由 ui.md §3.9 + PORT §3.11.8 取代**（归 tree-tray）。

```cpp
struct AgentNode {
    ToolEvent event;
    int depth = 1;            // 主 Agent 直接派的是 1；并行分组不算一层
    bool parallel = false;
    QList<AgentNode> children;
    QUuid id() const;
    QString name() const;     // agentName ?? "子 Agent"
    bool isRunning() const;   // status == Running
};
struct AgentTreeTurn { QUuid id; QString request; QList<AgentNode> nodes; };
struct AgentTreeStats { int running = 0, done = 0, stopped = 0, failed = 0, deepest = 0; int total() const; };
struct UsageSummary { int generated = 0, prompt = 0; double speed = 0; int agents = 0, running = 0; UsageSummary operator+(const UsageSummary&) const; };
struct AgentTreeRow { AgentNode node; int level = 0; QList<bool> guides; bool isLast = false; };

namespace AgentTree {
QList<AgentNode> build(const QList<MessageBlock>& blocks, int depth = 1, bool parallel = false);
QList<AgentTreeTurn> turns(const QList<ChatMessage>& messages);           // 新的在前
QList<AgentNode> all(const QList<AgentNode>& nodes);
QList<AgentNode> path(const QUuid& id, const QList<AgentNode>& nodes);
AgentTreeStats stats(const QList<AgentNode>& nodes);
QList<AgentTreeRow> flatten(const QList<AgentNode>& nodes, const QSet<QUuid>& collapsed, int level = 0, const QList<bool>& guides = {});
UsageSummary usage(const AgentNode& node, const AgentTeamState& team, const QDateTime& now);
/// (主 Agent, 全部子 Agent)；主 Agent 的实时速度由调用方给（AgentRunner::mainLiveSpeed），generating = 这段对话正在生成
std::pair<UsageSummary, UsageSummary> conversationUsage(const QList<ChatMessage>& messages, const AgentTeamState& team,
                                                        double mainSpeed, bool generating, const QDateTime& now);
QString compact(qint64 n);              // 950 / 12.3k / 1.3M
QString speedText(double speed);        // — tok/s / %.1f / %.0f tok/s
QString menuBarSpeedText(double speed); // == MenuBarLabel.speedText
QString conclusion(const QString& result);
constexpr int autoOpenDepth = 3;
}
```

`src/app/viewmodels/AgentTreeModel.h`（给界面分析；行 = 当前对话按轮次摊平后的可见行）：
```cpp
class AgentTreeModel : public QAbstractListModel {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(QString conversationID READ conversationID WRITE setConversationID NOTIFY conversationIDChanged)
    // 概览（全部轮次）与工具栏按钮（最新一轮）
    Q_PROPERTY(int running READ running NOTIFY summaryChanged)
    Q_PROPERTY(int done READ done NOTIFY summaryChanged)
    Q_PROPERTY(int stopped READ stopped NOTIFY summaryChanged)
    Q_PROPERTY(int failed READ failed NOTIFY summaryChanged)
    Q_PROPERTY(int deepest READ deepest NOTIFY summaryChanged)
    Q_PROPERTY(int total READ total NOTIFY summaryChanged)
    Q_PROPERTY(int latestRunning READ latestRunning NOTIFY summaryChanged)
    Q_PROPERTY(int latestDeepest READ latestDeepest NOTIFY summaryChanged)
    /// conversationUsage：{speed, generated, prompt, running, main:{…}, sub:{…, agents}}；生成中每秒刷新
    Q_PROPERTY(QVariantMap usage READ usage NOTIFY usageChanged)
public:
    enum Role {
        KindRole = Qt::UserRole + 1,   // "turn" | "agent"
        TurnIdRole, TurnNumberRole, IsLatestTurnRole, TurnRequestRole, TurnExpandedRole,
        AgentIdRole, ParentIdRole,     // 父 Agent 的事件 id；第 1 层为空（主 Agent）
        DepthRole, LevelRole, GuidesRole /*QVariantList<bool>*/, IsLastRole, ChildCountRole, ExpandedRole,
        NameRole, ParallelRole,
        StatusRole,                    // running | queued | awaitingApproval | succeeded | stopped | failed | denied（AgentStatusIcon 的分支）
        IsRunningRole, HasPendingApprovalRole,
        ModelRole, ServiceRole, ThinkingRole, TemperatureRole, ModelDetailRole,   // 来自 usage，没有为空
        SpeedRole,                     // 运行中实时，结束后平均（AgentTree::usage）
        GeneratedRole, PromptRole,
        StatusLineRole,                // 运行中：agentStatusText(compact)；否则 conclusion(result)
        TaskRole, ResultRole, ToolCallsRole,
        StartedAtRole,                 // 运行时才有（team->started），结束后为空——Swift 不持久化开始 / 结束时间
    };
    Q_INVOKABLE void toggle(const QString& agentID);
    Q_INVOKABLE void toggleTurn(const QString& turnID);
    Q_INVOKABLE QVariantList path(const QString& agentID) const;     // [{id, name}]，第 1 层到它
    Q_INVOKABLE QVariantMap details(const QString& agentID) const;   // ToolEvent.toVariant + depth + children + 运行记录块
};
```
刷新：`ConversationStore` 这段对话的消息变化与 `team->changed` 时重建；生成中 1 秒定时器只对运行中的行发 `dataChanged`（速度 / 生成 / 状态行），并发 `usageChanged`。
托盘：~~`AgentRunner::liveChanged` → 界面按 `AgentTree::menuBarSpeedText` 画~~ **已由 PORT_V15_V19.md §3.11.6 取代**：`AgentRunner::liveMeterChanged` → tree-tray 的 FridayApp
用 `Format::meterSpeed` 写 tooltip 三行与菜单行，图标不画字。

---

## 4. 测试与调试钩子

### 4.1 SelfTests.swift → QtTest（新文件 `tests/tst_team.cpp`，类 `TestAgentTeam`、`TestAgentTree`、`TestRequestGate`）

| Swift（`Agent/SelfTests.swift`） | Qt 测试 | 断言（逐条移植） |
|---|---|---|
| `requestGate()`（:60-112） | `TestRequestGate::retryable` | 1302「您当前使用该API的并发数过高」可重试；1311「当前订阅套餐暂未开放该模型」不可；`{"code":1113,"message":"余额不足"}` 不可；`insufficient_quota` 不可；「Rate limit reached for requests」可 |
| 同上 | `TestRequestGate::priorityGrowthCut` | `gate(id, 2)` 与 `gate(id, 9)` 是同一个；占满 2 个后先排低（priority 1）后排高（5），`waiting == 2`；release(Ok) 先放高、再 release 放低；再各一个排队 + release(Ok) 后 `limit == 3`；release(RateLimited) 后 `limit == 1` |
| 同上 | `TestRequestGate::cancelWhileQueued` | 排队时取消抛 CancellationError；之后 `waiting == 0` 且 inFlight 不变；（Qt 加）onQueue 收到 true 再 false |
| `localScheduler()`（:114-125） | `TestSubAgentScheduling::firstAlwaysAdmitted` | 第一个 `acquireLocal(1)` 不排队，且之后 `canStartLocal() == (available >= budget + headroom)`；`releaseLocal(true)` 后一定能放行；`resumeLocal(resumeBoost + 1)` 不排队 |
| （Qt 新增） | `TestSubAgentScheduling::priorityOrder` | `localSlotLimit = 1`：占 1 个后依次排 depth 1、depth 3、resume(10001)；release 时顺序 resume → depth 3 → depth 1；同优先级先来先走 |
| （Qt 新增） | `TestSubAgentScheduling::parkedHoldsSequence` | S = 2：两个新开的跑；一个 `yieldLocal` 后再新开的仍要排（alive 2 ≥ 2）；让出的 `resumeLocal` 不排队；全部让出（running 0）时新开的被放行 |
| 现有 `localPoolFull` | 改写 | 去掉嵌套借名额分支（`retryLocalAfterPoolFull(false)`）；参数改成 priority；其余断言（上限 2、3 秒窗口只降一次、排队 1 个、取消后不变负、全部结束恢复）保留 |
| 现有 `onlineQueue` / `onlineCancelHandsOver` | 删除 | 在线排队移到 RequestGate |
| `teamState()`（:382-413） | `TestAgentTeam::noticesAndMessages` | 汇报以「[子 Agent「研究员」（id：{shortID}）中途汇报」开头、以正文结尾；`noticeTitle` → 中途汇报 / 完成；停止通知含「不要重新派发」且标题「已停止」；`agentMessageNote(nil)` 含「用户在你工作时补充」；`agentMessageNote("主 Agent")` 含「主 Agent在你工作时追加」；不在运行的 id `sendToAgent` / `stopAgent` 都 false；收件箱先进先出、取走后为空；`AgentLog::render` 含「【用户补充】改成只查官方文档」 |
| `teamDepth()`（:415-482） | `TestAgentTeam::depthRules` | `similarity("调查 Swift 6.2 的新特性，写一份总结", "调查 Swift 6.2 的新特性，并写一份总结给我") >= 0.8`；「用 delegate_task 再派一个下级子 Agent 去执行 shell 命令 echo hi，拿到结果后报告」vs「执行 shell 命令 echo hi，把命令的完整标准输出报告回来」< 0.8；`("abc","abc") == 1`、`("","abc") == 0`；三层 run（组长 / 组员 / 校对）的上级链含「主 Agent派给「组长」：把仓库里的文档」「「组长」派给「组员」：检查 docs/」「第 3 层」，第 1 层为 nil；组员只派一个「检查 docs/ 目录下的全部 Markdown 文件」→ 拒绝，「校对 docs/install.md 里的错别字」→ 放行，两项 parallel → 放行；上限 10 且已派 9 → parallel 2 个拒绝；上限 0 → 放行；merge：并行卡片和子卡片状态以本地为准、子卡片运行记录和排队标记以存档为准 |
| `liveness()`（:484-544） | `TestAgentTeam::liveness` | `duration(5/125/3700)` = 「5 秒」「2 分 5 秒」「1 小时 1 分」；看门狗停的 cardText 以「被看门狗」开头、含「做到一半」，建议含「换个做法」；用户停的建议含「不要重新派发」；期限停的含「收尾」；失败的以「失败了：HTTP 500」开头、建议含「失败原因」；看门狗停止通知以「[子 Agent「研究员」（id：…）被看门狗」开头、含「做到一半」「换个做法」、标题「已停止」；被停的 delegate 卡片 `toolResultText` 含「做到一半」「换个做法」；`childDeadline(100, 父级剩 30 秒)` ∈ (25, 30]、`(10)` ∈ (5, 10]、主 Agent 没期限 → nil；状态含「在执行「执行命令」」「第 1 步」「已运行」，有期限的含「期限还剩」；`noteRepeat` 同样调用三次 1/2/3、换参数 1、wait_agents 0；半成品含「已经查到两个来源」「「审稿人」：结论：没问题」「执行命令 ×2」 |
| `agentTree()`（:546-595） | `TestAgentTree::buildTurnsFlatten` | 总负责人 →（被拒的 delegate 不算）→ 并行（甲组长 → 计算员 → 核对员；乙组长）：names 顺序、depth `[1,2,3,4,2]`、parallel 标记、stats running 4 / done 1 / deepest 4、path 到核对员；turns：工作中补充不开新轮、新的在前、`turns[1]` 节点 甲、乙；flatten level `[0,1,2,3,1]`、guides `[false]`、`[false,true]`、isLast；收起计算员后 4 行 |
| `agentUsage()`（:597-639） | `TestAgentTree::usage` | `AgentUsage(glm-5.3-flash, 智谱 GLM, 中, 0.6, 300, 9000, 6)`：tokensPerSecond 50、detail「深度思考：中\n温度：0.6\n服务：智谱 GLM」；windowSpeed 样例 20、5 秒后 0；`estimateTokens("<think>", false) == 0`、`("你好", true) == 1`；会话合计 main 250 / 4000、sub 140 / 1500、agents 2、speed 0；compact 950 / 12.3k / 1.3M；merge 后保留存档的 usage；（Qt 加）AgentUsage JSON 往返、ToolEvent 带 stopped / usage 的往返、缺 service 不写键 |
| :44 MenuBarLabel | **已由 PORT_V15_V19.md §7.1 取代**：tree-tray 的 `TestTreeTray::formats`（`Format::meterSpeed`） | `[0, 8.46, 179.4, 1234, 3.4e6, 1.1e12]` → 「0 t/s」「8.5 t/s」「179 t/s」「1.2 Kt/s」「3.4 Mt/s」「1.1 Tt/s」 |
| `stopState()` / `queueState()` | 主循环的测试 | 本文只要求其中「并行里的子 Agent（运行中、排队中）都停止、queued 清空」「子 Agent 里还在跑的命令也停止」两条在 Qt 版保留 |

### 4.2 端到端（`tests/tst_runner.cpp`，在线引擎指向 `FakeChatServer`）

1. `backgroundDelegate`：delegate `background:true` → 工具结果 == `backgroundStarted`；卡片在子 Agent 结束前是 Running；结束后 Succeeded、`result` = 子 Agent 输出；主 Agent 下一段请求的最后一条 user 消息 ==「[子 Agent「X」（id：{shortID}）完成了]\n…」，存档里那条 `fromAgent == "X"`；这一轮等它结束才收尾。
2. `messageAgentAndReport`：后台子 Agent 被 `hold`；主 Agent 调 `message_agent`（id 前 8 位）→「已排队：子 Agent「X」做完当前这一步就会看到。」；子 Agent 下一段请求含「（主 Agent在你工作时追加了下面的指示：和你的任务一起考虑，有冲突以它为准。）\n…」；它的运行记录有 `agent_message` 卡片，标题「主 Agent追加的指示」；子 Agent 调 `report_to_parent` → 主 Agent 收到「…中途汇报，它还在继续]\n…」。前缀不足 4 位 → 「没有 id 以「…」开头…」。
3. `stopAgentByParent`：`stop_agent` → 卡片 Failed + stopped、result 以「被主 Agent停止了，没做完。」开头；通知以「[子 Agent「X」（id：…）被主 Agent停止了，没做完。]」开头、以 advice 的「不要重新派发…」结尾。
4. `userStopsOneSubAgent`：并行两个、其一被 hold；`m_runner->stopAgent(id)` → 它的 section ==「### 任务 1（甲）：被用户停止了，没做完。」+（有半成品时）+「\n不要重新派发同样的任务；…」；另一个照常；整轮不停。
5. `waitAgentsTimeout`：`wait_agents {"seconds":1}` 时子 Agent 被 hold → 结果以「等了 1 秒，还没有新的汇报或结果。还在跑的：\n- 「X」（id：…）：」开头、以「…或 stop_agent 停掉（会交回做到一半的结果）。」结尾。
6. `unlimitedDepthAndShrink`：三层 delegate 链跑通（第 2、3 层请求的任务末尾带「[你在整个任务里的位置…」）；某层原样转派 → 卡片 Failed、result 是相似度那句；该层系统提示在第 4 层以「你已经在第 4 层了」结尾。
7. `turnLimit`：`setSubAgentTurnLimit(2)` 后 parallel 3 个 → Failed「这一轮最多派 2 个子 Agent（设置 › 子 Agent），还能派 2 个，这次要派 3 个。…」；被拒的不进 `AgentTree::build`。
8. `deadlineStopsChild`：`setLivenessTimingForTests(100, 1, 300)`（宽限 1 秒）；`deadline_seconds:1` 的后台子 Agent 被 hold → 到点后它的队列（`team->queue(id)`）里出现 from「Friday」的
   「派发时给的时间到了：…1 秒内不交就会被停掉。」（请求被扣着，送不到它手上），再过 1 秒被停：卡片 / 通知「被期限（到点后又等了 1 秒）停止了，没做完。」，建议以「时间到了：」开头；
   日志「子 Agent「X」过了期限还没交付，已停掉」。另测一个不被扣的：提醒在它下一步时作为「Friday追加的指示」卡片进运行记录。父级有期限时下级的期限被截短（`childDeadline`）。
9. `watchdogStopsStalled`：`FakeChatServer::subHangMs` + stall 1 秒 → 「被看门狗（1 秒没有任何输出）停止了，没做完。」，日志「子 Agent「X」1 秒没有任何输出，看门狗停掉了它」；在等下级 / 排队的不被停。
10. `repeatGuard`：主 Agent 同样的 `run_shell` 连调 5 次 → 第 3 次回灌带「（同样的调用已经连续 3 次了…）」，第 5 次「调用被拒绝（未执行）：同样的调用已经连续 5 次了…」，日志「拦下重复调用 run_shell（连续 5 次）」；`get_current_time` 连调不计。
11. `rateLimitAdaptive`：`maxConcurrent = 1`、429 带 `Retry-After: 0`，parallel 3 个在线子 Agent 全部成功；`RequestGate::gate(id)->snapshot().limit == 1`；卡片在排队时 `queued == true`。
12. `quota429FailsFast`：`quota429 = true` → 子 Agent 失败「HTTP 429：{"error":{"code":"1311",…}}」，每个只发了 1 个请求。
13. `usageOnCards`：子卡片 `usage` = {model: fake-model, service: 测试引擎, thinking: 「关闭」或「不支持」, temperature}，`generatedTokens` 来自 usage 块。
14. `nestedOnlineSingleConcurrency`（替换 `nestedDelegateSharesLease`）：起步并发 1 的引擎上两层 delegate 不死锁；结束时闸门 `inFlight == 0`。
15. 现有 `teamToolValidation` 去掉「已经是第 2 层…」那条；`parallelSubAgents` 的子 Agent 系统提示断言随新 preamble / subGuidance 更新；`subAgentBackground` 里 `guidance()` 断言不变（主 Agent 仍包含它）。

### 4.3 进程内（`tests/tst_inproc.cpp`，`FakeLlama`）

- `nestedLocalParking`（localSlotLimit 2）：主 Agent → delegate A（序列 1）→ A delegate B：B 落在序列 2，A 的序列不还；B 运行时诊断「本地子 Agent 运行中 1 个…其中 1 个被在等下级 / 确认的子 Agent 占着」；结束后「运行中 0 个」，序列 1、2 都能再借。
- `localGuardWhenAllParked`（localSlotLimit 1）：A（序列 1）→ delegate B：B 由「没有正在跑的」放行，请求 `id_slot == -1`；整轮完成。
- 现有 `subAgentPoolFullRetriesInProcess` 保留（日志「让出名额后重试（第 1 次）」）。

### 4.4 `FakeChatServer` / `MockServer` 增补（== `Tools/Perf/mock_sse.py:9-11, 28-31, 127-149, 160-163`）

> 位置**已由 PORT_V15_V19.md §6.A 改为**：骨架把 `FakeChatServer` 抽到 `tests/FakeChatServer.h`、`MockServer` 抽到 `tests/MockServer.h`，两个都归 team；
> `tests/AgentTestSupport.h`（AgentRig 等）归 core。`mock_sse.py` 另加 Qt 截图要的开关 `nest / background / report / sub_approval`（PORT §5.2）。

`FakeChatServer`（tst_runner.cpp）加：
- `int maxConcurrent = 0`：在途（已接受、还没回完）的请求 ≥ 它 → 回 `429`，`Content-Type: application/json`，正文 `{"error":{"code":"1302","message":"您当前使用该API的并发数过高，请降低并发"}}`；
- `bool quota429 = false`：全部回 429 + `{"error":{"code":"1311","message":"当前订阅套餐暂未开放该模型"}}`；
- `int subHangMs = 0`：系统提示含「临时子 Agent」且还没有 tool 消息的请求，先挂这么久再回（不出字）；
- `int chunkIntervalMs = 0`：SSE 正文按 chunk 分批写（QTimer），让请求在时间上重叠，在途计数才有意义；
- `QByteArray retryAfter`（429 时带的 `Retry-After`，测试用 "0"）；统计 `peak`、`rejected`。
`MockServer`（tst_engines.cpp）：现有 `Reply.headers` 足够；新增测试 `rateLimitRetries`：`Retry-After: 300` 的可重试 429 连续三次 → 第三次抛「HTTP 429：持续被限流超过 10 分钟，放弃这次请求。…」、`delays == [300, 300]`、请求数 3；不可重试 429（1311）→ 1 个请求直接「HTTP 429：…」；
`rateLimitDelaySeconds(1…6, "", 0)` == 2, 4, 8, 16, 30, 30；`Retry-After: 5` → 5。
现有 `retryBackoff` 改：429 与 502/503/529 分开计数，回复序列 429(0) / 502 / 529 / 503 / 503 → 「HTTP 503：…」，delays `[0, 2, 4, 8]`，请求数 5。新增 `localEngineBypassesGate`：LocalLlamaEngine（子进程模式指向 MockServer）429 仍按旧规则退避 3 次。
手工性能 / 限流复现：`Friday/Tools/Perf/mock_sse.py` 是 OpenAI 兼容服务，Qt 版直接加一个指向 `http://127.0.0.1:<端口>/v1` 的自定义引擎 + `FRIDAY_AUTOSEND` 就能用（`mock_config.json` 的 `max_concurrent` / `quota_429` / `sub_hang`），不必移植脚本。

### 4.5 调试钩子（`agent/DebugHooks.cpp`，只在调试构建；`DebugHooks.swift:12-20, 112-213`）

| 环境变量 | 行为 | 输出（stdout，逐字） |
|---|---|---|
| `FRIDAY_AGENT_MESSAGE="<条件>\|<文字>"` | 条件满足后等到有运行中的子 Agent，`sendToAgent(它, 文字)` | `FRIDAY_AGENT_MESSAGE sent`；日志「FRIDAY_AGENT_MESSAGE：已发给子 Agent「{name}」」 |
| `FRIDAY_STOP_AGENT="<条件>[\|<名字>]"` | 同上，`stopAgent(它)`（按名字挑，不给名字取第一个） | `FRIDAY_STOP_AGENT stopped {name}` |
| `FRIDAY_TREE_FOCUS="<条件>\|<名字或「最深」>"` | `team->setTreeFocus(它)`；「最深」挑 depth 最大的运行中的 | `FRIDAY_TREE_FOCUS {name}` |
| `FRIDAY_STALL_LIMIT=<秒>` | 看门狗阈值（每次读） | — |
| `FRIDAY_LOCAL_SLOTS=<n>` | 本地子 Agent 同时最多 n 个（只读一次），叠加在 §2.5 规则上 | — |

`<条件>` 用和 `FRIDAY_AUTOSTOP / FRIDAY_QUEUE / FRIDAY_SWITCH` 共用的 `waitForCondition`（`approval` / `tool:<名>` / `after:<秒>` / `text:<秒>`，主循环那边移植）。

---

## 5. 风险与待定

1. **本地并发上限 3**（`ModelManager::maxExtraSlots`）对「层数不限」偏小：在等下级的子 Agent 占着序列，深树里新开的常常只能等或走 `-1`。要不要按内存放宽（更多序列、池不按序列数线性加）需要实测（RTX 5080 + Qwen3.5-9B 的 64k + 3 × 16384 是现有基准）。规则本身不依赖这个数。
   **已定（PORT_V15_V19.md §3.6.4）**：去掉固定的 3——保底 3、富余那一半在主对话到顶、份额到 16384 之后再加序列，技术上限 32；内存紧时结果不变，12 GiB 的基准仍是 64k + 3 × 16384。
2. **子进程模式 `-1` 可能挤掉主对话 0 号槽位**（§2.5 规则 7），主对话整段重新预填；进程内没有这个问题。过渡期接受，WP10 删子进程模式后消失。
   **已定（PORT_V15_V19.md §3.6.3 / Q3）**：子进程模式借不到槽位时用 `sharedSlot()`（显式的非主槽位），不再发 `-1`。
3. **线程池**：`Worker` 池上限 256。每轮上限设成 0（不限）时，大量并行 / 后台子 Agent 可能把池占满：父线程阻塞等子任务，而子任务排在池里拿不到线程 → 死锁。默认 100 时安全。可选加固：`checkDispatch` 在池快满时按「这一轮已经派满…」拒绝（Qt 版新增，需确认文案）。
   **已定（PORT_V15_V19.md §3.10、§3.6.6 / Q5）**：阻塞等待让出池名额（`releaseThread / reserveThread`），外加同时活着的子 Agent 安全上限 1000（文案见 PORT §3.6.6）。
4. **信箱 + 终态不回退**是 Qt 特有机制（Swift 靠主 actor 串行），要重点测：后台孙子结束时父级正阻塞、父级被停的同时下级结束、`markStopped` 之后迟到的发布。
5. **看门狗误停**：后端队列里等待（进程内「序列被占就排队」、llama-server 的 deferred 任务）和超长预填都表现为「生成中但没输出」，超过 5 分钟会被停。Swift 的长预填同样如此；Qt 把 Progress 算作输出（§2.8）只解决写大参数的情况。
6. **429 + `Retry-After: 0` 永远不放弃**：Swift 按请求的等待秒数累计 10 分钟，服务端一直回 0 秒时会紧密重试（并发已降到 1）。保持一致还是给个下限（如 1 秒）待定。
   **已定（PORT_V15_V19.md §4.2）**：照搬 Swift；需要时两边一起改。
7. **闸门起步并发只在第一次建闸门时生效**：设置里改「起步并发」要重启才生效（Swift 同样）。
8. **后台卡片的 result 会被覆盖**：模型当时看到的是「已在后台启动…」，结束后卡片 result 变成子 Agent 的输出；重建历史（折叠、停止后同步、重启）时这次调用的工具结果和当初不一致，本地 KV 前缀在这里失配（Swift 同样）。
9. **依赖主循环**：`isStopping()`（原子）、`ChatEngine::interjection` 的调用点、`updateEvent` / SubRun 节流刷新在整轮全部助手消息里找事件、`takeInterjection` 里的 `fromAgent` 汇报卡片、`drainBackgroundAgents`、`finish()` 的 `reset` 与 `markStopped`。缺任何一项，后台子 Agent 的卡片 / 通知会丢。
10. **`reservedToolNames`** 只保留 delegate / parallel（Swift 同样），新能力工具可能和 `message_agent` 等重名。要不要一并保留待定。
11. **托盘两行字**：QSystemTrayIcon 只能放图标和提示；Windows 上要把「{n} AI / {speed}」画进图标或放进提示文字，是界面取舍。
    **已定（PORT_V15_V19.md §3.11.6 / Q9）**：不画进图标；tooltip 三行 + 菜单行，归 tree-tray。
12. **开始 / 结束时间不持久化**：树的「已运行」只在运行中有；要持久化必须加新键（Swift 读时会忽略、写回时丢掉），不建议。
13. **similarity 的字符类**：Swift `CharacterSet.punctuationCharacters / symbols / whitespacesAndNewlines` 与 `QChar` 的 Unicode 类别基本一致，生僻字符上可能有个别差异；小写化用 `QString::toLower()`（全 Unicode）。
14. **`ChatEngine.swift` 的注释过时**（说子 Agent 不接收补充），按 v1.7 代码：子 Agent 也设 interjection。
15. **时钟**：期限、状态文字、速度窗口用墙上时间（== Swift 的 `Date`）；系统改时间会影响期限，与 Swift 一致，不另做处理。
16. **看图 / 起标题也借非主序列**：`alive` 只数子 Agent，它们借走序列时新开的子 Agent 会拿到 `-1`（与现在相同）。
