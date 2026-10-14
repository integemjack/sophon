# Swift v1.5–v1.9（+ d696a44、d621837）移植 Qt：总契约

> **v2.0 注**：v2.0 起 Qt 版不带本地推理（llama.cpp / stable-diffusion.cpp、本地模型与 KV 缓存、本地子 Agent 的名额调度等都已删除），
> 本文里与本地推理有关的部分不再适用；文中提到的 LOCAL_INFERENCE.md、BACKENDS.md 也已在 v2.0 删除。本文作为历史记录保留，不再更新。

> 提交位置：`qt/docs/PORT_V15_V19.md`；三份分析稿作为附录提交为 `qt/docs/port/core-loop.md`、`qt/docs/port/subagents.md`、`qt/docs/port/ui.md`。
> 基线：Swift `origin/main = e2f8f41`（merge-base `44116fb` 之后的 e71cf89 / 76534ff / e3aa578 / f1f6d58 / e2f8f41 五个合并），
> 外加两个更早、Qt 一直没移植的提交 `d696a44`、`d621837`。Qt 基线 = 骨架提交（§6）开始时 `v1.5` 分支的 HEAD。

> **实施方式（2026-10-02 起）**：用户要求不用多 Agent 并行，改为按 Swift 的版本顺序（v1.5 → v1.6 → v1.7 → v1.8 → v1.9，再补 d696a44、d621837）
> 直接移植、每版单独提交。§5 的分包与 §6 的骨架清单不再执行，只作为接口与文件归属的参考；接口、决定、测试与验收仍以本文为准。
> 期间远程 main 又合并了 Swift v1.10（主 Agent 只派活和验收、自我学习等），排在这批之后移植，本文不含。

## 0. 这份文档怎么用

- **优先级**：本文 > 附录。附录是行为规格（逐字文案、Swift 文件:行号、Qt 映射、测试向量），写得很细，但三份是分头写的，接口互相对不上；
  凡是本文写了的接口、归属、决定，以本文为准。附录里已作废的段落就地标了「**已由 PORT_V15_V19.md §x 取代**」。
- **对齐原则**：行为照抄 Swift（附录标「照搬」的怪癖也照抄）；Qt 只在外观（STYLE.md）和 §2.4 列出的地方有意不同。
  两个疑似 Swift bug 也照抄，列在 §8 等用户决定。
- **文案**：凡附录用「」或代码块给出的中文都是逐字的（全角标点、空格、换行）。本文不重抄，只给出处。
- **五个工作包**（§5）：`core`（主 Agent 循环）、`team`（子 Agent 系统）、`chat-ui`（对话界面）、`tree-tray`（Agent 树、工具栏合计、托盘）、
  `settings`（设置页与服务一级设置）。先由一个人做**骨架提交**（§6），然后五个包并行。
- 术语：「回合线程」= 主 Agent 一轮的 Worker 线程；「子 Agent 线程」= 同步 delegate 时就是调用方线程，并行 / 后台时是各自的 Worker 线程；
  「S」= 本地模型可给子 Agent 用的非主序列数（§3.6）。
- 各包的阅读顺序（都先读 §2、§4、§5 里自己那一节）：core = §3.1–§3.2、§3.6.5、§3.7 + core-loop.md；team = §3.3–§3.10 + subagents.md；
  chat-ui = §3.11.2–§3.11.5、§1.6 + ui.md §1.1–§1.4、§2.2–§2.5；tree-tray = §3.9、§3.11 + ui.md §1.4–§1.5、§2.6–§2.9、§4；
  settings = §3.12、§1.7 + ui.md §1.4「设置」、§2.8。骨架的执行者读 §3 与 §6。

---

## 1. 功能一览（按版本）

### 1.1 v1.5 停止状态（e71cf89 = a9a6d27）
`stop()` 只认第一次；`isStopping` 期间输入框停止键换成顺时针旋转的刷新图标并禁用，正在写的消息底栏显示「正在停止…」、不显示打字动画。
正在跑的命令连同子进程结束（Windows 作业对象已有；Unix 改为整个进程组），审批立刻按「停止」收尾；还没跑完、在排队、在等确认的调用（递归进子 Agent
运行记录）统一标灰色「已停止」（`ToolEvent.stopped`、`MessageBlock::markStopped`），不算失败也不算拒绝。被停的空回复不再写「（已停止）」，
而是底栏「已停止」标签（`ChatMessage.stoppedByUser`）；没被停的空回复写「（没有输出）」。停止时不进自愈续写；下一轮先按存档重新同步
（在线丢会话、本地 `replaceHistory`），历史里给被停的助手消息补 stoppedNote，停掉的调用回灌「不要自动重试」。
→ core-loop.md §1.1、§2.2–§2.3、§2.9、§2.11；ui.md §1.1、§2.2；subagents.md §1.1。

### 1.2 v1.6 工作中补充 + 工作中换模型（76534ff = 9536c33、1b760a4）
生成中也能发消息（`submit`）：排队（`QueuedInput`，只在内存），会话末尾一条一个虚线气泡、可单独撤回；做完当前这一步（一段生成的工具调用全部执行完、
发下一次请求之前，`ChatEngine::interjection`）分条作为「工作中补充」（`ChatMessage.interjected`）接进上下文：当前回复收尾（统计定稿）→ 补充气泡 →
新开一条助手消息接着写（一轮拆成几条助手消息，`turnMessageIDs`）；这一轮不再调工具就在结束后分条作为下一轮一起发出。被用户停止的一轮结束时还有排队 →
暂停，点「发送」才发。工作中可换大脑和思考强度：这一轮照旧（`turnEngineKey / turnThinking` 快照），底栏橙色「下一轮生效」；模型胶囊生成中不再禁用。
→ core-loop.md §1.2、§2.4、§2.6、§2.13；ui.md §1.2、§2.3。

### 1.3 v1.7 父子协作（e3aa578 = 7f53e44、33aff41）
子 Agent 可后台跑（`background: true`），中途汇报（`report_to_parent`）/ 完成 / 失败 / 被停止以灰色汇报卡片（`ChatMessage.fromAgent`，role 为 user）
接进父级；父级用 `message_agent` 追加指示、`stop_agent` 单独停掉、`wait_agents` 等消息；用户可在卡片上单独停止某个子 Agent（v1.8 起给子 Agent
发消息的输入框只在 Agent 树详情里）。团队状态（收件箱、队列、活动、运行中）在 `AgentTeamState`。调度修死锁：等下级 / 等确认让出本地名额
（`waitingWithoutSlot`），等完优先拿回、深的先走。在线并发不设上限，按请求过 `RequestGate`：从起步并发开始、没被限流且有排队就加、429 减半、
额度 / 权限类 429 直接报错。主 Agent 想结束这一轮时还有后台子 Agent 就等它们（`drainBackgroundAgents`）。
→ subagents.md §1.3、§2.2–§2.4、§2.6；core-loop.md §1.3、§2.4.2；ui.md §1.3、§2.4。

### 1.4 v1.8 层数不限 / 活性 / 用量 / Agent 树（f1f6d58 = 95d4df0、a3b7284、2084be2、9096118、e672b61）
删掉层数上限：每层必须拆小（只派一个下级且和自己的任务相似度 ≥ 0.8 被拒），每轮派发总数默认 100（`subAgentTurnLimit`，0 = 不限），
第 2 层起任务后附上级链，第 4 层起提示「除非明显还能拆，否则自己做」。活性：状态文字、看门狗（生成中 5 分钟没输出就停）、`deadline_seconds`、
原地打转（同样的调用第 3 次提醒、第 5 次拦下）、子 Agent 每 40 步提醒；被停 / 失败的子 Agent 交回半成品并按原因给建议（`SubAgentEnded`）。
用量：每个子 Agent 的模型、思考强度、温度、速度、生成 / 输入（`ToolEvent.usage = AgentUsage`），工具栏显示会话合计。界面：子 Agent 卡片默认收起、
标题后模型胶囊、并行派发用小标签、深层缩进收窄；Agent 树面板（对话区右边一列，按轮次，带连接线，详情页，第一次出现第 3 层时自动打开）。
→ subagents.md §1.4、§2.5、§3.6–§3.7；core-loop.md §1.4；ui.md §1.4、§2.5–§2.8。

### 1.5 v1.9 菜单栏计量（e2f8f41 = 5103122）
`liveAgents`（主 Agent 在生成算 1 + 运行中的子 Agent，全部对话）与 `liveSpeed`（3 秒窗口速度合计，一位小数）生成期间每秒更新。
Swift 画在菜单栏图标右边两行；**Qt 的托盘图标放不下字**：tooltip「Friday / N AI / 速度」，托盘菜单状态行下面一行禁用项「N AI 在工作 · 速度」（N > 0），
图标本身不变（§3.11.6）。→ core-loop.md §1.5；ui.md §1.5、§2.9。

### 1.6 d696a44 输入区底栏胶囊只显示图标和当前值
模型胶囊只放「图标 + 模型名」（服务名移到提示「在线引擎 <标题> · <模型>（<地址>）」），宽度贴合名字、不再固定撑到 260，窗口窄时只有模型名从中间截断
（底栏的空白最后分配）；本地状态文字等宽数字；深度思考胶囊只显示当前值（只有开 / 关：「开启 / 关闭」；分档：「低 / 中 / 高」；不支持：「不支持」），
提示「深度思考：这个模型只有开 / 关」/「深度思考分档：低 / 中 / 高，越高越慢越贵」；权限、工作目录、深度思考、模型四个胶囊都去掉下拉箭头。
→ `git show d696a44`；归 chat-ui（§5.3）。

### 1.7 d621837 显示名、接口地址、密钥名改为服务一级
三者是服务一级的设置：在服务的「模型」面板（ServiceModelsSheet）统一改，这家服务的全部模型和跟着它的能力提供方一起更新；模型的「高级」（EngineEditor）
里不再有这三项（标题改「<显示名> 的模型设置」）；接入表单（AddServiceForm）可填接口地址；目录里不需要密钥的服务（Ollama、LM Studio）改了地址后
按显示名仍认这家服务。→ `git show d621837`；§3.12；归 settings（§5.5）。

---

## 2. 全局约定

### 2.1 线程（沿用 ARCHITECTURE.md §3；下面是这次新增 / 强调的不变量）

| # | 规则 |
|---|---|
| T1 | 被界面观察的状态只在主线程改，信号只在主线程发。唯一的「任意线程可调」的 QObject 是 `AgentTeamState`：内部一把 `std::mutex`，任意线程读写，但 `changed()` / `treeFocusChanged()` / `clockChanged()` 只在主线程发（变化时 `runOnMain` 合并成一次）。 |
| T2 | 工作线程阻塞式顺序书写；取消一律 `CancelToken`：整轮停止 = 回合令牌 + `cancelTeam()`（后台子 Agent 的令牌是独立的，不是回合令牌的子令牌）；单独停一个 = 那个子 Agent 的令牌。等待一律 `Promise` / `Worker::sleep`（它们会让出线程池名额，§3.10）。 |
| T3 | 主线程从不等待工作线程：`stop / stopAgent / sendToAgent / cancelTeam / 看门狗 / 计量 / tickClock` 都不阻塞。 |
| T4 | 工作线程改会话只用 `updateMessage / updateEvent / eventTask`（排到主线程，FIFO）；读用 `runOnMainSync`。持任何锁时不 `runOnMainSync`、不阻塞、不发信号；取消令牌一律「锁内收集、锁外 `cancel()`」（`cancel` 会同步跑回调）。 |
| T5 | 子 Agent 的本地块副本只在它自己的线程读写；别的线程要改它（下级结束写状态）只能经它的信箱 `AgentEditMailbox::postEdit`，同时 `updateEvent` 改存档。 |
| T6 | **终态不回退**：ToolEvent 的 status 一旦是 succeeded / failed / denied（`ToolEvent::isFinished()`），任何晚到的更新（流式发布、`SubRun::merge`、`onQueued` 的排队标记、信箱里的旧修改）都不得把 status 改回 running / awaitingApproval，也不得改 result / stopped / media，不得把 `queued` 设成 true；`usage` 与运行记录可以继续合并，运行记录里的嵌套卡片递归适用同一规则。 |
| T7 | `isStopping()` 任意线程可读（原子量）；`m_turnMessageIDs` 与 `m_streamMessageID` 同受 `m_streamMutex` 保护；主 Agent 的速度窗口受 `m_liveMutex` 保护（回合线程写、主线程每秒读）。 |
| T8 | 回合收尾闭包（主线程）里，`setGenerating(nullopt)` 之前先把这一轮的全部状态清 / 设好（core-loop.md §2.2 的 1–10 顺序）：它同步发 `generatingChanged`，直连槽可能立刻重入 `send()`。`send()` 同理：这一轮的快照在 `setGenerating(id)` 之前定好。 |

### 2.2 所有权、分区与改别人文件的规矩

- 每个文件只有一个包改（§5 的清单）。少数文件按「区」或「函数」分，§5 写明；没写的就是不许碰。
- 骨架（§6）先把跨包要用的声明一次加齐。之后沿用 ARCHITECTURE.md §2.3：头文件归实现它的包——可以加私有成员、补 include、**追加**公开方法；
  不改、不删已有的公开签名（别的包正按它写）。确实要改的写进交付总结，由协调者协调。
- `AgentRunner.h` 分区：骨架加两段注释围起来的区——public 里 `// ===== team 包（PORT_V15_V19.md §3.3）=====` … `// ===== team 包结束 =====`、
  private 里同名的一段——归 team；其余归 core。team 的实现放在 `AgentTeamControl.cpp` / `AgentLiveness.cpp` / `SubAgentRunner.cpp`，
  **不改 `AgentRunner.cpp`**；core 不改 team 的三个 .cpp。
- `OpenAIChatEngine.{h,cpp}` 按函数分（§5.1 / §5.2）。
- 测试：各包有自己的测试文件；共用的测试辅助由骨架从现有大文件里抽成头文件（§6.A），之后按 §5 归属。
- QML 组件：谁的文件谁改；别的包只按 §3.11.5 定的属性接口用它。
- 文档按段落分（§5 各包的「文档」一项）；`qt/README.md` 功能列表由协调者集成时写（§5.6）。

### 2.3 持久化（与 Swift Codable 键完全相同；nil 不写键；读时宽松）

| 位置 | 键 | 类型 / 规则 |
|---|---|---|
| ToolEvent | `stopped` | Bool?（只在有值时写；Swift 只写 true） |
| ToolEvent | `usage` | AgentUsage?（对象） |
| AgentUsage | `model`、`thinking`、`temperature`、`generatedTokens`、`promptTokens`、`generateTime` | 6 个**必写**（Swift 合成的 Decodable 要求键都在）；tokens 是 Int，另两个是 Double |
| AgentUsage | `service` | String?，nil 不写 |
| ChatMessage | `stoppedByUser`、`interjected` | Bool? |
| ChatMessage | `fromAgent` | String?（子 Agent 名字） |
| settings.ini | `subAgentTurnLimit` | Int，默认 100，读入 `max(0, …)` |
| settings.ini | `agentTreeVisible` | Bool，默认 false（== Swift `@AppStorage`） |
| settings.ini | `agentTreeWidth` | Double，默认 320，写入时夹到 260…560 |

不落盘：排队的消息（QueuedInput）、团队状态（runs、队列、收件箱、活动、开始时间、速度窗口）、子 Agent 的开始 / 结束时间（Swift 也不存）。
旧存档没有这些键 → nil。ConversationStore 读档时对每条消息 `markInterrupted`（现有，保留）。

### 2.4 Qt 版有意的差异（每一条都要在代码里写注释「Qt 版…」，并在交付总结里列出）

| # | 差异 | 原因 | 包 |
|---|---|---|---|
| Q1 | 并行派发在**整轮停止**时父卡片标「已停止」（Failed + `stopped = true`）；Swift 父卡片仍是成功 | 协调者决定保留 Qt 现有行为，与「统一标成已停止」的描述一致 | team |
| Q2 | 主 Agent 的一段请求遇到 `KVCacheFullError`：等一个本地子 Agent 结束（最多 2 秒）后原样重发这一段，最多 2 次，再不行才走 recover | Qt 的 KV 池是预分配、几个请求共用的（子进程模式一满就让所有在途槽位一起失败）；MLX 没有这个问题 | core（team 提供 `waitForLocalEnd`） |
| Q3 | 子 Agent / 看图 / 起标题的本地请求不落主序列：子进程模式借不到槽位时用显式的非主槽位（`sharedSlot`），不再发 `id_slot -1`；进程内运行时本来就只在 n_seq > 1 时用非主序列。只有一个序列时例外 | 防止子 Agent 挤掉主对话的 KV | team |
| Q4 | 本地子 Agent 的「剩余内存」在加载时折算成 S 个非主序列；名额与序列份额规则（§3.6.2）；`planContext` 去掉固定的 3 个上限（§3.6.4） | MLX 按需分配 KV，Qt 是加载时一次性分配 | team |
| Q5 | Worker 线程池：阻塞等待时让出池名额（§3.10）；同时活着的子 Agent 安全上限 1000（只在关掉「每轮总数上限」时可能碰到） | 层数 / 数量不限之后，父线程阻塞等子线程会把 256 个池线程占满而死锁；Swift 的 Task 没有这个问题 | team |
| Q6 | `EngineEvent::Progress`（工具参数在生成）算子 Agent「有动静」并计入估算 token | 一口气写几分钟大文件参数会被看门狗误停（Swift 在线引擎没有这个事件） | team |
| Q7 | 后台子 Agent 结束时已被 `reset()` 注销就不再投通知；子 Agent 收尾循环取不到待处理就抛取消、不空转 | Swift 有竞态（subagents.md §2.8-3、§2.8-4） | team |
| Q8 | 终态不回退（T6）与信箱（T5） | Swift 靠 @MainActor 串行，没有跨线程发布的竞态 | team |
| Q9 | 托盘不画字：tooltip 三行 + 菜单状态行 | Windows / Linux 托盘图标是 16–32 px 方块，放不下两行字 | tree-tray |
| Q10 | 工具栏放不下时 Agent 树按钮 / 用量徽标进「»」菜单（`overflowText`，§3.11.7） | NSToolbar 会自动处理，Qt 的 Toolbar 只收按钮 | tree-tray |
| Q11 | 子 Agent 卡片停止键的「正在停止」同时看 `AgentTeam.isStopRequested(id)` | 委托重建会丢 QML 本地状态（Swift 的 @State 跟视图身份走，Qt 的 ListView 会回收委托） | chat-ui |
| Q12 | 调试钩子 `FRIDAY_SLOW_AGENT_STOP`、DebugHarness 的 `drag:` / `traytip` / 注释行、FakeChatServer 与 `mock_sse.py` 的扩展 | 截图与测试需要 | team / tree-tray |
| Q13 | Unix 上 run_shell 取消 / 超时时对整个进程组 SIGTERM，2 秒后 SIGKILL（Swift 是 `pkill -P` + terminate） | 覆盖得更全；Windows 作业对象已是整棵进程树 | core |
| Q14 | `AgentUsage::detail` 的温度按 `QLocale()`（== Swift 当前区域）格式化，参数可传 locale | 单元测试固定 `QLocale::c()` | （骨架） |
| Q15 | `eventTask / updateEvent` 在**创建**闭包时捕获这一轮的对话 id 与助手消息 id 列表（Swift 在执行时读） | 回合线程的 `finish()` 先清 `m_turnMessageIDs` 再排收尾闭包，排在它前面的卡片更新执行时会扑空 | core |

---

## 3. 接口（定稿）

骨架（§6）按本节一次加齐声明；「（骨架实现）」= 骨架就写完整实现（纯函数 / 数据，多个包都要用），「（某包实现）」= 骨架给空实现、该包填。
空实现的行为写在 §6.C。

### 3.1 数据（`core/models/ChatModels.h`、`core/agent/QueuedInput.h`、`core/agent/ToolTypes.h`、`core/models/AppSettings.h`）

```cpp
// ChatModels.h（骨架实现，之后归 core）
/// 一个子 Agent 用的大脑和用量（== Swift AgentUsage，Models/ChatModels.swift:85-106）
struct AgentUsage {
    QString model;                    // 在线：引擎的 model；本地：模型显示名
    std::optional<QString> service;   // 在线服务名（EngineConfig.title）；本地 nullopt
    QString thinking;                 // 「关闭」「低」「中」「高」；大脑不支持思考时「不支持」
    double temperature = 0;
    int generatedTokens = 0;
    int promptTokens = 0;             // 本地：新处理的提示词；在线：各段请求的输入合计
    double generateTime = 0;          // 秒，不含等首 token
    /// generateTime > 0 ? generatedTokens / generateTime : 0
    double tokensPerSecond() const;
    /// 「深度思考：<thinking>\n温度：<0–2 位小数、去掉末尾的 0>[\n服务：<service>]」（== Swift detail；Q14）
    QString detail(const QLocale& locale = QLocale()) const;
    QJsonObject toJson() const;                          // 6 个必写键 + service（nil 不写）
    static AgentUsage fromJson(const QJsonObject& object);   // 宽松：缺的用默认值
    /// {model, service（缺省 ""）, thinking, temperature, generatedTokens, promptTokens, generateTime, tokensPerSecond, detail}
    QVariantMap toVariant() const;
    bool operator==(const AgentUsage&) const = default;
};

struct ToolEvent {
    // … 现有字段 …
    /// v1.5：没跑完是因为停止（整轮停止 / 单独停掉 / 看门狗 / 期限）；status 记 Failed，界面灰色「已停止」
    std::optional<bool> stopped;
    /// v1.8：子 Agent 用的大脑和用量（子 Agent 自己写）
    std::optional<AgentUsage> usage;
    /// succeeded / failed / denied（§2.1 T6）
    bool isFinished() const;
    // toJson / fromJson / operator== 带上两个新字段；
    // toVariant 追加：stopped（bool，缺省 false）、usage（AgentUsage::toVariant，没有为空 map）、hasUsage（bool）
};

struct ChatMessage {
    // … 现有字段 …
    std::optional<bool> stoppedByUser;   // v1.5：这一轮是点停止结束的
    std::optional<bool> interjected;     // v1.6：工作中补充
    std::optional<QString> fromAgent;    // v1.7：后台子 Agent 的汇报 / 结果（role 为 user；值是子 Agent 名字）
    // toVariant 追加：stoppedByUser、interjected（bool，缺省 false）、fromAgent（缺省 ""）。
    // 汇报卡片的标题 / 正文不放这里（ChatModels 不依赖 AgentRunner），由 MessageListModel 的角色算（§3.11.3）
};

struct MessageBlock {
    /// v1.5（core 实现；骨架空实现）：running / awaitingApproval 的调用（递归进运行记录）→ Failed、stopped = true、queued = nullopt；
    /// result = 原来是 awaitingApproval ?「等待确认时被停止，没有执行」:「已停止」（先按原状态定 result，再改 status）。已结束的不动
    static void markStopped(QList<MessageBlock>& blocks);
};
```

```cpp
// core/agent/QueuedInput.h（新，骨架实现，之后归 core）——对应 Swift：Agent/QueuedInput.swift
/// Agent 工作时排队的一条消息；主 Agent 的排队（AgentRunner）和发给子 Agent 的消息（AgentTeamState::queue）共用。只在内存里
struct QueuedInput {
    QUuid id = QUuid::createUuid();
    QString text;                                   // 已去首尾空白
    QStringList attachments;                        // 本地文件路径（发出 / 交给 Agent 时才导入媒体目录）
    QDateTime createdAt = QDateTime::currentDateTimeUtc();
    std::optional<QString> from;                    // nullopt = 用户；否则父级名字（「主 Agent」「父 Agent「X」」「Friday」）
    /// {id（大写 UUID）, text, attachments（路径列表）, attachmentNames（文件名用「、」连接）, from（缺省 ""）, createdAt}
    QVariantMap toVariant() const;
    bool operator==(const QueuedInput&) const = default;
};
```

```cpp
// ToolTypes.h（骨架实现，之后归 team）
struct ToolOutput {
    QString text;
    QList<MediaAttachment> media;
    /// v1.7：后台派发的子 Agent——工具已经返回，它们还在跑（dispatch 把卡片留在 Running，结束时再定稿）
    bool runsInBackground = false;
    ToolOutput() = default;
    explicit ToolOutput(QString text, QList<MediaAttachment> media = {}, bool runsInBackground = false);
    QString modelText() const;
};

// AppSettings.h（骨架实现）
/// 一轮最多派多少个子 Agent（所有层加起来；只防失控）。0 = 不限；默认 100；键 subAgentTurnLimit
Q_PROPERTY(int subAgentTurnLimit READ subAgentTurnLimit WRITE setSubAgentTurnLimit NOTIFY subAgentTurnLimitChanged)
```

### 3.2 AgentRunner——core 部分（`core/agent/AgentRunner.h`，core 区）

行为：core-loop.md §1.1–§1.5、§2.2–§2.5、§2.11–§2.13。`AgentRunner.h` 新增 include：`agent/AgentTeamState.h`、`agent/QueuedInput.h`。

```cpp
class AgentRunner : public QObject, public ToolSink {
    Q_OBJECT
    // … 现有两个属性 …
    /// v1.5：点了停止、这一轮还没收尾（停止键旋转、底栏「正在停止…」）
    Q_PROPERTY(bool isStopping READ isStopping NOTIFY stoppingChanged)
    /// v1.9：正在工作的 Agent 个数（主 Agent 在生成算 1 + 运行中的子 Agent，全部对话）与速度合计（t/s，一位小数）；值变了才发
    Q_PROPERTY(int liveAgents READ liveAgents NOTIFY liveMeterChanged)
    Q_PROPERTY(double liveSpeed READ liveSpeed NOTIFY liveMeterChanged)
    /// v1.6：排队或暂停状态每变一次 +1（QML 绑定 queuedInputs(id) / isQueuePaused(id) 时让它参与求值，ARCHITECTURE §8.1）
    Q_PROPERTY(int queueRevision READ queueRevision NOTIFY queueRevisionChanged)

public:
    /// v1.7：父子 Agent 往来的状态（构造时 new AgentTeamState(this)；QML 里是单例 AgentTeam，不经 AgentRunner 取）
    AgentTeamState* const team;

    // MARK: 停止（v1.5）
    bool isStopping() const;                       // 任意线程（原子量）
    static QString stoppedNote();                  // 「（这一轮被用户中途停止了，上面的回答和操作可能不完整。）」

    // MARK: 工作中补充、工作中换模型（v1.6）——主线程
    /// 输入框、空状态建议都走它：空闲能发且这段没有排队就 send，否则排队（== Swift submit；replacing 在忙时直接忽略）
    void submit(const QString& text, const QStringList& attachments, const QUuid& conversationID,
                const std::optional<QUuid>& replacing = std::nullopt);
    void removeQueued(const QUuid& itemID, const QUuid& conversationID);
    void resumeQueue(const QUuid& conversationID);
    QList<QueuedInput> queuedInputs(const QUuid& conversationID) const;
    bool isQueuePaused(const QUuid& conversationID) const;
    int queueRevision() const;
    static QString interjectionNote();             // 「（用户在你工作时补充：…有冲突以这条为准。）\n」
    /// "engine:<引擎 id>" / "local:<本地模型 id>"
    QString engineKey(const QUuid& conversationID) const;
    struct TurnChanges {
        bool model = false;
        bool thinking = false;
    };
    /// 这段在生成、且大脑 / 思考强度（原始档位）与这一轮开始时定下的不同（== Swift pendingTurnChanges）
    TurnChanges pendingTurnChanges(const QUuid& conversationID) const;
    /// 回合线程（引擎的 interjection 回调、drainBackgroundAgents）：取走排队的补充（暂停时不取）和主 Agent 收件箱里的汇报，
    /// 当前助手消息收尾、写入汇报卡片和补充、开新的助手消息（发 turnMessageStarted），返回交给模型的用户消息（== Swift takeInterjection）
    QList<EngineMessage> takeInterjection(const QUuid& conversationID, bool supportsVision);

    // MARK: 这一轮的消息（v1.6 / v1.7）——任意线程
    QList<QUuid> turnMessageIDs() const;           // 从旧到新；m_streamMutex
    QList<ChatMessage> currentTurnMessages(const QUuid& conversationID) const;   // runOnMainSync 取最新内容
    /// 改一个工具事件（可能嵌在子 Agent 运行记录里）：返回排到主线程执行的闭包；闭包在**创建时**捕获的这一轮全部助手消息里
    /// 从新到旧找、改第一个找到的（== Swift updateEvent；Q15）。不在生成返回空 std::function。updateEvent 改为 runOnMain(eventTask(…))
    std::function<void()> eventTask(const QUuid& eventID, std::function<void(ToolEvent&)> body) const;

    // MARK: 原地打转（v1.8；对应 Swift AgentLiveness.swift 的 noteRepeat，实现在 AgentRunner.cpp）
    static constexpr int repeatWarn = 3;
    static constexpr int repeatBlock = 5;
    static constexpr int stepNudge = 40;
    static const QSet<QString>& pollingTools();    // wait_agents、check_job、get_current_time
    /// 记下这次调用，返回同样的调用连续出现了几次（轮询类返回 0）；sink 是 SubRun 用它的 lastCall，否则 m_mainLastCall（回合线程）
    int noteRepeat(const ToolCall& call, ToolSink& sink);

    // MARK: 计量（v1.8 / v1.9）
    int liveAgents() const;                        // 主线程
    double liveSpeed() const;                      // 主线程
    /// 主 Agent 最近 3 秒的速度；这段不在流式生成返回 0（任意线程）
    double mainLiveSpeed(const QUuid& conversationID, const QDateTime& now = QDateTime::currentDateTimeUtc()) const;
    /// 一个 chunk 约几个 token：<think> / </think> 0；本地 1；在线 max(1, round(UTF-8 字节数 / 3.5))
    static int estimateTokens(const QString& chunk, bool local);

    // MARK: QML（加在现有「// MARK: QML」一节）
    Q_INVOKABLE void submit(const QString& text, const QStringList& attachments, const QString& conversationID,
                            const QString& replacing = QString());
    Q_INVOKABLE void removeQueued(const QString& itemID, const QString& conversationID);
    Q_INVOKABLE void resumeQueue(const QString& conversationID);
    Q_INVOKABLE QVariantList queuedInputs(const QString& conversationID) const;      // [QueuedInput::toVariant]
    Q_INVOKABLE bool isQueuePaused(const QString& conversationID) const;
    Q_INVOKABLE QVariantMap pendingTurnChanges(const QString& conversationID) const; // {model: bool, thinking: bool}
    Q_INVOKABLE double mainLiveSpeed(const QString& conversationID) const;

    // ===== team 包（§3.3）… =====

signals:
    // … 现有 …
    void stoppingChanged();
    /// 某段对话的排队消息或暂停状态变了（主线程）
    void queuedInputsChanged(const QUuid& conversationID);
    void queueRevisionChanged();
    /// liveAgents / liveSpeed 变了（每秒最多一次，值不变不发；都停下归零时发一次）
    void liveMeterChanged();
    /// 回合中途换了一条新的助手消息（插入补充 / 汇报之后；主线程）。friday-cli 等跟着正在写的消息走的地方用它
    void turnMessageStarted(const QUuid& conversationID, const QUuid& messageID);
};
```

core 区的私有成员由 core 自定（建议照 core-loop.md §2.1 的私有部分：`TurnPhase` 状态机、`m_stopRequested`、`m_stoppedConversations`、
`m_queuedInputs`、`m_pausedQueues`、排队投递的 QTimer、`m_turnEngineKey / m_turnThinking / m_turnEngineName / m_turnExpectsThinking`、
`m_turnMessageIDs`、`m_mainLastCall`、`m_liveMutex` + `QList<AgentTeamState::Sample> m_recentChunks`（替换现有 `RecentChunk`）、计量 QTimer、
`markResync / resyncLocalSession` 替换 `markRefold / refoldLocalSession / m_refoldPending`、`discardMainPass()`）。
被改的现有签名（全是 core 私有）：`runTurn(…, const QList<ChatMessage>& userMessages, …)`、`describeAttachments(const ChatMessage&, bool supportsVision, const QUuid&)`；
删除 `expectsThinking` / `engineNameForStats`（改读这一轮的快照）。

core 还负责在这些位置调 team 的东西（Swift 出处见 subagents.md §3.2 表）：`configure` 与 `refreshTools` 用 `tools->guidance(AgentTeamTools::Role::Main)`、
`tools->mainAgentSpecs()`；`configure` 每次设 `engine.interjection = [..]{ return takeInterjection(id, vision); }`；`send()` 里 `startMeter()`、
每轮第一次流式前清 `m_mainLastCall`；`takeInterjection` 里 `team->takeNotices(mainKey)`；`drainBackgroundAgents`；`submit` 时这段在生成就
`team->wake(mainKey)`；`stop()` 里 `cancelTeam()`（= `team->cancelRuns(id)` + `team->wake(mainKey)`）；`finish()` 里 `team->reset(id)`；
`dispatch` 的打转拦截、`waitingWithoutSlot(sink, Activity::waitingApproval(), …)`、`setActivity / noteStep` 与退出时回 generating、
`runsInBackground ? Running : Succeeded`、`SubAgentEnded` 分支、打转 / 步数提醒；计量里 `team->runningCount()`、`team->speed(id)`、
每秒 `team->tickClock()`。

### 3.3 AgentRunner——team 部分（`AgentRunner.h` 的 team 区）与 SubRun

行为：subagents.md §1.3–§1.4、§2.3–§2.4、§2.8。

```cpp
    // ===== team 包（PORT_V15_V19.md §3.3）：实现在 AgentTeamControl.cpp / AgentLiveness.cpp / SubAgentRunner.cpp =====
    // MARK: 父子往来——任意线程
    static QString agentMessageTool();                                   // "agent_message"（运行记录里的卡片名，不是工具）（骨架实现）
    static QString agentMessageNote(const std::optional<QString>& from); // subagents.md §1.3.3（骨架实现）
    static QString noticeText(const AgentTeamState::Notice& notice);     // subagents.md §1.3.4（骨架实现）
    static QString noticeTitle(const QString& firstLine);                // 「中途汇报」「完成」「失败」「已停止」（骨架实现）
    QUuid parentKey(const ToolSink& sink) const;                         // SubRun → 它的事件 id；主 Agent → mainKey
    QString parentName(const ToolSink& sink) const;                      // 「父 Agent「X」」/「主 Agent」
    /// 只停这一个：记下 by、锁外取消它的令牌；没登记或已经停过返回 false。不阻塞（主线程也可调）
    bool stopAgent(const QUuid& agentID, const QString& by = QStringLiteral("用户"));
    /// 发给运行中的子 Agent（它做完当前这一步就看到）；不在运行或文字和附件都空返回 false
    bool sendToAgent(const QUuid& agentID, const QString& text, const QStringList& attachments = {},
                     const std::optional<QString>& from = std::nullopt);
    void removeAgentQueued(const QUuid& itemID, const QUuid& agentID);
    /// 按 id 前缀（去空白、小写，少于 4 个字符为 nullopt）找 parentKey 派出、还在运行的直接下级：(id, 名字)
    std::optional<std::pair<QUuid, QString>> runningChild(const QString& prefix, const QUuid& parentKey,
                                                         const QUuid& conversationID) const;
    /// 子 Agent 引擎的 interjection（它自己的线程）
    QList<EngineMessage> takeAgentInterjection(SubRun& run, bool supportsVision);
    /// 派发检查（参数校验之前）：每轮总数上限、Qt 版同时活着的安全上限（§3.6.6）、单个下级原样往下传；不通过抛 AgentToolError
    void checkDispatch(const QString& tool, const QJsonObject& args, const ToolSink& sink);
    /// 第 2 层起附在任务后面的「你在整个任务里的位置」；第 1 层 nullopt
    std::optional<QString> ancestryNote(const QUuid& eventID, const QUuid& conversationID) const;
    /// == Swift waitingWithoutSlot(sink, as:, body)：sink 是占着本地名额的 SubRun 时活动标成 waiting、让出名额（序列不还），
    /// body 结束后（没被取消、不在整轮停止）以 resumeBoost + 层数拿回名额、卡片 queued 期间为 true、标回 generating；
    /// 主 Agent / 在线子 Agent 只设活动（主 Agent 连活动都没有）、直接执行 body。body 的异常原样传出；
    /// body 成功而拿回名额时被取消 → 抛 CancellationError。调用方按引用捕获 body 的结果
    void waitingWithoutSlot(ToolSink& sink, const AgentTeamState::Activity& waiting, const std::function<void()>& body);
    /// 改一个子 Agent 的卡片：父级是登记着的 SubRun 时投进它的信箱（T5），并且 updateEvent 改存档
    void updateAgentEvent(const QUuid& eventID, const QUuid& parentKey, std::function<void(ToolEvent&)> body);

    // MARK: 活性（AgentLiveness.cpp）
    double stallLimitSeconds() const;              // 300；调试构建 FRIDAY_STALL_LIMIT=<秒>（每次读）；测试可改
    static constexpr int deadlineGraceSeconds = 60;
    /// 「在执行「X」 · 12 秒前有动静 · 第 5 步 · 已运行 3 分 2 秒[ · 期限还剩 …]」；compact 到时长为止；没有活动 nullopt（任意线程）
    std::optional<QString> agentStatusText(const QUuid& agentID, const QDateTime& now = QDateTime::currentDateTimeUtc(),
                                           bool compact = false) const;
    static QString duration(double seconds);       // 「N 秒」「M 分 S 秒」「H 小时 M 分」（骨架实现）
    void startWatchdog();                          // 任意线程；主线程 QTimer 每 5 秒巡查，running 空了就停（懒建，构造函数不动）
    std::optional<QDateTime> childDeadline(std::optional<double> seconds, const QUuid& parentKey) const;
    /// 等它做完再返回的子 Agent：中途汇报并进结果，并删掉它在父级收件箱里的全部通知
    ToolOutput withReports(const ToolOutput& output, const QUuid& agentID, const QUuid& parentKey);
    std::optional<QString> partialResult(SubRun& run) const;   // 在 SubRun 自己的线程、退出前算
    /// == Swift endedError：整轮在停止原样；被单独停 → SubAgentEnded(stoppedBy, "", partial)；取消 / 已是 SubAgentEnded 原样；
    /// 其余 → SubAgentEnded(nullopt, 错误文字, partial)
    std::exception_ptr endedError(std::exception_ptr error, const QUuid& agentID) const;
    /// 测试：看门狗间隔、期限宽限、卡住阈值（调试 / 测试构建里有效）
    void setLivenessTimingForTests(int watchdogTickMs, int graceSeconds, double stallSeconds);

    // MARK: QML（参数是大写 UUID 字符串）
    Q_INVOKABLE bool stopAgent(const QString& agentID);
    Q_INVOKABLE bool sendToAgent(const QString& agentID, const QString& text, const QStringList& attachments = {});
    Q_INVOKABLE void removeAgentQueued(const QString& itemID, const QString& agentID);
    Q_INVOKABLE QString agentStatusText(const QString& agentID, bool compact = false) const;   // 没有活动返回 ""
    // ===== team 包结束 =====

    // （private 里的 team 区：m_watchdog、计时参数、watchdogTick、finishBackgroundParentIfDone、三种派法的辅助……由 team 自定）
```

`SubRun`（`SubAgentRunner.h`，归 team）——骨架只加 core 要用的两项，其余（`holdsLocalSlot`、`isLocal`、用量、`AgentEditMailbox` 基类与信箱、
`merge` 的终态不回退）由 team 加：

```cpp
    int steps() const { return m_steps; }                  // consumeStep 计数（含被预检 / 打转拦下的）；步数提醒用
    std::optional<std::pair<QString, int>> lastCall;       // noteRepeat：签名、连续次数（只在它自己的线程读写）
```

### 3.4 AgentTeamState（`core/agent/AgentTeamState.{h,cpp}`，新，归 team；头文件骨架写全）

对应 Swift `Agent/AgentTeamControl.swift:12-194`。语义见 subagents.md §1.3.1、§3.1；与附录不同的地方以这里为准。

```cpp
/// 子 Agent 本地块副本的信箱（AgentRunner::SubRun 实现）：嵌套类不能前置声明，单独成接口
class AgentEditMailbox {
public:
    virtual ~AgentEditMailbox() = default;
    /// 任意线程：投递对它本地块里某个事件的修改；它自己的线程在下次发布 / 读块前应用
    virtual void postEdit(const QUuid& eventID, std::function<void(ToolEvent&)> body) = 0;
};

class AgentTeamState : public QObject {
    Q_OBJECT
    /// running / activity / steps / queues 任一变化 +1（QML 绑定的依赖）
    Q_PROPERTY(int revision READ revision NOTIFY changed)
    Q_PROPERTY(int runningCount READ runningCount NOTIFY changed)
    /// 主 Agent 在生成或有子 Agent 在跑时每秒 +1（core 的计量调 tickClock），都停下后最后再 +1 一次；
    /// 状态行、树的运行中节点、用量合计按它刷新（不在每张卡片里各开一个 Timer）
    Q_PROPERTY(int clock READ clock NOTIFY clockChanged)
    /// 要在 Agent 树面板里打开的子 Agent（大写 UUID；空 = 没有）。「在 Agent 树里看」、小标签、调试钩子写；面板接住后写回空
    Q_PROPERTY(QString treeFocus READ qmlTreeFocus WRITE setQmlTreeFocus NOTIFY treeFocusChanged)

public:
    enum class ActivityKind { Generating, Tool, WaitingChildren, WaitingApproval, Queued };
    struct Activity {
        ActivityKind kind = ActivityKind::Generating;
        QString title;                                       // Tool 的工具显示名
        static Activity generating() { return {}; }
        static Activity tool(const QString& title) { return { ActivityKind::Tool, title }; }
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
        CancelTokenPtr token;                  // == Swift Run.task
        std::optional<QString> stoppedBy;
        std::optional<QDateTime> deadline;     // UTC
        int depth = 1;                         // Qt 补充（调试钩子「最深」、优先级）
        std::optional<QString> partial;        // Qt 补充：异常退出时算好的半成品（endedError 用）
    };
    struct Sample {
        QDateTime time;
        int tokens = 0;
    };

    static QUuid mainKey();                    // 00000000-0000-0000-0000-00000000000A（骨架实现）
    /// Qt 版安全上限：同时登记着的子 Agent 不超过这么多（§3.6.6；只在关掉每轮总数上限时可能碰到）
    static constexpr int maxLiveRuns = 1000;

    explicit AgentTeamState(QObject* parent = nullptr);

    // 登记（== register / unregister）
    void registerRun(const QUuid& id, Run run);           // 在子 Agent 线程开始前登记
    bool unregister(const QUuid& id);                     // 返回之前是否登记着
    std::optional<Run> run(const QUuid& id) const;
    QHash<QUuid, Run> runsSnapshot() const;
    int runCount() const;
    bool isRunning(const QUuid& id) const;
    QList<QUuid> running() const;
    int runningCount() const;
    /// stopAgent 用：在且还没停过 → 记下 who 并返回它的令牌（调用方锁外 cancel）；否则空
    CancelTokenPtr markStoppedBy(const QUuid& id, const QString& who);
    std::optional<QString> stoppedBy(const QUuid& id) const;
    void setPartial(const QUuid& id, const std::optional<QString>& partial);
    std::optional<QString> partial(const QUuid& id) const;
    /// 整轮停止（core 的 cancelTeam）：这段对话的 run 全部取消（不记 stoppedBy、不注销）
    void cancelRuns(const QUuid& conversationID);
    /// 回合收尾（core 的 finish）：这段对话的 run 取消并注销、叫醒全部等待、清空全部收件箱、launched 归零
    void reset(const QUuid& conversationID);

    // SubRun 信箱（只活在 runSubAgent 的栈帧里：建好引擎后登记，退出前撤下）
    void registerSubRun(const QUuid& id, AgentEditMailbox* run);
    void unregisterSubRun(const QUuid& id);
    bool postEdit(const QUuid& parentKey, const QUuid& eventID, std::function<void(ToolEvent&)> body);

    // 每轮总数
    int launched(const QUuid& conversationID) const;
    void addLaunched(const QUuid& conversationID, int count);

    // 活动、步数、速度
    void setActivity(const QUuid& id, const Activity& activity);   // 没登记忽略；也记一次「有动静」
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
    /// 只留 now - time <= 3 秒的；不足 2 个为 0；(除第一个外的 tokens 之和) / (now - 第一个.time)（骨架实现）
    static double windowSpeed(const QList<Sample>& samples, const QDateTime& now);
    bool markDeadlineWarned(const QUuid& id);              // 第一次返回 true

    // 收件箱
    QList<QUuid> backgroundChildren(const QUuid& key, const QUuid& conversationID) const;
    void post(const Notice& notice, const QUuid& key);     // 追加并 wake(key)
    QList<Notice> takeNotices(const QUuid& key);           // 先进先出，取走即清空
    bool hasNotices(const QUuid& key) const;
    void removeNotices(const QUuid& key, const QUuid& agentID);
    QStringList takeReports(const QUuid& key, const QUuid& agentID);   // withReports 用
    /// 等 key 的收件箱非空 / 被 wake / 超时 / 当前线程令牌取消；不抛（== Swift waitForNotice，调用方自己查取消）
    void waitForNotice(const QUuid& key, std::optional<int> timeoutMs = std::nullopt);
    void wake(const QUuid& key);

    // 发给子 Agent 的消息（界面观察）
    bool enqueue(const QUuid& id, const QueuedInput& item);   // 不在 running 返回 false
    QList<QueuedInput> queue(const QUuid& id) const;
    QList<QueuedInput> takeQueue(const QUuid& id);
    bool hasQueue(const QUuid& id) const;
    void removeQueued(const QUuid& id, const QUuid& itemID);

    // 树焦点、时钟
    std::optional<QUuid> treeFocus() const;
    void setTreeFocus(const std::optional<QUuid>& id);
    int revision() const;
    int clock() const;
    void tickClock();                                      // 主线程（骨架实现）

    // QML（参数是大写 UUID 字符串）
    QString qmlTreeFocus() const;
    void setQmlTreeFocus(const QString& id);
    Q_INVOKABLE bool isRunning(const QString& id) const;
    Q_INVOKABLE bool hasActivity(const QString& id) const;
    Q_INVOKABLE bool isStopRequested(const QString& id) const;   // stoppedBy 有值（Q11）
    Q_INVOKABLE QVariantList queue(const QString& id) const;     // [QueuedInput::toVariant]
    Q_INVOKABLE double speed(const QString& id) const;
    Q_INVOKABLE int passTokens(const QString& id) const;

signals:
    void changed();
    void treeFocusChanged();
    void clockChanged();
};
```

### 3.5 SubAgentEnded（`core/agent/AgentLiveness.{h,cpp}`，新，归 team；这个类骨架实现）

```cpp
/// 子 Agent 被单独停掉（stoppedBy 有值）或失败（nullopt），带着做到一半的结果交回（== Swift AgentLiveness.swift:13-40）
class SubAgentEnded : public FridayError {   // message() == cardText()
public:
    SubAgentEnded(std::optional<QString> stoppedBy, QString message, std::optional<QString> partial);
    const std::optional<QString>& stoppedBy() const;
    const QString& detail() const;                    // Swift 的 message
    const std::optional<QString>& partial() const;
    /// (stoppedBy ?「被{stoppedBy}停止了，没做完。」:「失败了：{message}」) + (partial ?「\n\n」+ partial : "")
    QString cardText() const;
    /// 按 cardText 开头给建议（四句，core-loop.md §1.4.3 / subagents.md §1.4.3）
    static QString advice(const QString& cardText);
};
```

### 3.6 本地并发、槽位与池满（两种推理模式同一套规则）

**3.6.1 S。** `scheduler.localSlotLimit = loadedModel.slotCount() − 1`（现有 `AgentRunner::updateLocalSlotLimit`，不改）：子进程 = `-np` − 1，
进程内 = `n_seq` − 1（不含工作序列）。模型加载时按内存算好（§3.6.4），之后不变。

**3.6.2 名额与序列份额（team，`SubAgentScheduler`）。** 把 Swift 的「名额 = 正在算的子 Agent，内存 = 还占着 KV 的子 Agent」映射成：
- `running`：占名额的本地子 Agent（在生成或跑工具）。等下级 / 等确认 / 等收件箱（`waitingWithoutSlot`）时让出名额，**序列不还**。
- `alive`：已开跑、还没结束的本地子 Agent（含让出名额在等的）。结束时还。
- 放行判定（按顺序）：① `running == 0` → 放行（没有正在跑的就一定放行：嵌套多少层都不会互相卡死）；② 调试构建 `FRIDAY_LOCAL_SLOTS=n` 且
  `running ≥ n` → 否；③ `running ≥ S` → 否；④ **新开的**且 `alive ≥ S` → 否（拿回名额的不看这条，它自己有序列）；⑤ `poolLimit > 0 && running ≥ poolLimit` → 否；
  ⑥ `Platform::availableMemory() < localBudget + headroom` → 否（Swift 原条件，保留）；否则放行。
- 队列严格按 (priority 大, 先来) 排，只看队首，队首放不进就停。优先级：新开 = 层数；拿回（等完下级 / 确认 / 收件箱、池满重试）= `resumeBoost(10000) + 层数`。
- 无死锁：在等的不占名额；所有活着的都在等时 `running == 0`，队首一定放行。
- 接口（team 定稿，骨架只加 core 要用的 `waitForLocalEnd` 和常量）：

```cpp
class SubAgentScheduler {
public:
    static constexpr int resumeBoost = 10000;                        // （骨架加）
    bool canStartLocal() const;                                       // 新开的现在能不能直接跑
    bool acquireLocal(int priority, const CancelToken* token = CancelToken::current());   // 新开：running+1、alive+1；返回是否排过队
    void yieldLocal();                                                // 让出名额（running−1），放行队首
    bool resumeLocal(int priority, const CancelToken* token = CancelToken::current());    // 拿回名额（running+1，不看序列份额）
    void releaseLocal(bool holdingSlot);                              // 结束：alive−1（holdingSlot 时 running−1）；唤醒 waitForLocalEnd；
                                                                      // alive == 0 且没人排队时池满降档恢复
    bool retryLocalAfterPoolFull(int priority, const CancelToken* token = CancelToken::current());
    /// Qt 版（core 的 Q2 用）：等一个本地子 Agent 结束（releaseLocal）或超时；结束返回 true。取消抛 CancellationError（骨架加）
    bool waitForLocalEnd(int timeoutMs, const CancelToken* token = CancelToken::current());
    int localPoolLimit() const;
    void resetLocalPoolLimit();
    QString diagnostics() const;                                      // subagents.md §2.5 规则 8 的文字
    // 删除：acquireOnline / releaseOnline、旧的 acquireLocal(token) / releaseLocal() / retryLocalAfterPoolFull(bool holding, …)、poolFullWaitMs
};
```

**3.6.3 序列分配与主序列保护（team）。** 子 Agent 从开跑到结束借一个非主序列（`LoadedSlotLease`），让出名额时不还。借不到（`alive ≥ S` 时被规则①放行）：
- 进程内：`-1`，运行时只在空闲的非主序列里挑（`Scheduler::chooseSequence`，n_seq > 1 时绝不用主序列），都忙就排队；
- 子进程：**不再发 `-1`**（llama-server 的 `-1` 会挑到正等工具结果、空闲着的 0 号槽位）。改用 `LoadedModel::sharedSlot()`：最久没被借出的非主槽位的显式编号，
  忙就由 llama-server 排队；
- 只有一个序列（S == 0）时两种模式都只能用主序列（现状：进程内清掉主序列归属、下一轮从磁盘 / 前缀恢复；子进程 `-1`）。
- 看图（LocalVLMBackend）、起标题也经 `LoadedSlotLease`，自动得到同样的保护。
- 新接口：`int LoadedModel::sharedSlot() const;`、`int LlamaServer::sharedSlot();`（S == 0 或没加载返回 -1）；`LoadedSlotLease` 借不到时子进程模式取
  `sharedSlot()`、进程内仍是 -1。
- 残留（记录在案，不处理）：子进程模式下 llama-server 池满时会先清空闲槽位，可能包括正等工具结果的主槽位（服务端行为；下一段从 `--cache-ram` /
  分歧处重新预填）。进程内运行时腾空闲序列、选牺牲者都是非主优先。

**3.6.4 planContext（team，`ModelManager`）——去掉固定的 3。**
- `maxExtraSlots = 3` 改名 `baseExtraSlots = 3`（保底：内存够时至少这么多，不是上限）；新增 `maxExtraSlots = 32`（技术上限：每个序列有固定开销，
  进程内 n_seq_max 另含工作序列，远低于 llama.cpp 的 256）。
- 算法前三步不变（主对话 16k / 不够往下截；额外序列 = clamp(spare / (kv × 12000), 0, baseExtraSlots)、每个先占 12288；富余的一半依次给主对话到 32768、
  份额到 16384、主对话到上限）；**新增第 4 步**：主对话已到上限、份额已到 16384 时，用那一半里剩下的再加序列：
  `more = min(maxExtraSlots − extra, remaining / (kv × slotContext))`。另一半始终留给系统、计算缓冲和本地生图（不变）。
- 内存紧时结果与现在完全一样（第 4 步加 0）；现有 `TestLLM::planContext` 里 12 GiB 的 hybrid 例仍是 64k + 3 × 16384；40 GiB 的 roomy 例变成
  64k + 31 × 16384（team 改断言）。`conservativePlan`（异常退出后减半）不变。

**3.6.5 主 Agent 遇到池满（core，Q2）。** 主 Agent 每一次 `consume(stream)`（首段、验证提醒段、drain 段……）包一层：
1. 捕获 `KVCacheFullError`（只有本地引擎会抛）且不在停止中、这次 consume 里重发少于 2 次 →
2. 丢掉这一段已显示的半段文字（`discardMainPass()`：消息块截回本段起点、清空 pass 文字），诊断日志 framework 类
   「本机模型的上下文缓存满了：等子 Agent 让出后重发这一段（第 N 次）」，详情 `scheduler.diagnostics()`；
3. `scheduler.waitForLocalEnd(2000)`（有本地子 Agent 结束就提前返回；可取消）；
4. 原样重发：`engine.stream(QList<EngineMessage>(), on)`（失败的这一段没进引擎历史，接着最后一次成功的状态请求）；
5. 第 3 次仍失败 → 照旧抛出，走 `recover`（「遇到内部错误（…），已自动重建上下文并继续」）。

**3.6.6 安全上限（team，Q5）。** `checkDispatch` 在每轮总数检查之后：`team->runCount() + tasks.count > AgentTeamState::maxLiveRuns` →
`AgentToolError::failed`：「同时在跑的子 Agent 已经有 {runCount} 个了（Friday 的安全上限 1000），这次要派 {n} 个。先用已经拿到的结果收尾，或等它们结束后再派，没做完的自己做或在结果里说明。」
每轮总数上限开着（默认 100、最多 1000）时永远碰不到，与 Swift 行为相同。

### 3.7 引擎（`core/llm/engine/`）

```cpp
// ChatEngine.h（骨架加）
    /// 一段生成的工具调用全部执行完、发起下一段请求之前调用一次（引擎所在的工作线程）：返回要接在工具结果后面的消息。
    /// 主 Agent（用户的工作中补充 + 后台子 Agent 的汇报）与子 Agent（交给它的指示 + 它的后台下级的汇报）都设；空 = 不调
    /// （== Swift ChatEngine.interjection；Swift 注释里「子 Agent 不接收补充」已过时，按 v1.7 代码）
    std::function<QList<EngineMessage>()> interjection;

// OpenAIChatEngine.h（骨架加；include "llm/engine/RequestGate.h"）
public:
    /// 闸门排队优先级：主 Agent / 起标题 RequestGate::mainPriority，子 Agent = 层数（team 设）
    int requestPriority = RequestGate::mainPriority;
    /// 在闸门前开始 / 结束排队（引擎线程同步调用）；子 Agent 用它把卡片标「排队中」
    std::function<void(bool waiting)> onQueued;
    static constexpr int rateLimitGiveUpSeconds = 600;
    /// 第 rateLimits 次（1 起）可重试的 429 之后等多久：Retry-After 能按数字解析就用它（负数按 0），否则 min(30, 2^min(rateLimits, 5)) + jitterMs / 1000
    static double rateLimitDelaySeconds(int rateLimits, const QByteArray& retryAfter, int jitterMs);
protected:
    /// 用户消息统一处理（== Swift userObject）：userMessage(content, images, videos, supportsVision(), false)（骨架实现）
    QJsonObject userObject(const EngineMessage& message) const;
    /// 这个引擎用的并发闸门：RequestGate::gate(m_config.id, m_config.maxConcurrency)；空 = 不过闸门（team 实现；骨架返回空）
    virtual std::shared_ptr<RequestGate> requestGate() const;
    /// 429 退避的随机抖动毫秒（0–999；测试覆盖成 0）
    virtual int rateLimitJitterMs() const;

// LocalLlamaEngine.h（骨架加，定稿）
protected:
    /// 本地引擎不过闸门（Swift 的本地 MLX 根本不经过 OpenAIChatEngine）；429 / 502 / 503 / 504 / 529 仍按旧规则共用计数退避（最多 10 次）
    std::shared_ptr<RequestGate> requestGate() const override { return nullptr; }
```

`OpenAIChatEngine::run()` 的 interjection 调用点**由骨架实现**（主 Agent 和子 Agent 都要用、只有几行）：工具结果全部追加进 `m_history` 之后、`while`
下一次迭代之前，`if (interjection) { CancelToken::checkCurrent(); for (m : interjection()) m_history.append(m.role == User ? userObject(m) : convert(m, supportsVision())); }`；
收尾请求（wrapUp）、截断续写、`!keepGoing` 都不经过这里；钩子只在两次请求之间调用，绝不在 `transport()` 的 chunk 回调里（core-loop.md §2.6.2）。
`stream(const QList<EngineMessage>&)` 里用户消息改走 `userObject`（core，== 1b760a4）；`LocalLlamaEngine::stream(messages)` 的覆盖随之可删（core 定）。

```cpp
// core/llm/engine/RequestGate.h（新，归 team；头文件骨架写全，实现骨架空）——对应 Swift：LLM/Engine/RequestGate.swift
class RequestGate {
public:
    enum class Outcome { Ok, RateLimited, Failed };
    static constexpr int mainPriority = 1000;
    static constexpr int defaultInitial = 4;
    /// 按引擎 id 共用；第一次取时按 initial（nullopt → 4，至少 1）建，之后 initial 不再起作用
    static std::shared_ptr<RequestGate> gate(const QString& engineID, std::optional<int> initial);
    /// 拿一个名额；要排队时先 onQueue(true)，排到 / 取消时 onQueue(false)；排队时令牌取消抛 CancellationError
    void acquire(int priority, const std::function<void(bool)>& onQueue, const CancelToken* token = CancelToken::current());
    void release(Outcome outcome);
    struct Snapshot { int limit = 0; int inFlight = 0; int waiting = 0; };
    Snapshot snapshot() const;
    static bool isRetryableRateLimit(const QString& body);
};
```
`transport()` 的循环、计数、退避、日志：subagents.md §1.3.9、§3.5（那里的伪代码就是定稿）。主 Agent、起标题的在线请求也过闸门；看图 / 能力提供方
（OpenAIStyleAPI）不过。日志类别 `com.example.Friday.ratelimit`。

### 3.8 AgentProfiles / ToolRegistry（归 team；骨架改签名）

```cpp
class AgentTeamTools {
public:
    static QString delegateName();        // delegate_task
    static QString parallelName();        // run_parallel_tasks
    static QString readLogName();         // read_agent_log
    static QString messageAgentName();    // message_agent        （骨架加）
    static QString stopAgentName();       // stop_agent           （骨架加）
    static QString waitAgentsName();      // wait_agents          （骨架加）
    static QString reportToParentName();  // report_to_parent     （骨架加；只给子 Agent）
    static constexpr int deepLevel = 4;
    static constexpr double shrinkThreshold = 0.8;
    enum class Role { Main, Sub };
    // 删除 maxDepth（骨架删，并同步改全部用处，§6.D）
    static bool isTeamTool(const QString& name);                 // delegate / parallel
    static double similarity(const QString& a, const QString& b);  // Dice 系数（subagents.md §1.4.1；QChar：toLower、去 isSpace / isPunct / isSymbol、按 UCS-4 码点）
    static QList<ToolDescriptor> descriptors(const QList<EngineConfig>& engines, bool localAllowed);   // 7 个，顺序 subagents.md §1.3.3
    static QString splitGuidance();
    static QString guidance();
    static QString subGuidance();
    static QString subAgentPreamble(const QString& name, int depth);
};

// ToolRegistry.h
QList<ToolSpec> mainAgentSpecs() const;                   // 任意线程：activeSpecs 去掉 report_to_parent（骨架实现）
QString guidance(AgentTeamTools::Role team) const;        // 替换 guidance(bool)（骨架改签名：Main = 原来的 true，Sub = 原来的 false；team 改成 v1.8）
// preflight：report_to_parent 且 depth == 0 →「report_to_parent 只给子 Agent 用；你是主 Agent，直接回答用户即可。」（team）
```

### 3.9 AgentTree 纯函数与格式化（骨架实现；之后 AgentTree 归 team、Format 三个函数归 tree-tray）

```cpp
// core/agent/AgentTree.h（新）——对应 Swift：Views/AgentTreePanel.swift 的数据部分（:7-174）
struct AgentNode {
    ToolEvent event;
    int depth = 1;                 // 主 Agent 直接派的是 1；并行分组不算一层
    bool parallel = false;
    QList<AgentNode> children;
    QUuid id() const;
    QString name() const;          // agentName ?? "子 Agent"
    bool isRunning() const;        // status == Running
};
struct AgentTreeTurn { QUuid id; QString request; QList<AgentNode> nodes; };
struct AgentTreeStats { int running = 0, done = 0, stopped = 0, failed = 0, deepest = 0; int total() const; };
struct UsageSummary {
    int generated = 0; int prompt = 0; double speed = 0; int agents = 0; int running = 0;
    UsageSummary operator+(const UsageSummary& other) const;
};
struct AgentTreeRow { AgentNode node; int level = 0; QList<bool> guides; bool isLast = false; };

namespace AgentTree {
QList<AgentNode> build(const QList<MessageBlock>& blocks, int depth = 1, bool parallel = false);  // 派发被拒（没有运行记录）的不算节点
QList<AgentTreeTurn> turns(const QList<ChatMessage>& messages);   // 用户自己发的消息开新一轮（interjected / fromAgent 不算）；去掉空轮；新的在前
QList<AgentNode> all(const QList<AgentNode>& nodes);
QList<AgentNode> path(const QUuid& id, const QList<AgentNode>& nodes);
AgentTreeStats stats(const QList<AgentNode>& nodes);
QList<AgentTreeRow> flatten(const QList<AgentNode>& nodes, const QSet<QUuid>& collapsed, int level = 0, const QList<bool>& guides = {});
UsageSummary usage(const AgentNode& node, const AgentTeamState& team, const QDateTime& now);
/// (主 Agent, 全部子 Agent)；mainSpeed = AgentRunner::mainLiveSpeed；generating = 这段对话正在生成；子 Agent 结束的速度不计
std::pair<UsageSummary, UsageSummary> conversationUsage(const QList<ChatMessage>& messages, const AgentTeamState& team,
                                                        double mainSpeed, bool generating, const QDateTime& now);
/// approval（running 且嵌套待确认，或 awaitingApproval）| queued（running 且 queued）| running | succeeded | stopped（failed 且 stopped）| failed（其余 failed / denied）
QString statusKind(const ToolEvent& event);
/// 并行派发卡片收起时的小标签：build(transcript) 的顶层节点 → [{id（大写 UUID）, name, statusKind, model（usage.model，没有为 ""）}]
QVariantList chips(const QList<MessageBlock>& transcript);
QString conclusion(const QString& result);     // 结果第一行去空白，「结论：」开头则去掉
constexpr int autoOpenDepth = 3;
}

// core/util/Format.h（追加）+ app/viewmodels/Formatters.h 同名 Q_INVOKABLE 包装
static QString agentTokens(qint64 n);     // == AgentTree.compact：≥1,000,000「%.1fM」，≥1000「%.1fk」，否则原数（与「万 / 亿」的 compact 不同）
static QString agentSpeed(double speed);  // == AgentTree.speedText：≤0「— tok/s」，≥100「%.0f tok/s」，否则「%.1f tok/s」
static QString meterSpeed(double speed);  // == MenuBarLabel.speedText：0 t/s、8.5 t/s、179 t/s、1.2 Kt/s、3.4 Mt/s、1.1 Gt/s、1.1 Tt/s
```

### 3.10 线程池：等待不占池（team，Q5）

问题：`Worker` 池最多 256 线程；并行 / 后台子 Agent 各占一个池线程，父线程阻塞等子线程；层数和数量不限之后，池可能被「都在等」的线程占满，被等的任务排在池里拿不到线程 → 死锁。

定稿：**阻塞等待不占池名额。**
- `Promise::waitImpl` 真正要阻塞时（结果还没有、不是 0 超时），如果当前线程是 Worker 池线程（`Worker::run` 的 runnable 里设一个 thread_local 标记），
  就在等之前 `Worker::pool()->releaseThread()`、等完（含抛出）`reserveThread()`（RAII；可嵌套，只有最外层让出）。这是 Qt 文档给的「线程要睡下等活」的用法，
  池会临时多起线程。所有等待都经过它：审批、并行父级等子级、`waitForNotice`、调度器排队、闸门排队、`Worker::sleep`。
- `Worker::shutdown` 的等待条件改为 `activeThreadCount() + 让出中的个数 > 0`（让出时 activeThreadCount 少算 1，否则退出时会提前返回）。
- 实现位置：`util/Worker.{h,cpp}`（Promise 在 Worker 之前定义：用 Worker.cpp 里实现的两个自由函数 `detail::beginBlockingWait()` / `endBlockingWait()`）。
- 加上 §3.6.6 的安全上限，线程数有界（每轮上限开着时 ≤ 1000 + 少量）。
- 验收：`TestWorkerPool::blockedWaitersDoNotPinPool`（临时把池上限设成 2：两个任务各等第三个任务兑现的 Promise，必须在 2 秒内完成，测完恢复）、
  `TestWorkerPool::shutdownCountsReleased`、team 的压力用例（§7.5-H）。

### 3.11 界面层

**3.11.1 QML 单例。**
- `AgentRunner`（现有）：新属性 `isStopping / liveAgents / liveSpeed / queueRevision`，新方法见 §3.2、§3.3 的 QML 一节。
- `AgentTeam`（新）：`AgentRunner::team` 指向的 `AgentTeamState`，main.cpp 里 `registerSingleton("AgentTeam", core.runner->team)`（骨架）。
- `UiState`（新，`app/viewmodels/UiState.{h,cpp}`，QML_SINGLETON，骨架实现，之后归 tree-tray）：

```cpp
class UiState : public QObject {
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    /// == Swift @AppStorage("agentTreeVisible")，默认 false，settings.ini 同名键
    Q_PROPERTY(bool agentTreeVisible READ agentTreeVisible WRITE setAgentTreeVisible NOTIFY agentTreeVisibleChanged)
    /// == @AppStorage("agentTreeWidth")，默认 320，写入时夹到 260…560
    Q_PROPERTY(double agentTreeWidth READ agentTreeWidth WRITE setAgentTreeWidth NOTIFY agentTreeWidthChanged)
public:
    static constexpr double minimumTreeWidth = 260;
    static constexpr double maximumTreeWidth = 560;
    static constexpr double defaultTreeWidth = 320;
    explicit UiState(QObject* parent = nullptr);   // 读 SettingsFile::shared()
    // getter / setter / signals
};
```
- `Format`（现有 Formatters）追加 `agentTokens / agentSpeed / meterSpeed`。

**3.11.2 ChatViewModel（chat-ui）。**

```cpp
    /// 语义改：== runner.canSend(in:)（在线引擎存在 / 这段对话选的本地模型已就绪），不再 && !isGenerating；
    /// QML：canSend = hasContent && (vm.isGenerating ? !isEditing : vm.canSend)
    Q_PROPERTY(bool canSend READ canSend NOTIFY changed)
    /// 语义改：runner.canSend(id) || runner.isGenerating（生成中点建议也排队）
    Q_PROPERTY(bool suggestionsEnabled READ suggestionsEnabled NOTIFY changed)
    /// placeholderText / placeholder(editing) 的顺序：重新输入 → 这段在生成 → 别处在生成 → 原规则（ui.md §1.2）
    Q_PROPERTY(bool isStopping READ isStopping NOTIFY changed)                    // runner->isStopping()（全局）
    Q_PROPERTY(bool pendingModelChange READ pendingModelChange NOTIFY changed)
    Q_PROPERTY(bool pendingThinkingChange READ pendingThinkingChange NOTIFY changed)
    Q_PROPERTY(QString pendingChangeHelp READ pendingChangeHelp NOTIFY changed)   // 「Agent 这一轮还在用原来的…，下一轮自动换成现在选的」
    Q_PROPERTY(QVariantList queuedInputs READ queuedInputs NOTIFY queueChanged)  // [QueuedInput::toVariant]
    Q_PROPERTY(bool queuePaused READ queuePaused NOTIFY queueChanged)
    Q_PROPERTY(QString queueStatus READ queueStatus NOTIFY queueChanged)         // 五种状态文字（ui.md §1.2 QueuedBubbles）
    /// 骨架：原来的 send 改名为 submit（转给 runner->submit）
    Q_INVOKABLE void submit(const QString& text, const QStringList& attachments, const QString& replacing = QString());
    Q_INVOKABLE void removeQueued(const QString& itemID);
    Q_INVOKABLE void resumeQueue();
signals:
    void queueChanged();
```
停止键 / 排队键的提示在 QML 里拼（`App.shortcutModifier`），不进 view-model。连接：`stoppingChanged` → changed；`queuedInputsChanged(这段)` → queueChanged；
`queueStatus` 还依赖生成状态和 canSend，`generatingChanged` / 模型状态变化时也比较、变了发 queueChanged。

**3.11.3 MessageListModel（chat-ui）。** 新角色（都加进 messageChanged 的 dataChanged 角色表）：`StoppedByUserRole`「stoppedByUser」、
`InterjectedRole`「interjected」、`FromAgentRole`「fromAgent」、`AgentNoticeTitleRole`「agentNoticeTitle」（`AgentRunner::noticeTitle(第一行)`）、
`AgentNoticeBodyRole`「agentNoticeBody」（plainText 第一个 `\n` 之后）。新方法 `Q_INVOKABLE QVariantList childAgentsAt(int row, const QVariantList& path) const;`
= path 指向的工具块的 transcript 交给 `AgentTree::chips`。

**3.11.4 「消息提供者」接口（chat-ui 定义，tree-tray 实现第二份）。** `SubAgentTranscript` / `ToolCard` / `BlockView` 的 `messages` 属性只调用这四个方法，
`row` 的含义由提供者定（MessageListModel：可见窗口里的行；AgentTreeModel：**完整当前路径**里的消息下标）：

```cpp
Q_INVOKABLE QStringList blockIDsAt(int row, const QVariantList& path) const;
Q_INVOKABLE QVariantMap blockAt(int row, const QVariantList& path) const;     // MessageBlock::toVariant
Q_INVOKABLE int blockRevisionAt(int row, const QVariantList& path) const;
Q_INVOKABLE QVariantList childAgentsAt(int row, const QVariantList& path) const;   // AgentTree::chips
```
chat-ui 不得给这几个 QML 组件加别的 `messages.xxx` 调用（加了就要 tree-tray 同步实现，走协调者）。

**3.11.5 跨包使用的 QML 组件（骨架先放占位，属性接口以此为准）。**

| 组件（归属） | 属性 / 信号 | 用在 |
|---|---|---|
| `chat/SpinningRefreshIcon.qml`（chat-ui） | `size: 14`、`weight: Font.Bold`；无障碍名「正在停止」 | 输入框停止键、消息底栏、AgentControls |
| `chat/ModelPill.qml`（chat-ui） | `usage: ({})`（ToolEvent.toVariant().usage）；没有 `usage.model` 时不显示、宽高 0 | ToolCard 标题、AgentNodeCard、详情头卡 |
| `chat/AgentStatusIcon.qml`（chat-ui） | `kind: ""`（§3.9 statusKind）、`size: 14` | ChildAgentChips、AgentNodeCard、详情头卡 |
| `chat/AgentControls.qml`（chat-ui） | `eventID`、`name`、`showsInput: true` | ToolCard（showsInput false）、AgentDetailView（true） |
| `chat/SubAgentTranscript.qml`（chat-ui，现有） | 新增 `nesting: 0`、`deepens: true`（骨架先加属性） | ToolCard、AgentDetailView（`nesting: 2`） |
| `agenttree/AgentTreeColumn.qml`（tree-tray） | `tree: AgentTreeModel`；只读 `space`（面板占的宽度，含分隔条，动画中的当前值）、`panelActive`（面板已建） | ChatView |
| `agenttree/AgentTreeButton.qml`（tree-tray） | `tree`；是 `ToolbarButton`；`overflowText` | ChatView 工具栏（最右） |
| `agenttree/ConversationUsageBadge.qml`（tree-tray） | `tree`；`overflowText` | ChatView 工具栏（树按钮左边） |

**3.11.6 托盘（tree-tray，Q9；协调者已定）。** 图标不变、不画字、不加角标。`FridayApp`：
- tooltip 恒为三行 `"Friday\n" + liveAgents + " AI\n" + Format::meterSpeed(liveSpeed)`（空闲「Friday\n0 AI\n0 t/s」）；
- 托盘菜单状态行（「Friday 正在生成…」/ 模型状态）**下面紧接**一行禁用项「{n} AI 在工作 · {meterSpeed}」，只在 n > 0 时可见（`&` 转义成 `&&`）；
- 连 `AgentRunner::liveMeterChanged`（每秒最多一次、值不变不发）刷新；只改文字、菜单只建一次（开着的菜单不闪不关）；
- 给 DebugHarness：`QString trayToolTip() const`。

**3.11.7 工具栏溢出（tree-tray，Q10）。** `style/Toolbar.qml` 的溢出菜单项文字取 `overflowText`（有且非空时）> `text` > `help`；不是按钮、但有
`overflowText` 的项（用量徽标）收起时在「»」菜单里显示成一行禁用项。树按钮 `overflowText`「Agent 树」+（运行中 > 0 时「（运行中 N）」），点了切换面板；
徽标 `overflowText` = 「{速度} · 生成 {n} · 输入 {n}」（不在跑时没有速度那段）。树按钮声明在最右，最后才被收起。STYLE.md 的 Toolbar 一行同步写上。

**3.11.8 AgentTreeModel（`app/viewmodels/AgentTreeModel.{h,cpp}`，归 tree-tray）。** 定稿见 ui.md §3.9 的声明，另加：`Q_INVOKABLE QVariantMap node(nodeID)`、
`hasNode`、§3.11.4 的四个方法（row = 完整路径下标）。数据一律经 §3.9 的 AgentTree 函数（不在模型里另写 build / statusKind）。刷新：会话变化与
`AgentTeam.changed` 合并成 ≤ 6 Hz 一次重建（按消息 id + 修订号缓存每条消息的节点，流式时只重算正在写的那条）；`AgentTeam.clockChanged` 且（`active`
或这段在生成）时只刷运行中的行与合计；行差量插删 + dataChanged，不 `beginResetModel`；`setConversationID` 里同步算一次（`deepRunning` 的初值不能触发
「由假变真」）。骨架只放 `conversationID`、`active`、`rowCount() == 0` 与 §3.11.4 四个方法的空实现。

**3.11.9 ChatView 结构（骨架改一次，之后 ChatView.qml 归 chat-ui）。**
```qml
ChatViewModel { id: chatModel; conversationID: root.conversationID }
AgentTreeModel { id: agentTree; conversationID: root.conversationID; active: treeColumn.panelActive }
Item { id: chatArea; width: Math.max(0, root.width - treeColumn.space); height: root.height
    // 原来的 contentClip（消息列表 / 空状态）、ForkIndicator、ComposerView 原样搬进来，锚到 chatArea
}
AgentTreeColumn { id: treeColumn; x: chatArea.width; y: toolbar.height
    width: root.width - chatArea.width; height: root.height - toolbar.height; tree: agentTree }
Toolbar { id: toolbar; anchors.top: parent.top; width: parent.width; title: chatModel.title
    ConversationUsageBadge { tree: agentTree }
    AgentTreeButton { tree: agentTree } }
// send() 改调 chatModel.submit(…)；EmptyStateView 的 onPicked 改调 chatModel.submit(text, [], "")
```
面板和它左边的分隔条都从工具栏下沿开始（不穿过工具栏行，也不碰标题栏命中测试）。宽度动画、拖动、自动打开、接住 `treeFocus` 全在 `AgentTreeColumn` 里
（ui.md §2.7：动画照 Swift `.snappy`；长对话里卡就退成「对话区宽度一步到位、只滑面板 + 淡入」，门槛见 §7.5-F）。

### 3.12 服务一级设置（settings，d621837）

```cpp
// core/llm/OnlineServices.h
/// 服务一级的设置：这家服务的全部模型共用，在服务的「模型」面板里改（== Swift ServiceSettings）
struct ServiceSettings {
    QString title;
    QString baseURL;
    QString secretName;        // 密钥库里的名字；空 = 不需要密钥
    bool operator==(const ServiceSettings&) const = default;
};
class ServiceInstaller {
    // engines()：目录里不需要密钥的服务（非 custom、secretName 为空），接口地址改过也按显示名（title == 模板 title）认（== d621837）
    /// 这家服务的接口地址：已接入的看现有引擎，没接入的用目录默认值
    static QString baseURL(const OnlineService& service, const EngineStore* engines);
    static ServiceSettings settings(const OnlineService& service, const EngineStore* engines);
    /// 全部模型一起改；跟着它的能力提供方一起改（baseURL 在旧地址里才换；custom 另改标题与 apiKeySecret）；目录服务的名字与密钥名固定
    static void apply(const ServiceSettings& settings, const OnlineService& service, EngineStore* engines,
                      CapabilityRegistry* capabilities);
    // install / setModels：新加的引擎用这家服务当前的接口地址
};

// app/viewmodels/ServiceCatalog.h（QML）
/// {title, baseURL, secretName, defaultBaseURL, isCustom, needsSecret}
Q_INVOKABLE QVariantMap serviceSettings(const QString& serviceID) const;
/// == ServiceModelsSheet.save()：密钥名非法返回「密钥名只能用大写字母、数字、下划线，且以字母开头」且什么都不改；
/// 否则 setModels 再 apply，返回 ""
Q_INVOKABLE QString applyServiceSettings(const QString& serviceID, const QString& title, const QString& baseURL,
                                         const QString& secretName, const QStringList& models);
/// install 加 baseURL 参数（== AddServiceForm.install：接入后按表单里的地址 apply）
Q_INVOKABLE void install(const QString& serviceID, const QString& secret, const QStringList& models, const QString& baseURL);
```
界面：EngineEditor 去掉显示名 / 接口地址 / 密钥名三项、标题「{显示名} 的模型设置」、只在模型名为空时禁用保存、「起步并发 {n}（不设上限：没被限流就往上加，遇到 429 减半）」；
ServiceModelsSheet 加服务一级的一节（custom：显示名、接口地址、密钥名 + 是否在库的图标；目录服务：接口地址 + 不同于默认时「恢复默认」、只读密钥名）与 footer 文案、
`problem` 文字、保存条件 `!selection.isEmpty && !title.isEmpty && baseURL.startsWith("http")`；AddServiceForm 加「接口地址」（模板有引擎时）与接入条件；
「并发」相关文案全改「起步并发」（v1.7，ui.md §1.4「设置」）。逐字文案：`git show d621837`、ui.md §1.4。

### 3.13 调试钩子（只在调试构建）

| 名字 | 归属 | 说明 |
|---|---|---|
| `FRIDAY_AUTOSTOP=<条件>` | core（DebugHooks） | 条件满足后 `stop()`，打印 `FRIDAY_AUTOSTOP clicked` / `FRIDAY_AUTOSTOP stopped_in=%.2f`（core-loop.md §4.4） |
| `FRIDAY_SLOW_STOP=<秒>` | core（`AgentRunner::stop`） | 延后真正取消，审批立刻收尾 |
| `FRIDAY_QUEUE="<条件>\|文字\|…"` | core | 对正在生成的那段逐条 `submit` |
| `FRIDAY_SWITCH="<条件>\|engine:<id>\|thinking:<档>"` | core | 打印 `FRIDAY_SWITCH pending_model=… pending_thinking=…` |
| `FRIDAY_AGENT_MESSAGE="<条件>\|文字"` | core（调 team 的 `sendToAgent`） | `FRIDAY_AGENT_MESSAGE sent` |
| `FRIDAY_STOP_AGENT="<条件>[\|名字]"` | core（调 `stopAgent`） | `FRIDAY_STOP_AGENT stopped <名>` |
| `FRIDAY_TREE_FOCUS="<条件>\|<名字或「最深」>"` | core（写 `team->setTreeFocus`） | `FRIDAY_TREE_FOCUS <名>` |
| `FRIDAY_STALL_LIMIT=<秒>` | team（AgentLiveness） | 看门狗阈值，每次读 |
| `FRIDAY_LOCAL_SLOTS=<n>` | team（SubAgentScheduler） | 本地子 Agent 同时最多 n 个，只读一次 |
| `FRIDAY_SLOW_AGENT_STOP=<秒>`（Qt） | team（`stopAgent`） | 立刻记 stoppedBy（`isStopRequested` 为真），推迟这么久才取消令牌，截停止键旋转图标 |
| DebugHarness `drag:<选择器>::<dx>,<dy>`、`traytip`、以 `#` 开头的行跳过（Qt） | tree-tray | 拖分隔条、打印托盘 tooltip、脚本注释 |

条件语法（`approval` / `tool:<名>` / `text:<秒>` / `after:<秒>`）与等待规则：core-loop.md §4.4。`DebugHooks.h` 文件头列出全部名字（骨架加名字，core 维护）。

---

## 4. 决定记录

### 4.1 附录之间的分歧

| # | 分歧 | 决定 |
|---|---|---|
| D1 | AgentTeamState 的线程模型：subagents 内部加锁、任意线程；ui 写成只在主线程 | 内部加锁、任意线程，信号只在主线程合并发（T1）。子 Agent 线程要同步读写它（收件箱、等待、活动），只在主线程会处处 runOnMainSync |
| D2 | Activity 的写法：`Kind + title + tool()` vs `ActivityKind + tool 字段 + toolNamed()` | `ActivityKind` 枚举 + `title` 字段 + 工厂 `tool(title)`（字段与工厂不同名，又和 Swift `.tool(String)` 对应） |
| D3 | windowSpeed 的样本类型 | `AgentTeamState::Sample`；主 Agent 的速度窗口也换成它 |
| D4 | waitForNotice 带不带 token | 不带：用当前线程令牌（== Promise 的默认），不抛 |
| D5 | waitingWithoutSlot 是模板还是 std::function | `std::function<void()>`，实现放 .cpp；调用方按引用捕获结果。头文件里不放模板，core 与 team 都不必改对方的区 |
| D6 | team 怎么暴露给 QML / C++ | C++：公有常量成员 `AgentRunner::team`；QML：单例 `AgentTeam`；不设 `AgentRunner.team` 属性 |
| D7 | 计量归谁、信号叫什么 | core；`liveMeterChanged`；`AgentTeam.clock` 由 core 的计量每秒 `tickClock()` |
| D8 | 速度 / 用量文字函数 | `Format::agentTokens / agentSpeed / meterSpeed`（ui 的命名）；core-loop 的 `Format::agentSpeed`（= 菜单栏）改叫 `meterSpeed`；AgentTree 里不再有 compact / speedText / menuBarSpeedText |
| D9 | 汇报卡片标题 / 正文放哪 | MessageListModel 的角色（调 `AgentRunner::noticeTitle`）；ChatMessage::toVariant 只加三个原始字段 |
| D10 | agentMessageTool 的类型 | `static QString agentMessageTool()` |
| D11 | ChatViewModel 的名字 | ui 的：`queuedInputs / queuePaused / queueStatus`；不要 `queueButtonHelp / stopButtonHelp`（QML 拼）；`send` 改名 `submit` |
| D12 | AgentTreeModel 两份声明 | ui.md §3.9 为准（tree-tray 内部可再调）；statusKind 与小标签规则放 AgentTree 纯函数，两个模型共用 |
| D13 | 本地引擎怎么跳过闸门 | 虚函数 `requestGate()`，LocalLlamaEngine 覆盖返回空（骨架写好） |
| D14 | 原地打转归谁 | core（dispatch 与 `m_mainLastCall` 在 core）；其余活性归 team |
| D15 | cancelTeam 归谁 | core（Swift 在 AgentRunner.swift）；team 提供 `AgentTeamState::cancelRuns` |
| D16 | AgentUsage.temperature 缺省值 | 0（Swift 无缺省，构造时一定给）；`detail` 带 locale 参数 |
| D17 | `AgentRunner::conversationUsage`（subagents） | 删；AgentTreeModel 用 `AgentTree::conversationUsage` 算 |
| D18 | ChatView 的面板布局谁改 | 骨架改一次（chatArea + AgentTreeColumn + 工具栏两项），之后 ChatView 归 chat-ui、面板内部全在 tree-tray 的 AgentTreeColumn |
| D19 | Harness 的 `wait:stopping / wait:queued` | 不加，用现有 `until:` 表达式；DebugHarness 只由 tree-tray 改 |
| D20 | AgentTeam 的 QML 方法名（isRunningID / queueVariant / speedOf / passTokensOf） | 同名 QString 重载 `isRunning / queue / speed / passTokens`，加 `isStopRequested / hasActivity` |
| D21 | updateEvent 何时读这一轮的消息 | 创建闭包时（Q15） |
| D22 | 调度器接口 | subagents.md §3.4 + core 要的 `waitForLocalEnd`；waitingWithoutSlot 的模板版作废 |
| D23 | FakeChatServer / MockServer 的扩展放哪 | 骨架抽到 `tests/FakeChatServer.h`、`tests/MockServer.h`，归 team；core 只用现有功能 |
| D24 | Agent 树的 QML 放哪 | `qml/agenttree/`（类型名全模块唯一，不变） |

### 4.2 各附录 §5「风险 / 待定」逐条

| 出处 | 问题 | 决定 |
|---|---|---|
| core-loop §5-1 | 收尾闭包的重入顺序 | 采纳 T8；`stopAfterReentrantSend` 加断言「重入的新一轮 `isStopping()` 一开始为 false」 |
| core-loop §5-2 | `stop()` 不在生成时无效、只认第一次 | 采纳；现有用例按 core-loop.md §4.3 改 |
| core-loop §5-3 | 子 Agent 流式刷新只认正在写的那条 | 采纳：team 的 SubRun 节流刷新改用 `eventTask` |
| core-loop §5-4 | 插入补充后自愈重发了这一轮开头的 prompt | **照搬**，列入 §8 |
| core-loop §5-5 | `requirementMessages` 会抓到汇报卡片 | **照搬**，列入 §8 |
| core-loop §5-6 | 子进程模式 KV 池满连累主 Agent | Q2（§3.6.5）+ Q3（§3.6.3） |
| core-loop §5-7 | 工作中改思考强度会经后续 configure 进旧引擎 | 照搬 Swift |
| core-loop §5-8 | 在线引擎停止时留下的半截文字 | 保留现状（主 Agent 下一轮按存档重建，不依赖它） |
| core-loop §5-9 | detail 的小数点 | Q14 |
| core-loop §5-10 / subagents §5-11 / ui Q1、Q4 | 托盘两行字 | §3.11.6（tooltip 三行、菜单行、不加角标；第一行保留「Friday」） |
| core-loop §5-11 | `\|\|` 短路导致多同步一次 | 照搬；`markResync` 两个原因都有时按 Fold |
| core-loop §5-12 | 并行父卡片停止后的状态 | Q1 |
| core-loop §5-13 | 忙 / 有暂停队列时「重新输入」被静默忽略 | 照搬 |
| core-loop §5-14 | 排队投递最多轮询 20 分钟 | 照搬 |
| core-loop §5-15 | 计量与速度窗口的锁 | T7；临界区只做 append / 删旧 / 复制 |
| core-loop §5-16 | CLI 跟消息 | core 做：连 `turnMessageStarted` 换跟随的消息，新出现的汇报卡片 / 补充打印一行灰字（标题用 `noticeTitle`）；`--json` 另发一条 message 事件（格式 core 定，写进 ARCHITECTURE §11） |
| subagents §5-1 | 本地上限 3 太小 | §3.6.4（保底 3 + 富余再加，技术上限 32） |
| subagents §5-2 | `-1` 挤掉主槽位 | Q3 |
| subagents §5-3 | 线程池死锁 | §3.10 + §3.6.6 |
| subagents §5-4 | 信箱与终态不回退的竞态 | 采纳；必测：后台孙子结束时父级正阻塞、父级被停的同时下级结束、`markStopped` 之后迟到的发布（§7.1 TestAgentTeam::mailboxAndTerminal） |
| subagents §5-5 | 看门狗误停（后端排队、超长预填） | 照搬 Swift（只有「生成中」长时间无输出才算）；Q6 只多算 Progress；名额规则让后端很少排队 |
| subagents §5-6 | `Retry-After: 0` 永不放弃 | 照搬（按请求的等待秒数累计 10 分钟）；需要时两边一起改 |
| subagents §5-7 | 起步并发只在第一次建闸门时生效 | 照搬 |
| subagents §5-8 | 后台卡片的 result 结束后被覆盖 | 照搬 |
| subagents §5-9 | 依赖主循环 | 接口定死在 §3.2，按 §5.0 的合并顺序验 |
| subagents §5-10 | reservedToolNames 只保留两个 | 照搬 |
| subagents §5-12 | 开始 / 结束时间不持久化 | 照搬，不加键 |
| subagents §5-13 | similarity 的字符类 | QChar：`toLower`，去掉 `isSpace / isPunct / isSymbol`，按 UCS-4 码点 |
| subagents §5-14 | ChatEngine.swift 注释过时 | 按代码（子 Agent 也设 interjection），Qt 注释写对 |
| subagents §5-15 | 墙上时间 | 照搬 |
| subagents §5-16 | 看图 / 起标题也借序列 | 接受；`alive` 只数子 Agent |
| ui §5-1 | 树的性能 | 采纳为验收项（§3.11.8、§7.5-F） |
| ui §5-2 / Q2 | 面板开关时对话区宽度动画 | 先照 Swift 做动画；达不到 §7.5-F 门槛就退成一步到位 + 只滑面板（协调者已定） |
| ui §5-3 | 详情页「过程」 | AgentTreeModel 自己实现消息提供者（完整路径下标）；块修订号逻辑先拷 MessageListModel 的一份，以后再抽公共类（记为后续，不在本次） |
| ui §5-4 | 标题栏 / 工具栏溢出 | §3.11.7 |
| ui §5-5 | ListView footer 与空会话 | 采纳；空会话里排队不显示（Swift 原样） |
| ui §5-6 | 线程 | T1 |
| ui §5-7 | 委托内状态丢失 | Q11 |
| ui §5-8 | 自动打开的初值 | §3.11.8（同步首算） |
| ui §5-9 | 生成中换本地模型（进程内） | core 保证 `prepareModel` 生成中不动；真模型场景 R5 验 |
| ui §5-10 | GNOME 不显示 tooltip、计数是全局的 | 接受（菜单行兜底；Swift 同样是全局计数） |
| ui §5-11 | maxDepth、toVariant 键与角色要同一提交 | §6 骨架 |
| ui §5-12 / Q6 | d696a44 / d621837 | 都做：chat-ui / settings |
| ui Q3 | 分隔线穿不穿过工具栏行 | 不穿（协调者已定） |
| ui Q5 | Qt 专用钩子 | 接受（Q12） |

---

## 5. 工作包

### 5.0 通则与合并顺序

- 五个包都从骨架提交（§6）拉分支并行做；各包只改 §5.x「独占」里的文件和「共用」里写明的区 / 函数。骨架改过、下面没列给任何包的文件
  （`AppSettings.{h,cpp}`、`src/app/main.cpp`、图标生成物）这次不再改；真要改找协调者。`tests/fixtures/conversations-v19.json` 骨架之后归 core，
  界面包只拷贝使用（要别的状态就各自另加 fixture 文件，命名 `conversations-v19-<包>.json`）。
- 每个包交付时：`friday` / `friday-cli` / `friday_tests` / `friday_header_check` 能编（Debug），自己的测试全过、其余测试不比骨架时更差；
  交付总结列出：做了什么、Qt 版差异（§2.4 的哪几条）、核对过的文案表、没做完 / 有疑问的地方。
- 合并顺序（每个包合并前先 rebase 到当时的分支头，只解决自己文件里的冲突，然后跑全量测试）：
  **settings → core → team → chat-ui → tree-tray**。team 的「后台子 Agent 汇报接进主 Agent」类用例依赖 core 的 `takeInterjection / drainBackgroundAgents`，
  在 core 合入后必须全过；chat-ui / tree-tray 的实时截图在 core + team 合入后补拍（之前用 §6.E 的 fixture 拍静态状态）。
- 提交：中文一行说明；作者按 CLAUDE.md（仓库本地 git config），不加 AI 署名。

### 5.1 core（主 Agent 循环）

**独占**：`src/core/agent/AgentRunner.cpp`；`AgentRunner.h`（team 区以外）；`QueuedInput.h`；`LocalKVSession.cpp`；`TurnCheck.{h,cpp}`；`AgentTools.{h,cpp}`；
`DebugHooks.{h,cpp}`；`src/core/util/ProcessRunner.{h,cpp}`；`src/core/models/ChatModels.{h,cpp}`（骨架之后）；`src/core/llm/engine/ChatEngine.{h,cpp}`；
`LocalLlamaEngine.{h,cpp}`（`requestGate` 覆盖除外）；`src/cli/**`。
**共用**：`OpenAIChatEngine.cpp` 里的 `run`（骨架已加钩子，core 可调整）、两个 `stream`、`userObject`；`OpenAIChatEngine.h` 里这些函数的声明与注释。
**测试**：`tests/tst_runner.cpp`（骨架搬走 team 的用例之后剩下的 TestRunner）、`tst_models.cpp`、`tst_engines.cpp`、`tst_context.cpp`、`tst_util.cpp`、`tst_inproc.cpp`；
测试辅助 `tests/AgentTestSupport.h`、`tests/EngineTestSupport.h`、`tests/InProcTestSupport.h`（`FakeChatServer.h`、`MockServer.h` 只读用，要加功能找 team）；
`tests/fixtures/conversations-v19.json`。
**提供**：§3.2 全部；`MessageBlock::markStopped`；`isStopping()`；`eventTask / updateEvent / currentTurnMessages / turnMessageIDs`；`estimateTokens`；
`mainLiveSpeed`；`noteRepeat`；计量与 `tickClock` 调用；`turnMessageStarted`；CLI。
**依赖**：骨架里 team 的声明（`AgentTeamState`、`waitingWithoutSlot`、`SubAgentEnded`、`noticeText`、`scheduler.waitForLocalEnd`、`mainAgentSpecs`、`guidance(Role)`）。
**任务**：core-loop.md §2.2–§2.5、§2.8.1（markStopped）、§2.9–§2.12；§3.6.5（Q2）；Q13、Q15；按 core-loop.md §2.15 的顺序做，每步能单独编、跑测试。
**测试**：§7.1、§7.2 标 core 的全部；改 core-loop.md §4.3 的现有用例。
**文档**：ARCHITECTURE.md §3.4（回合线程新增的 takeInterjection / drainBackgroundAgents、收尾顺序、m_turnMessageIDs）、§11（CLI）；`DebugHooks.h` 文件头。

### 5.2 team（子 Agent 系统）

**独占**：新文件 `src/core/agent/AgentTeamState.{h,cpp}`、`AgentTeamControl.cpp`、`AgentLiveness.{h,cpp}`、`AgentTree.{h,cpp}`、`src/core/llm/engine/RequestGate.{h,cpp}`；
`AgentProfiles.{h,cpp}`、`SubAgentRunner.{h,cpp}`、`SubAgentScheduler.{h,cpp}`、`ToolRegistry.{h,cpp}`、`ToolTypes.{h,cpp}`；`src/core/llm/engine/EngineStore.{h,cpp}`；
`src/core/llm/LoadedModel.{h,cpp}`、`LlamaServer.{h,cpp}`、`ModelManager.{h,cpp}`（planContext 一处）；`src/core/util/Worker.{h,cpp}`；
`Friday/Tools/Perf/mock_sse.py` 与 `Friday/Tools/Perf/README.md`（只加 Qt 截图要的开关，不改已有行为）。
**共用**：`AgentRunner.h` 的两段 team 区；`OpenAIChatEngine.cpp` 里的 `transport`、`requestGate`、`rateLimitDelaySeconds`、`rateLimitJitterMs` 与 transport 用的私有辅助；
`OpenAIChatEngine.h` 里这些成员的声明（骨架已加）。
**测试**：新 `tests/tst_team.cpp`、`tst_team_runner.cpp`（骨架从 tst_runner 搬来的用例 + 新端到端）、`tst_team_inproc.cpp`（骨架从 tst_inproc 搬来的两个 + 新的）、
`tst_request_gate.cpp`（骨架搬来的 retryBackoff + 新的）；`tests/FakeChatServer.h`、`tests/MockServer.h`（照 `mock_sse.py` 加的假服务功能都在这两个里）；
`tst_registry.cpp`（这次整份归 team）；`tst_llm.cpp` 的 `planContext`、`planLaunch` 两个函数。
**提供**：§3.3–§3.6、§3.7 的闸门部分、§3.8、§3.10；AgentTree 维护（骨架实现）；`mock_sse.py` 新开关 `nest`（子 Agent 不到这一层就再派两个下级）、
`background`（主 Agent 派发带 `background: true`）、`report`（子 Agent 先 `report_to_parent` 一次）、`sub_approval`（子 Agent 调一次要确认的调用）。
**依赖**：core 的 `isStopping`、`eventTask`、`currentTurnMessages`、`turnMessageIDs`、`estimateTokens`、`dispatch`（含 waitingWithoutSlot 调用点）、`takeInterjection`
（汇报接进主 Agent）、`markStopped`（清 queued）。
**任务**：subagents.md §2.1–§2.8 全部（被本文取代的除外）；Q1、Q3–Q8、Q12（钩子部分）；§3.6.4、§3.6.6。
**测试**：§7.1、§7.2 标 team 的全部；subagents.md §4.2 的 1–15、§4.3。
**文档**：ARCHITECTURE.md §3.5（AgentTeamState、RequestGate、SubAgentScheduler 三行）；BACKENDS.md（v2.0 已删除）§2.10（名额 / 序列规则、sharedSlot、主 Agent 池满重发 Q2 的说明）；
LOCAL_INFERENCE.md（v2.0 已删除）§A.3、§E.4 的相关句子；`mock_sse.py` 的开关写进 `Friday/Tools/Perf/README.md`。

### 5.3 chat-ui（对话界面）

**独占**：`src/app/qml/chat/` 下 `ChatView.qml`（骨架之后）、`ComposerView.qml`、`MessageList.qml`、`MessageRow.qml`、`ToolCard.qml`、`SubAgentTranscript.qml`、
`BlockView.qml`、`ModelStatusView.qml`、`ThinkingChip.qml`、`PermissionChip.qml`、`WorkingDirectoryChip.qml`、`EmptyStateView.qml`；新 `SpinningRefreshIcon.qml`、
`DashedOutline.qml`、`QueuedBubbles.qml`、`AgentControls.qml`、`ChildAgentChips.qml`、`ModelPill.qml`、`AgentStatusIcon.qml`
（其中 SpinningRefreshIcon、AgentControls、ModelPill、AgentStatusIcon 骨架给占位，属性接口不能改，§3.11.5）；
`src/app/viewmodels/ChatViewModel.{h,cpp}`、`MessageListModel.{h,cpp}`。
**测试**：新 `tests/tst_chat_ui.cpp`——只测 core 层可链接的部分（`AgentTree::chips` 的界面用法、noticeTitle 拆分规则等纯函数）；view-model 与 QML 用 harness 验。
**Harness**：`qt/tools/harness/chat-stop.txt`、`chat-queue.txt`、`chat-agents.txt`、`chat-composer.txt`（§7.3）。
**提供**：§3.11.2–§3.11.5 中归 chat-ui 的部分；d696a44。
**依赖**：骨架的数据字段与 `AgentRunner` / `AgentTeam` 的 QML 接口；实时状态依赖 core / team 合入。
**任务**：ui.md §2.2–§2.5（v1.5–v1.8 对话里的全部）；d696a44（§1.6：ThinkingChip、ModelStatusView、PermissionChip、WorkingDirectoryChip、ComposerView 的空白分配、
ChatViewModel 的 `modelHelpText`「在线引擎 %1 · %2（%3）」与本地状态文字等宽数字）；Q11。图标只用骨架生成好的（缺了找协调者）。
**文档**：无（新组件在 `qml/chat/`，不是设计系统组件，文件头写清「对应 Swift：…」即可；STYLE.md 这次只由 tree-tray 改）。

### 5.4 tree-tray（Agent 树、工具栏合计、托盘）

**独占**：新 `src/app/qml/agenttree/`（`AgentTreeColumn.qml`、`AgentTreeButton.qml`、`ConversationUsageBadge.qml` 三个骨架给占位，另有 `AgentTreePanel.qml`、
`AgentPanelSummary.qml`、`AgentTreeRow.qml`、`AgentNodeCard.qml`、`AgentDetailView.qml`、`PanelResizeHandle.qml`）；`src/app/qml/ContentView.qml`；
`src/app/style/Toolbar.qml`；`src/app/FridayApp.{h,cpp}`（骨架删 agentTeamMaxDepth 之后）；`src/app/DebugHarness.{h,cpp}`；
`src/app/viewmodels/AgentTreeModel.{h,cpp}`、`UiState.{h,cpp}`、`Formatters.h`（骨架之后）。
**共用**：`src/core/util/Format.{h,cpp}` 里 `agentTokens / agentSpeed / meterSpeed` 三个函数（骨架已实现，tree-tray 只修 bug）。
**测试**：新 `tests/tst_tree_tray.cpp`（Format 三个函数的测试向量、`AgentTree` 在树面板里的用法：用 §6.E 的 fixture 算 turns / stats / flatten / usage 字符串）。
**Harness**：`qt/tools/harness/tree-panel.txt`、`tree-toolbar.txt`、`tray.txt`。
**提供**：§3.11.6–§3.11.9 中归 tree-tray 的部分；Q9、Q10；DebugHarness 的 `drag:`、`traytip`、`#` 注释行。
**依赖**：AgentTree（骨架实现）、AgentTeam、`AgentRunner.liveAgents / liveSpeed / mainLiveSpeed`、chat-ui 的 ModelPill / AgentStatusIcon / AgentControls / SubAgentTranscript
（按 §3.11.5 的接口用，占位期间显示为空）。
**任务**：ui.md §2.6、§2.7（被 §3.11.9 改写的部分以本文为准）、§2.9、§4.3 的 harness 部分。
**文档**：STYLE.md（Toolbar 的 `overflowText`；托盘一节；§9 验证里补 `drag:` / `traytip` / `#` 注释与 `qt/tools/harness/` 的用法）。

### 5.5 settings（设置页与服务一级设置）

**独占**：`src/app/qml/settings/AgentsSettingsView.qml`、`AgentSettings.qml`（这次 Swift 没改它，列在这里只为归属清楚）、`EngineEditor.qml`、`ServiceModelsSheet.qml`、
`ServiceModelPicker.qml`、`AddServiceForm.qml`、`SecretsSettingsView.qml`、`ServiceGallerySheet.qml`；`src/app/viewmodels/ServiceCatalog.{h,cpp}`、`ModelMenuBuilder.{h,cpp}`；`src/core/llm/OnlineServices.{h,cpp}`。
**测试**：新 `tests/tst_services.cpp`（骨架把 tst_engines 的 `onlineServicesCatalog / onlineServicesCustom / serviceInstaller` 搬进来）。
**Harness**：`qt/tools/harness/settings-agents.txt`、`settings-services.txt`、`settings-model-menu.txt`。
**提供**：§3.12；v1.7 / v1.8 的设置文案（「起步并发」全部位置：EngineEditor、ServiceModelPicker 行尾与「高级」提示、ServiceModelsSheet footer、SecretsSettingsView
「模型」按钮提示、ModelMenuBuilder 的在线模型副标题）；AgentsSettingsView 的「限制每轮派发的子 Agent 总数」开关 + 「最多 N 个」步进（10…1000，步长 10）。
**依赖**：`AppSettings.subAgentTurnLimit`（骨架）。与其他包没有运行时依赖，最先合并。
**任务**：ui.md §2.8 + §1.4「设置」；§1.7 / §3.12（d621837）。
**文档**：ARCHITECTURE.md §8.3 里 ServiceCatalog 一行补新方法。

### 5.6 协调者的集成步骤（不是工作包）

五个包都合入后：`qt/README.md` 功能列表（多 Agent 一段改「层数不限…」，新增「父子 Agent 协作」「速度和用量」「托盘计量」「Agent 树」「子 Agent 一直不回来」
「工作中补充」「停止」各段，按 Swift README 的 v1.5–v1.9 改动对应改写，托盘一段按 §3.11.6）；全量测试 + §7.4 真模型场景 + 全部 harness 截图复核；
ARCHITECTURE.md §12 测试一节补新测试文件。

---

## 6. 骨架提交（一个人在主工作树里做，五个包开工前）

**前提**：工作树干净（进程内推理的工作已提交）。骨架不改任何运行时行为，唯一例外是删 `maxDepth` 带来的「层数不限」（§6.D）。

**A. 测试辅助抽取与用例搬家（纯搬动，不改断言，除 §6.D 列的）**
1. `tests/AgentTestSupport.h`：tst_runner.cpp 匿名命名空间里的 `thrownMessage / text / makeCall / makeEvent / makeImage / FakeSink / findTool / logNote`，
   再加 `struct AgentRig`（= 现在 `TestRunner::initTestCase` 建的那一整套对象 + `newConversation / addFakeEngine / ensureRunShellEnabled / tearDown`）。
2. `tests/FakeChatServer.h`：`FakeChatServer`（归 team）。
3. `tests/MockServer.h`：tst_engines.cpp 里的 `MockServer`（归 team）；`tests/EngineTestSupport.h`：其余的 `ProbeEngine / ProbeLocalEngine` 与辅助函数（归 core，包含 MockServer.h）。
4. `tests/InProcTestSupport.h`：tst_inproc.cpp 匿名命名空间里的 `EnvGuard / InProcessScope / RuntimeProbe / Recorder / LocalCore` 与辅助函数。
   （头文件内容放在匿名命名空间里；同名的辅助（如 `sseFixture`）去重到一处，保证几个头文件能同时被包含。）
5. 搬家：TestRunner 的 `subRunMerge、subRunLocalBlocks、teamToolValidation、subAgentEngineChoice、parallelSubAgents、parallelChildFinishesEarly、parallelStopped、
   delegateSubAgent、nestedDelegateSharesLease、taskStateForSubAgents、readAgentLogTool、subAgentBackground` → `tests/tst_team_runner.cpp`（类 `TestTeamRunner`）；
   整个 `TestSubAgentScheduling` → `tests/tst_team.cpp`；TestEngines 的 `retryBackoff` → `tests/tst_request_gate.cpp`（类 `TestRequestGate`）；
   TestEngines 的 `onlineServicesCatalog、onlineServicesCustom、serviceInstaller` → `tests/tst_services.cpp`（类 `TestServices`）；
   TestInProcess 的 `parallelSubAgentsInProcess、subAgentPoolFullRetriesInProcess` → `tests/tst_team_inproc.cpp`（类 `TestTeamInProcess`）。
   新建空的 `tests/tst_chat_ui.cpp`（`TestChatUi`）、`tests/tst_tree_tray.cpp`（`TestTreeTray`），各放一个 smoke 用例。（tests/ 用 GLOB，不改 CMake。）
   若届时进程内推理的工作还在改 tst_inproc.cpp：第 4 步与 tst_inproc 的搬家先不做，team 的进程内用例临时放在 tst_inproc.cpp 末尾的独立类里、只追加。

**B. 骨架实现的部分（多个包都要用的数据与纯函数，写完整）**
- §3.1 全部（`markStopped` 除外：空实现）：AgentUsage、ToolEvent / ChatMessage 新字段的 JSON / toVariant / `==`、`ToolEvent::isFinished`、`QueuedInput.h`、
  `ToolOutput::runsInBackground`、`AppSettings::subAgentTurnLimit`（+ ARCHITECTURE §5.4 一行）。
- `SubAgentEnded`（§3.5）；`AgentRunner::agentMessageTool / agentMessageNote / noticeText / noticeTitle / duration / stoppedNote / interjectionNote`；
  `AgentTeamState::mainKey / windowSpeed / tickClock / clock`。
- §3.9 全部（AgentTree、Format 三个函数 + Formatters 包装）。
- `UiState`（§3.11.1）。
- `ToolRegistry::mainAgentSpecs()`；`OpenAIChatEngine::userObject()` 与 `run()` 里的 interjection 调用点；`LocalLlamaEngine::requestGate()` 覆盖。
- main.cpp 注册 `AgentTeam` 单例。

**C. 声明 + 空实现（行为 = 现在的行为）**
- `AgentRunner.h`：§3.2、§3.3 全部声明，两段 team 区的注释围栏；构造函数里 `team(new AgentTeamState(this))`。空实现：`isStopping()` 读原子量（`stop()` 暂不设它）；
  `submit` = 现在 `send` 的前置条件满足就 send、否则忽略；`removeQueued / resumeQueue` 空；`queuedInputs` 空列表；`isQueuePaused` false；`queueRevision` 0；
  `engineKey` 按规则算（纯）；`pendingTurnChanges` 全 false；`takeInterjection` 空列表；`turnMessageIDs / currentTurnMessages` 只有正在写的那条；
  `eventTask` = 现在 `messageTask(eventBody(…))` 的行为；`noteRepeat` 0；`pollingTools` 三个名字；`liveAgents` 0、`liveSpeed` 0、`mainLiveSpeed` 0；
  `estimateTokens(chunk, local)` 写全（现有成员函数改调它）。team 区放进新文件 `AgentTeamControl.cpp` / `AgentLiveness.cpp`：`stopAgent / sendToAgent` false、
  `removeAgentQueued` 空、`runningChild` nullopt、`takeAgentInterjection` 空、`checkDispatch` 空、`ancestryNote` nullopt、`waitingWithoutSlot` 直接 `body()`、
  `updateAgentEvent` = `updateEvent`、`stallLimitSeconds` 300、`agentStatusText` nullopt / ""、`startWatchdog` 空、`childDeadline` nullopt、`withReports` 原样、
  `partialResult` nullopt、`endedError` 原样、`setLivenessTimingForTests` 空、`parentKey / parentName` 按规则写（简单）。
- `SubRun`：`steps()`、`lastCall`。
- `AgentTeamState.{h,cpp}`：§3.4 全部声明；除 B 列的以外全是空实现（没有 run、通知永远为空、`waitForNotice` 立即返回、`speed` 0、`isStopRequested` false）。
- `AgentLiveness.h`（SubAgentEnded）、`RequestGate.{h,cpp}`（`gate` 按 id 共用一个空闸门，`acquire` 立即返回、`release` 空、`isRetryableRateLimit` true）。
- `SubAgentScheduler`：只加 `resumeBoost` 与 `waitForLocalEnd`（立即返回 false）；旧接口不动（team 删）。
- `OpenAIChatEngine.h`：§3.7 的成员；`requestGate()` 空实现返回 nullptr、`rateLimitJitterMs()` 0–999 随机、`rateLimitDelaySeconds` 按 §3.7 写全（纯）。
- `ChatEngine.h`：`interjection` 成员。`AgentProfiles.h`：四个新名字、`deepLevel / shrinkThreshold / Role`、`similarity / splitGuidance / subGuidance` 空实现（0 / ""）。
  `ToolRegistry`：`guidance(Role)`。
- `ChatViewModel`：`send` 改名 `submit`（转 `runner->submit`）。`AgentTreeModel`：`conversationID`、`active`、`rowCount() == 0`、§3.11.4 四个方法空实现。
- QML 占位：§3.11.5 表里的 7 个组件（chat-ui 的四个 + tree-tray 的三个），只有属性、宽高 0、不显示；`SubAgentTranscript.qml` 加 `nesting / deepens` 两个属性。
- ChatView.qml 按 §3.11.9 改结构（send → submit、EmptyStateView → submit）。

**D. 必须在同一提交里做的删除 / 改名**
- 删 `AgentTeamTools::maxDepth`，同步改：`FridayApp` 删 `agentTeamMaxDepth` 属性与函数；`AgentsSettingsView.qml` 的 footer 换成 v1.8 的定稿文字（ui.md §1.4「设置」，
  不再插值）；`ToolRegistry.cpp` 的诊断文字换成 v1.8 定稿（subagents.md §1.8）；`SubAgentRunner.cpp` 去掉「已经是第 N 层」检查与按深度去掉派发工具；
  `AgentProfiles.cpp` 的 preamble 里 `maxDepth` 先换成 `deepLevel`（team 重写全文）；`tst_registry.cpp` 删 `maxDepth == 2` 的断言、搬家后的 `teamToolValidation`
  删「已经是第 2 层…」那条、「第 2 层子 Agent 没有多 Agent 工具」类断言删掉（team 按 v1.8 补新断言）。
- `ToolRegistry::guidance(bool)` → `guidance(AgentTeamTools::Role)`：同步改 `AgentRunner.cpp`（Main）、`SubAgentRunner.cpp`（Sub）、`tst_registry.cpp` 的 `guidanceSections`。
- `ChatViewModel::send` → `submit`：同步改 QML 里的调用。

**E. 数据、图标、文档**
- `tests/fixtures/conversations-v19.json`：Swift 格式（同现有 fixtures），至少含：被停止的助手消息（`stoppedByUser`、`run_shell` 卡 failed + stopped「已停止」、
  审批卡「等待确认时被停止，没有执行」、空回复只有底栏）；`interjected` 用户消息；`fromAgent` 汇报卡片（完成、中途汇报、被停止三种第一行）；
  `run_parallel_tasks`「并行 ×2」卡（两个子卡带 `usage`，其一嵌套 delegate 到第 4 层、其一 failed + stopped「被用户停止了，没做完。…」）；一个派发被拒、没有 transcript 的
  delegate 卡；第二轮的一棵树（测「本轮 / 第 1 轮」）。core 的 JSON 测试、tree-tray / chat-ui 的静态截图都用它（`FRIDAY_DATA_DIR` 指到拷贝）。
- 图标：`style/icons/mapping.json` 加 ui.md §2.10 的 9 个（`stop.circle`、`stop.circle.fill`、`text.bubble`、`waveform.path.ecg`、`point.3.filled.connected.trianglepath.dotted`、
  `arrow.up.forward`、`arrow.down.backward`、`arrow.up.circle.fill`、`arrow.up.message`），`LUCIDE_ICONS=<lucide-static 同版本的 icons 目录> python qt/tools/gen_icons.py`，
  提交生成物（regular / semibold / bold 三份 SVG 与 IconNames.js），`.fill` 的目视确认。
- 文档：本文 → `qt/docs/PORT_V15_V19.md`；三份附录 → `qt/docs/port/`；ARCHITECTURE.md §5.3（新键）、§5.4（subAgentTurnLimit、agentTreeVisible、agentTreeWidth）、
  §8.2（AgentRunner 新属性 / 方法、AgentTeam 一行）、§8.3（UiState、AgentTreeModel 两行）、§9（新文件对照：QueuedInput、AgentTeamControl、AgentLiveness、
  AgentTeamState、AgentTree、RequestGate、AgentTreeModel、UiState、新 QML）；`DebugHooks.h` 文件头列出 §3.13 的名字。

**F. 骨架的验收**：Debug 构建 `friday`、`friday-cli`、`friday_tests`、`friday_header_check` 全过；`qt/ci/run_tests.ps1` 全过（只有 §6.D 删掉的断言不同）；
`FRIDAY_SCREENSHOT_SCRIPT='main;settings:agents'` 浅 / 深色 QML 警告 0 条、没有「未知的 SF Symbol」；用 conversations-v19.json 打开对话不崩、旧界面照常显示。

---

## 7. 测试计划与验收

### 7.1 单元测试（Swift SelfTests.swift → QtTest；断言照附录逐条移植）

| Swift SelfTests | Qt 类::用例（文件） | 包 | 断言出处 |
|---|---|---|---|
| `stopState` | `TestModels::markStopped`、`TestModels::pendingApprovalAndInterrupted`（已有）（tst_models）；`TestRunner::stoppedHistory`（tst_runner） | core | core-loop.md §4.1 |
| `queueState` | `TestRunner::interjectedPromptText`；`TestModels::legacyMessageKeys`（含旧存档 `{"id":"2F1E7C1A-…","role":"user","blocks":[],"createdAt":0}`） | core | core-loop.md §4.1 |
| 新键 JSON | `TestModels::v19Keys`：ToolEvent.stopped / usage、ChatMessage 三键、AgentUsage 6 必写键 + service 省略；conversations-v19.json 读写往返 | core | §2.3 |
| `agentLog`（agent_message 一段） | `TestContextAssist::agentLogAgentMessage`（tst_context） | core | core-loop.md §4.1 |
| `liveness`（noteRepeat 一段） | `TestRunner::noteRepeatCounts` | core | core-loop.md §4.1 |
| `liveness`（toolResultText 一段） | `TestRunner::toolResultTextStoppedTeamTool` | core | core-loop.md §4.1 |
| `agentUsage`（windowSpeed / estimateTokens） | `TestRunner::windowSpeedAndEstimate` | core | core-loop.md §4.1 |
| 进程组 | `TestUtil::processGroupKilledOnCancel`（Unix，Windows 上 QSKIP） | core | core-loop.md §4.2-17 |
| `teamState` | `TestAgentTeam::noticesAndMessages`（tst_team） | team | subagents.md §4.1 |
| `teamDepth` | `TestAgentTeam::depthRules` | team | subagents.md §4.1 |
| `liveness`（其余） | `TestAgentTeam::liveness` | team | subagents.md §4.1 |
| （Qt） | `TestAgentTeam::mailboxAndTerminal`：后台孙子结束时父级正阻塞、父级被停的同时下级结束、markStopped 之后迟到的发布、onQueued 晚到——终态都不回退 | team | §2.1 T5 / T6 |
| `agentTree` | `TestAgentTree::buildTurnsFlatten`；`TestAgentTree::statusKindAndChips` | team | subagents.md §4.1 |
| `agentUsage` | `TestAgentTree::usage`（AgentUsage.detail 用 `QLocale::c()`、tokensPerSecond、会话合计、merge 保留存档 usage） | team | subagents.md §4.1 |
| `requestGate` | `TestRequestGate::retryable / priorityGrowthCut / cancelWhileQueued / rateLimitDelaySeconds`（tst_request_gate） | team | subagents.md §4.1 |
| `localScheduler` | `TestSubAgentScheduling::firstAlwaysAdmitted / priorityOrder / parkedHoldsSequence / localPoolFull（改写）/ waitForLocalEnd / diagnosticsText`；删 `onlineQueue / onlineCancelHandsOver` | team | subagents.md §4.1、本文 §3.6.2 |
| （Qt） | `TestWorkerPool::blockedWaitersDoNotPinPool / shutdownCountsReleased` | team | §3.10 |
| （Qt） | `TestLLM::planContext`（roomy 改成 64k + 31 × 16384，新增「第 4 步只在主对话到顶、份额到 16384 后才加」的断言）、`planLaunch`（上限改 `1 + maxExtraSlots`） | team | §3.6.4 |
| （Qt） | `TestTeamInProcess::sharedSlotNeverMain`（进程内 -1 不落主序列）；`LoadedSlotLease` 子进程模式退到 `sharedSlot()` 的单测（用 LlamaServer 的槽位账本，不起进程） | team | §3.6.3 |
| run（MenuBarLabel.speedText） | `TestTreeTray::formats`：meterSpeed `[0, 8.46, 179.4, 1234, 3.4e6, 1.1e12]` → `["0 t/s","8.5 t/s","179 t/s","1.2 Kt/s","3.4 Mt/s","1.1 Tt/s"]`，另加 `1.1e9` →「1.1 Gt/s」；agentTokens 950 / 12.3k / 1.3M；agentSpeed「— tok/s」/ 8.5 / 179 | tree-tray | §3.9 |
| （d621837） | `TestServices::serviceSettings / applyUpdatesEnginesAndProvider / keylessServiceMatchedByTitle / newEnginesInheritBaseURL / invalidSecretName` | settings | §3.12 |

### 7.2 集成用例（假服务；不需要密钥）

- **core**（tst_runner / tst_engines / tst_inproc）：core-loop.md §4.2 的 1–16 全部 + §4.3 的现有用例改写；另加
  `TestInProcess::mainAgentKVFullRetries`（FakeLlama 第一段 `status: FLR_KV_FULL`：主 Agent 不出「遇到内部错误」、≤ 2 次重发后成功、日志有 Q2 的那句；连续 3 次失败才走 recover）、
  `TestRunner::mainAgentKVFullRetriesProcess`（LocalLlamaEngine 指向 MockServer 回「Context size has been exceeded.」流错误，同样判据）、
  `TestRunner::liveMeter`（生成中 liveAgents 1、结束 ≤ 1.5 s 归零、`AgentTeam.clock` 在走）。
- **team**（tst_team_runner / tst_team_inproc / tst_request_gate）：subagents.md §4.2 的 1–15、§4.3、§4.4（FakeChatServer 加 `maxConcurrent / quota429 / subHangMs /
  chunkIntervalMs / retryAfter`，MockServer 的 `rateLimitRetries`、改写 `retryBackoff`、`localEngineBypassesGate`）；另加压力用例 `TestTeamRunner::deepParallelStress`
  （§7.5-H）与 `TestTeamRunner::parallelStoppedParentStopped`（Q1：父卡片 Failed + stopped、子卡片 stopped、queued 清空）。
- **settings**：TestServices 里用真实 EngineStore / CapabilityRegistry 对象（不联网）。

### 7.3 GUI harness（DebugHarness；脚本文件在 `qt/tools/harness/`，一行一步，`#` 开头是注释）

运行：`$env:FRIDAY_DATA_DIR=<scratch>\<脚本>\data`（每次新目录；静态状态先把 §6.E 的 fixture 拷进去，在线大脑用 mock：`python Friday/Tools/Perf/mock_sse.py <端口> <目录>`
+ 「自定义 OpenAI 兼容服务」接 `http://127.0.0.1:<端口>/v1`，存成 fixture 复用）、`$env:FRIDAY_SCREENSHOT=<目录>`、
`$env:FRIDAY_SCREENSHOT_SCRIPT = Get-Content -Raw <脚本>`。每个脚本跑：FluentWinUI3 浅 / 深；Fusion 浅 / 深（`FRIDAY_TITLEBAR=system`）；
改了原生控件的再跑 `QT_QUICK_CONTROLS_STYLE=Windows`（浅）。脚本内容照 ui.md §4.2 的 S1–S6，按包拆：

| 脚本 | 包 | 覆盖 |
|---|---|---|
| `chat-stop.txt` | chat-ui | S1：旋转停止键、「正在停止…」、「已停止」底栏与卡片（`FRIDAY_SLOW_STOP=4`；另跑 `FRIDAY_AUTOSTOP=approval` + mock `sub_approval`） |
| `chat-queue.txt` | chat-ui | S2：排队键与提示、虚线气泡、撤回、「下一轮生效」、「工作中补充」、别处在生成、暂停 +「发送」、空状态建议排队、生成中模型菜单可开 |
| `chat-agents.txt` | chat-ui | S3：并行小标签、卡片状态行 + 停止键（`FRIDAY_SLOW_AGENT_STOP`）、排队条目、汇报卡片、模型胶囊提示、深层收窄、「已经 N 步，展开看过程」/「在 Agent 树里看」 |
| `chat-composer.txt` | chat-ui | d696a44：四个胶囊无箭头、深度思考「开启 / 关闭 / 低 / 中 / 高 / 不支持」、窄窗口时只截模型名、提示文字 |
| `tree-panel.txt` | tree-tray | S4：自动打开（mock `nest: 3`）、概览、连接线、运行中节点、详情（上级链、指标、带输入框的 AgentControls、过程）、`drag:` 拖宽、窄窗口收侧边栏、轮次折叠、空树、`FRIDAY_TREE_FOCUS` |
| `tree-toolbar.txt` | tree-tray | 用量徽标（生成中 / 结束后提示）、树按钮个数与提示、窄到溢出时「»」里的两项 |
| `tray.txt` | tree-tray | S6：`tray` / `traytip` 空闲、生成中、结束后归零 |
| `settings-agents.txt` | settings | 设置 › 子 Agent：新开关、「最多 100 个」步进、footer |
| `settings-services.txt` | settings | 密钥 › 模型面板的服务一级设置（目录服务 / 自定义服务）、恢复默认、密钥名问题文字、「高级」里的 EngineEditor、接入表单的接口地址 |
| `settings-model-menu.txt` | settings | `menu:model` 在线模型副标题「… · 起步并发 N」 |

合并标题栏（Windows 11）上的工具栏右端（徽标、按钮挨着系统按钮）另用真实屏幕截图看一次（`grabWindow` 不含系统按钮）。

### 7.4 真模型场景（`unsloth/Qwen3.5-9B-GGUF:Q4_K_M`；`FRIDAY_INFERENCE=process` 与 `inproc` 各跑一遍；Debug 构建；隔离数据目录；没有在线密钥）

| # | 场景（入口） | 通过判据 |
|---|---|---|
| R1 | 停止正在跑的命令（friday-cli，`FRIDAY_AUTOSTOP=tool:run_shell`，`--yes --permission full`，让它 `ping -n 20 127.0.0.1`） | 打印 `stopped_in=` ≤ 1.0；ping 进程已不在；存档里最后一条助手消息 `stoppedByUser`、run_shell 卡 failed + stopped「已停止」；没有「遇到内部错误」 |
| R2 | 预填中停止（friday-cli 附一个长文本，`FRIDAY_AUTOSTOP=after:0.5`） | `stopped_in` ≤ 0.5；存档里这条助手消息 `stoppedByUser`、没有块、没有 error；同一对话再发「继续」：第二轮正常回答，统计里的缓存命中 > 0（停止没有丢掉整段 KV，按存档重新同步） |
| R3 | 工作中补充（friday-cli，`FRIDAY_QUEUE="tool:run_shell\|顺便说一下现在几点"`，让它用 run_shell 执行 `ping -n 6 127.0.0.1` 再说平均延迟） | 打印 `FRIDAY_QUEUE submitted`；存档顺序 user → 助手 A（工具 + 定稿统计）→ user（interjected）→ 助手 B（回应了补充）；B 的新预填 ≤ 1500 token |
| R4 | 排队到下一轮、停止后暂停（GUI，`chat-queue.txt` 的实时版） | 两条排队在下一轮一起发出、一个回复；停止后「已暂停」，点「发送」后发出 |
| R5 | 工作中换思考强度 / 大脑（friday-cli `FRIDAY_SWITCH="after:2\|thinking:off"`；GUI 生成中在模型菜单换成 mock 在线引擎；本机另有已下载的本地模型时再测本地 → 本地） | 打印 `pending_thinking=true`；这一轮有思考块、下一轮没有；换大脑：底栏「下一轮生效」，这一轮的请求仍发给原来的模型，下一轮发给新的；本地 → 本地时这一轮跑完才卸载 / 加载，排队消息等新模型就绪再发，没有崩溃、没有「模型未加载」错误 |
| R6 | 后台子 Agent（friday-cli `--cwd E:\Friday`：「后台派两个子 Agent 分别统计 qt/src/core 下 .cpp 和 .h 的个数，你同时读 qt/README.md 第一段，最后汇总」） | 至少一次运行里出现 `runsInBackground` 卡（工具结果是「已在后台启动：…」）、≥ 1 条 fromAgent 汇报、这一轮等它们结束后汇总；诊断日志有「本地子 Agent 运行中」；3 次都没用 background 时记录下来，并用 mock `background: true` 补跑同一路径 |
| R7 | 追加指示 / 单独停止（friday-cli，`FRIDAY_AGENT_MESSAGE="after:3\|只统计数量，不用列文件名"`；另一次 `FRIDAY_STOP_AGENT="after:3"`） | 前者子 Agent 运行记录里有「用户补充」卡；后者卡片 failed + stopped、结果以「被用户停止了，没做完。」开头，主 Agent 收到「这个子 Agent 被用户停止了…」并继续完成 |
| R8 | KV 池满（`FRIDAY_LLAMA_ARGS='-c 36864'`；后台 3 个子 Agent 各读一个约 200 KB 的日志，主 Agent 同时读另一个） | 诊断日志出现「本机模型的上下文缓存满了」（子 Agent 的让出重试和 / 或主 Agent 的 Q2 重发）；主消息上没有「遇到内部错误」（除非连续 3 次失败，要在日志里看得到）；所有子 Agent 最终完成或给出明确失败 |
| R9 | 主序列保护（主 Agent → A（delegate）→ B（delegate），`FRIDAY_LOCAL_SLOTS` 不设，让 S = 1：子进程 `FRIDAY_LLAMA_ARGS='-np 2'`，进程内同理） | B 的请求不落 0 号槽位 / 序列（子进程看 llama-server 日志的 slot id，进程内看 llama.log 的 seq）；A、B 返回后主 Agent 下一段的缓存命中 ≥ 上一段上下文的 90% |
| R10 | 计量与托盘（GUI，跑 R6 时 `traytip`、`tray`） | 子 Agent 在跑时 tooltip「Friday\n≥2 AI\n<速度>」、菜单有「N AI 在工作 · …」；结束后 ≤ 1.5 s 回到「Friday\n0 AI\n0 t/s」、菜单行消失 |
| R11 | Agent 树（GUI，两层派发） | 按钮显示运行中个数；节点模型名是本地模型显示名；结束后节点「平均」速度 > 0、生成 > 0；详情页有任务、结论、过程 |

在线行为全部用假服务验（§7.1 / §7.2 的 FakeChatServer / MockServer 用例 + GUI 上的 `mock_sse.py`：`max_concurrent: 2` 限流后全部完成、日志有「被限流：并发 …」；
`quota_429: true` 子 Agent 一次请求就失败「HTTP 429：…」；`sub_hang` + `FRIDAY_STALL_LIMIT=20` 卡片「被看门狗（20 秒没有任何输出）停止了，没做完。」；
`nest: 3` 树自动打开；`background` + `report` 汇报卡片）。

### 7.5 验收清单（全部满足才算这次移植完成）

- **A 构建**：Debug 与 Release 的 `friday`、`friday-cli`、`friday_tests`、`friday_header_check` 全过；没有新的编译警告。
- **B 测试**：§7.1、§7.2 列的用例全部存在且通过；`qt/ci/run_tests.ps1` 全过（Linux 交给 CI，本机不测）。
- **C 文案**：每个包对照附录的逐字文案核对，交付总结附核对表；`grep` 不到旧文案（「（已停止）」、「可嵌套」、「并发请求数」、「改接口地址、并发数」）。
- **D 存档同构**：conversations-v19.json 读入后写出，键名 / 类型与 §2.3 一致；旧存档（没有新键）读得出、新键为 nil。
- **E 界面**：§7.3 全部脚本 × 全部样式 / 配色截图齐全，每次脚本结束「QML 警告 0 条」，没有「未知的 SF Symbol」/ `Binding loop` / `TypeError`；结构、顺序、文案与附录一致（协调者目检）。
- **F 性能**：用 ≥ 200 条消息、≥ 60 个子 Agent 节点的对话：面板开关动画期间帧间隔 p95 ≤ 33 ms（达不到就改一步到位，§3.11.9）；树重建 ≤ 6 次 / 秒；
  流式输出时主线程没有 > 100 ms 的卡顿（`FRIDAY_PERF_STRESS` / perf 日志）。
- **G 真模型**：§7.4 R1–R11 在 process 与 inproc 两种模式下都通过（R6 允许按判据用 mock 补跑 background 路径）。
- **H 并发安全**：`TestTeamRunner::deepParallelStress`（在线假服务，3 层 × 每层并行 5 个，Worker 池上限临时设 4，`FakeChatServer::maxConcurrent = 2`）连续 10 次无死锁、无崩溃、
  闸门 `inFlight == 0`、调度器 running / alive 归零；`TestWorkerPool` 全过。
- **I 文档**：§5 各包的文档段落已改；附录里被取代的段落都已标注，与本文不矛盾。

---

## 8. 待用户决定（两个疑似 Swift bug：这次两边都照抄，修不修、怎么修请用户定）

1. **插入补充后自愈重发了这一轮开头的 prompt**（`AgentRunner.swift` `recover(from:conversationID:prompt:images:videos:)`，:864-906；Qt 同名函数）。
   插入补充 / 汇报后新开的助手消息还没有任何产出就出错时，`recover` 去掉「这条助手消息前面的一条」（就是最后一条补充），再 `stream(prompt:)` 重发——
   传进来的 `prompt` 是这一轮**开始时**发出的（最后一条）用户消息，于是模型重新看到原问题、最后一条补充从上下文里丢了（存档里还在）。
   **建议的修法（Swift 与 Qt 同改）**：没有产出时，取这条助手消息前面**连续的一串用户消息**（最后一批补充 / 汇报，或这一轮的几条输入）作为待重发，
   history = 它们之前的消息；用 `promptText(describeAttachments(…))`（与 send / takeInterjection 相同，带 interjectionNote）组成 `stream(messages:)` 重发；
   有产出的分支不变。`recover` 的 `prompt / images / videos` 参数随之删掉。
2. **任务状态的「像要求的原话」会抓到子 Agent 的汇报卡片**（`TaskState.swift` `requirementMessages(in:)`，:203-209；Qt `TaskState::requirementMessages`）。
   它扫所有 role == user 的消息，而 v1.7 起汇报卡片也是 role == user（`fromAgent` 有值），停止通知里有「不要重新派发…」（含「不要」），会被当成用户的要求附进提示。
   **建议的修法（两边同改）**：过滤条件加 `fromAgent == nil`（工作中补充 `interjected` 是用户的原话，保留）。
