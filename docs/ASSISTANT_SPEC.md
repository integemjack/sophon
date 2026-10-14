# 助手实现规格（照 ASSISTANT.md v2.3 写代码用）

来源：`qt/docs/ASSISTANT.md`（v2.3，623 行，设计稿）。本文件把它翻译成可直接照着写的实现规格：
类名 / 方法名 / 字段名 / 文件路径**凡设计稿写了的一律照抄**；设计稿没写明的按 Qt / C++ 现有惯例补全，标 **（补全）**。
协议以 `qt/docs/REMOTE.md` 为准，新增部分实现时写进 REMOTE.md（§3 给了要写的原文）。
**v2.8 注**：Mac 上的部分（`AssistantHost`、Friday.app 带的 friday-server 当助手进程、往它推引擎和密钥）已作废——Mac 上的助手内核改成 Swift 版、
在 Friday.app 里，见 ASSISTANT.md §2.2。下面保留 v2.3 当时的写法。

## 0. 全局约定（现有代码的规矩，新代码照做）

- 编码：UUID 大写字符串、日期 ISO 8601 **不带毫秒**、可选字段没值时**不写键**（`RemoteProtocol.h` 头注、REMOTE.md §3）。
- 协议：JSON-RPC 2.0，一行一条紧凑 JSON（`RemoteProtocol::frame` / `takeLine`）；错误码沿用
  `-32700/-32600/-32601/-32602/-32603` 与 `1` 协议版本、`2` 没 hello、`3` 找不到、`4` 不允许、`5` 未配对（`RemoteProtocol::ErrorCode`）。
  **不加 `protocol` 版本号**：新能力一律走 hello 的 `features` 数组，缺了就退回旧做法（ASSISTANT.md §11 开头）。
- 事件推送：服务端 80 ms 合批（`RemoteServer::m_flushTimer.setInterval(80)`，`scheduleFlush()` / `flush()`）；`activity` 事件不走合批，一到立即直发给订了的客户端（直发前先把攒着的状态 flush 出去，`RemoteServer.cpp` 注释「不合批」）（2026-10-07 按上游 v2.3 实现对齐）。
- 线程：core 对象主线程创建、以 `FridayCore` 为父；引擎调用在工作线程（`ChatEngine::stream` 是阻塞式回调，回调返回 false 提前结束）。
  `Triage` / `TurnBrief` / `ContextBuilder` 是**纯函数**，可任意线程调用、可单测。
- 构建：`qt/src/core/CMakeLists.txt` 与 `qt/tests/CMakeLists.txt` 都是 `file(GLOB_RECURSE … CONFIGURE_DEPENDS)`，
  **新增 .h/.cpp 不用改 CMake**；core 已链 `Qt6::WebSockets`（渠道要用）。
- 新文件顶部注释按现有风格写「对应 Swift：…」或「Qt 版新增（vX）：…」，并指回 `docs/ASSISTANT.md` 的小节号。

---

## 1. 文件清单（ASSISTANT.md §12）

### 1.1 `qt/src/core/assistant/`（新建目录）

| 文件 | 职责 | 主要类型 |
|---|---|---|
| `AssistantRuntime.h/.cpp` | 事件循环：收输入和事件、调度、打断、调大脑、排写回 | `class AssistantRuntime : QObject`、`struct AssistantInput`、`struct Reaction` |
| `Triage.h/.cpp` | 事件分级（纯函数） | `class Triage`、`enum class TriageLevel`、`struct TriageDecision`、`struct TriageContext` |
| `WorldBoard.h/.cpp` | 状态板：各机器各会话状态；渲染给模型 / 界面 | `class WorldBoard`、`struct BoardSession`、`struct BoardMachine`、`struct PendingApproval` |
| `ContextBuilder.h/.cpp` | 拼上下文、按层裁预算（纯函数） | `class ContextBuilder`、`struct ContextLayer`、`struct AssistantContext`、`struct Budgets` |
| `AssistantTools.h/.cpp` | 助手的工具（`ToolOrigin::Assistant`） | `class AssistantTools`、`struct SessionRef` |
| `Journal.h/.cpp` | 按天 JSONL、分页读、项目索引 | `class AssistantJournal`**（补全：文件名按设计稿 `Journal.*`，类名加 Assistant 前缀避免和现有 `AgentJournal` 混）**、`struct JournalEntry`、`struct JournalDay` |
| `TopicSummaries.h/.cpp` | 按项目的滚动摘要 | `class TopicSummaries`、`struct TopicSummary` |
| `AssistantMemory.h/.cpp` | 记忆条目、关键词检索 | `class AssistantMemory`、`struct MemoryEntry`、`struct SearchHit` |
| `BriefWriter.h/.cpp` | （可选，P1）小模型写回执那一句：2 秒超时、数字校验 | `class BriefWriter` |
| （无独立文件）**（2026-10-07 按上游 v2.3 实现对齐：无 FocusStack，焦点 = `WorldBoard` 的 `mentionedAt` 30 分钟窗口 + `AssistantRuntime::dossiers()` 取前 2 个）** | 焦点：30 分钟内问过的会话按问到时间新→旧取前 2 个 | `WorldBoard::mentionWindowSeconds`、`BoardEntry::mentionedAt`、`AssistantRuntime::dossiers()` |
| `WriteBackQueue.h/.cpp` **（补全：§6.4「共用一个并发队列」需要一个队列对象）** | 后台写回（日志 / 摘要 / 记忆 / 回执那一句）的单一并发队列，回执句排最前 | `class WriteBackQueue`、`struct WriteBackJob` |
| `AssistantSettings.h/.cpp` **（补全：设置项集中一处，落盘仍走 `AppSettings`）** | §9 全部开关的读写与默认值 | `struct AssistantSettings` |
| `channels/ChannelHub.h/.cpp` | 渠道总控：启停、路由（从哪来回哪去）、绑定码、主人校验、推送规则、合并 | `class ChannelHub`、`struct OutboundMessage`、`struct MessageRef`、`struct ChannelStatus` |
| `channels/ChannelAdapter.h/.cpp` | 适配器基类 + 能力声明 | `class ChannelAdapter : QObject`、`struct InboundMessage`、`struct ChannelCapabilities`、`struct ChannelButton` |
| `channels/ChannelRender.h/.cpp` | Markdown → 各平台格式（纯函数） | `class ChannelRender`、`enum class RenderFlavor` |
| `channels/SChatChannel.h/.cpp`（P1 第一个） | SChat 机器人：长轮询、streams 流式、inline 键盘、SPKI 指纹 | `class SChatChannel : ChannelAdapter` |
| `channels/TelegramChannel.*`、`WeixinChannel.*`、`WeComChannel.*`、`FeishuChannel.*`、`DingTalkChannel.*`（P1）；`QQChannel.*`、`SlackChannel.*`、`DiscordChannel.*`、`MatrixChannel.*`、`MailChannel.*`、`WebhookChannel.*`、`ScriptChannel.*`（P3） | 各平台适配器 | 同上 |
| `voice/`（P2） | VAD、流式识别 / 合成接口 | `class VoiceGate`**（补全）**、`class Transcriber`、`class Speaker` |

`AssistantRuntime` 关键签名（**（补全）**部分按现有 core 风格）：

```cpp
class AssistantRuntime : public QObject {
public:
    explicit AssistantRuntime(FridayCore* core, MachinePool* machines, QObject* parent = nullptr);
    ~AssistantRuntime() override;
    void start();                       // 读 state.json（仅 summarizedUntil）、连渠道、订 activity（2026-10-07 按上游 v2.3 实现对齐：无 board.json）
    void shutdown();                    // 落盘、停渠道、取消在跑的反应

    // 输入（耳朵）：渠道、桌面端、语音都走这里
    void submitInput(const AssistantInput& input);
    void interrupt();                   // assistant.stop：取消在跑的反应、停播报
    void notePresence(const QString& clientID, const std::optional<QUuid>& conversationID, bool foreground);

    // 事件（各机器推来）
    void applySnapshot(const QString& machineID, const QList<BoardSession>& sessions);
    void applyActivity(const QString& machineID, const ActivityEvent& event);

    WorldBoard* board();  AssistantJournal* journal();  TopicSummaries* topics();
    AssistantMemory* memory();  ChannelHub* channels();   // 2026-10-07 按上游 v2.3 实现对齐：无 focus() 访问器（焦点在 WorldBoard）
    bool isReacting() const;  int unread() const;

signals:
    void entryAppended(const JournalEntry& entry);
    void delta(const QUuid& entryID, const QString& text);   // assistant.delta
    void boardChanged();
    void unreadChanged();
    void speakingChanged(const QUuid& entryID);              // entryID 为空 = 没在说
    void channelsChanged();
    void attentionRequested(const QString& text);            // 桌面系统通知（补全）
};
```

调度规则（§3.2，全部要落地）：同一时间只有一次反应；反应期间到的输入 / 事件攒进 `m_pending`，下一次一起处理；
用户输入到达即 `interrupt()` 当前反应（断开引擎请求、已流出的文字保留并在日志条目标 `interrupted: true`、语音播报停），
带上这句话重开；已发出的工具调用不撤回；优先级 用户输入 > 立即级事件 > 及时级事件；
用户正在输入 / 说话时事件触发的反应推迟（文字按「输入框有字」，语音按 VAD）；
非用户触发的反应每小时最多 `wakeLimitPerHour`（默认 30）次，超了只更新状态板 + 按固定模板推送；
没有任何客户端和渠道时照常运转。

一次反应的流程（§3.3）：`ContextBuilder::build()` → 流式调大脑（`OpenAIChatEngine`，`enableThinking` 按设置，默认关）→
正文边出边发来源渠道（能流式的流式）+ `assistant.delta` → 工具调用最多 `assistantToolRounds`（默认 2）轮，超了先回话、剩下留给下一次 →
结束写日志、更新焦点、排后台写回。输出分「说的」（正文）和「给看的」（`post_detail` 工具）。

### 1.2 会话侧与远程（改现有文件 / 新建）

| 文件 | 内容 |
|---|---|
| `qt/src/core/agent/ActivityTap.h/.cpp`（新） | `class ActivityTap : QObject`。挂 `AgentRunner` / `ConversationStore` / `AgentTeamState` 的信号，把动静转成 `ActivityEvent`；`tool` 事件**每会话最多 1 条 / 3 秒**；`start(const QUuid& conversationID)` / `stop()` / `setFinalText(bool)`；信号 `activity(const ActivityEvent&)` |
| `qt/src/core/agent/TurnBrief.h/.cpp`（新） | `struct TurnBrief` + 纯函数：`static TurnBrief make(const Conversation&, const ChatMessage& lastAssistant, const TaskState&, const QString& machine, const QUuid& conversationID)`、`static QString pickLine(const QString& markdown, const TaskState* task)`、`static TurnCounts count(const ChatMessage&)`（补全：数字统计单独一个纯函数便于单测） |
| `qt/src/core/agent/AgentControl.h/.cpp`（改） | **加信号 `void turnFinished(const QUuid& conversationID, const QUuid& messageID, const QString& status)`**（status：`"finished"` / `"stopped"` / `"error"`（补全））；桌面系统通知和 CLI 的 `done` 改用它 |
| `qt/src/core/agent/AgentRunner.h/.cpp`（改） | 补 ActivityTap 需要的信号（补全，现有只有不带参数的 `generatingChanged` / `pendingApprovalsChanged`）：`turnStarted(QUuid conversationID, QString by)`、`toolAppended(QUuid conversationID, ToolEvent event)`、`approvalChanged(QUuid eventID, QUuid conversationID, bool pending)`、`quotaWaitChanged(QUuid conversationID, bool waiting, QDateTime resumeAt)`、`stalled(QUuid conversationID, double seconds)`（在 `AgentLiveness.cpp::watchdogTick` 里发）。`submit()` 加 `from` / `whenBusy` 两个参数（见 §3） |
| `qt/src/core/remote/MachinePool.h/.cpp`（新） | `class MachinePool : QObject`：从 `MachineSwitcher` 抽出来的连接池，持有 `remotes.json` 每台机器的 `RemoteSession`、重连、在线状态；本机（`localID = "local"`）进程内直接调 `ConversationStore` / `AgentRunner`。`MachineSwitcher` 改成在它上面只管「现在看哪台」 |
| `qt/src/core/remote/RemoteServer.cpp`（改） | 实现 `activity.subscribe` / `activity.unsubscribe` / `conversation.tail` / `assistant.*`；hello 里报 `features: ["activity"]`（主机侧另报 `"assistant"`，补全） |
| `qt/src/core/remote/RemoteClient.cpp` / `RemoteSession.cpp`（改） | 客户端侧：读 `server.features`、发 `activity.subscribe`、收 `activity` 事件灌进 `MachinePool` → `AssistantRuntime` |
| `qt/src/core/remote/RemoteProtocol.h/.cpp`（改） | 加编解码：`activityToJson/fromJson`、`boardSessionToJson/fromJson`、`turnBriefToJson/fromJson`（补全，风格同 `summaryToJson`） |
| `qt/src/core/agent/ToolTypes.h/.cpp`（改） | `ToolOrigin::Kind` 加 **`Assistant`**，加 `static ToolOrigin assistant()`、`title()` 返回「助手」、`key()` 返回 `"assistant"`；`ToolRegistry.cpp` 两处 switch（约 497、525 行）补 case |
| `qt/src/server/main.cpp`（改） | macOS：`--attach --app`，把 stdin/stdout 桥到 Swift 受管端的 Unix socket；App 没开先 `open -g -j -b <bundle id>` 拉起、等 socket 最多 20 秒 |
| `qt/src/app/DesktopService.cpp`（改） | 第 84 行的 daemon 交接条件 `clientCount() > 0 || RemoteWebSocketConfig::load().enabled` **加「助手开着 / 绑了渠道」** |
| `qt/src/app/viewmodels/Assistant*.{h,cpp}`、`qt/src/app/qml/assistant/`（新） | 助手页、回执行、状态板卡片（卡片 id `board`，可停靠，TERMINAL.md §2）、设置页 |
| `Friday/Friday/Agent/ActivityTap.swift`、`TurnBrief.swift`（新） | 同 C++ 规则 |
| `Friday/Friday/Remote/ManagedEndpoint.swift`（新） | Swift 受管端：本机 socket 上的协议子集（§3.4） |
| `Friday/Friday/Remote/AssistantHost.swift`（新） | 助手进程：`SMAppService` 登录项注册 / 注销、拉起、引擎和密钥自动推过去 |
| `Friday/Friday/Views/Assistant*.swift`（新） | Swift 显示端 |

`MachinePool` 关键签名（补全）：

```cpp
class MachinePool : public QObject {
public:
    explicit MachinePool(FridayCore* core, QObject* parent = nullptr);
    void start();                                   // 连 remotes.json 里全部机器
    QStringList machineIDs() const;                 // "local" 在最前
    QString machineName(const QString& id) const;
    bool isOnline(const QString& id) const;
    RemoteSession* session(const QString& id) const; // "local" 返回 nullptr（进程内直调）
    void call(const QString& machineID, const QString& method, const QJsonObject& params,
              const std::function<void(const QJsonObject& result, const QString& error)>& done);
    void subscribeActivity(const QString& machineID, bool finalText);
signals:
    void machinesChanged();
    void activity(const QString& machineID, const ActivityEvent& event);
    void snapshot(const QString& machineID, const QList<BoardSession>& sessions);
};
```

---

## 2. 数据结构

### 2.1 `ActivityEvent`（§4.1；`core/agent/ActivityTap.h`，补全为独立结构体）

| 字段 | 类型 | 含义 |
|---|---|---|
| `conversationID` | `QUuid` | 哪段会话 |
| `seq` | `qint64` | 每会话递增序号（乱序 / 重复丢弃用） |
| `kind` | `enum class Kind { Started, Step, Tool, Approval, Question, Quota, Stalled, Error, Finished }` | 线上是 `started/step/tool/approval/question/quota/stalled/error/finished` |
| `at` | `QDateTime` | 发生时间（UTC） |
| `machineID` | `QString` | 主机侧填的机器 id（`"local"` 或 remotes.json 的 id）（补全） |
| `startedBy` | `QString` | `kind==Started`：`user` / `assistant` / `queue` |
| `task` | `std::optional<TaskState>` | `kind==Step`：目标 + 步骤列表（`TaskState::Step{text,status}`）+ 完成数由代码算 |
| `summary` | `QString` | `kind==Tool`：一行摘要（`run_shell` / `run_in_terminal` 的命令、写了哪些文件、派了子 Agent） |
| `eventID` | `std::optional<QUuid>` | `kind==Approval`：待确认调用的 id（`runner.resolveApproval` 用） |
| `tool` | `QString` | `kind==Approval`：工具名 |
| `argumentSummary` | `QString` | `kind==Approval`：参数摘要（如 `rm -rf qt/build`） |
| `question` | `QString` | `kind==Question`：问题原文，**截 200 字** |
| `resumeAt` | `std::optional<QDateTime>` | `kind==Quota`：预计恢复时间 |
| `stalledSeconds` | `double` | `kind==Stalled`：卡了多久（阈值 `AgentRunner::stallLimitSeconds()`） |
| `errorSummary` | `QString` | `kind==Error` |
| `brief` | `std::optional<TurnBrief>` | `kind==Finished`：简报 |
| `finalText` | `std::optional<QString>` | 只在 `activity.subscribe {finalText:true}` 时带：这一轮最后一条回答的文字，**截 2000 字** |

### 2.2 `TurnBrief`（§4.2）

| 字段 | 类型 | 含义 / 怎么算 |
|---|---|---|
| `status` | `enum class Status { Finished, Stopped, Error, Approval, Question }` | 完成 / 被停 / 出错 / 要确认 / 在问你；线上 `finished/stopped/error/approval/question` |
| `conversationID` | `QUuid` | 会话 |
| `shortID` | `QString` | UUID 前 4 位（小写，同 §3.4 的「短号」）；同机器内冲突就加长到 5、6 位（补全：由 `WorldBoard` 分配并回填） |
| `machine` | `QString` | 机器名（显示用，如 `mac-mini`） |
| `title` | `QString` | 会话标题 |
| `filesChanged` | `int` | 这一轮改了几个文件：统计成功的 `write_file` / `edit_file` / `create_tool`… 的**去重路径数**（补全：工具名单见 §5.1） |
| `commandsRun` | `int` | 跑了几条命令：`run_shell` / `run_in_terminal` 的成功 + 失败次数 |
| `commandsFailed` | `int` | 其中失败几条（`ToolStatus::Failed`） |
| `subAgents` | `int` | 派了几个子 Agent（`delegate_task` 等有 `transcript` / `agentName` 的调用数） |
| `seconds` | `double` | 用时：这一轮用户消息 `createdAt` → 助手消息 `finishedAt`（没有就用 `updatedAt`） |
| `goal` | `std::optional<QString>` | 会话有任务状态时的目标 |
| `stepsDone` / `stepsTotal` | `int` | 完成几步 / 共几步（`TaskState::steps` 里 `StepStatus::Done` 的个数 / 总数） |
| `line` | `QString` | 「说了什么」那一句（≤ 约 80 字），规则见 §5.2 |
| `lineFromModel` | `bool` | 这一句是小模型写的（默认 false）（补全） |
| `at` | `QDateTime` | 简报生成时间 |

渲染模板（各渠道再渲染，§7.2）：

```
完成｜mac-mini｜跑单测 #3f2a
312 个测试全部通过                                  ← line
目标：跑一遍单测 3/3 · 改了 0 个文件 · 跑了 2 条命令 · 4 分 12 秒   ← 代码写
```

状态词：`完成` / `被停` / `出错` / `要确认` / `在问你`（补全：与 `Status` 一一对应）。
数字段缺项省略（没有任务状态就不写「目标：…」；`filesChanged==0` 仍写「改了 0 个文件」）。

### 2.3 `TriageDecision` / `TriageContext`（§5.1）

```cpp
enum class TriageLevel { Immediate, Timely, Summary, BoardOnly };   // 立即 / 及时 / 汇总 / 只上板
struct TriageDecision {
    TriageLevel level = TriageLevel::BoardOnly;
    bool wakeBrain = false;      // 叫醒大脑
    bool push = false;           // 走推送规则推代码模板
    bool batchWithProject = false; // 进合并窗口，窗口结束时合成一句过一次大脑
    bool deferToNextUserTurn = false; // 「用户隔了一阵回来」时随这次反应批量讲
    QString reason;              // 日志 / 调试用中文
};
struct TriageContext {
    bool userWaiting = false;        // 助手派的 / 用户最近问过（在焦点里）/ watch 过
    bool foreground = false;         // 用户正在桌面端前台看着这个会话（assistant.presence）
    bool brainAvailable = true;      // 大脑不可用时降级成模板推送
    int projectEventsInWindow = 0;   // 同一项目在这个合并间隔里已攒几条
    qint64 msSinceLastUserInput = 0; // 距用户上次说话多久（判「隔了一阵回来」）
    bool userSpeaking = false;       // 用户正在输入 / 说话（推迟事件触发的反应）
};
class Triage {
public:
    static TriageDecision decide(const ActivityEvent& event, const TriageContext& context);
    static TriageLevel defaultLevel(const ActivityEvent& event);   // 设置里能改的等级表
    static bool isWakeKind(ActivityEvent::Kind kind);              // approval / question / error / stalled
};
```

### 2.4 `BoardSession` / `BoardMachine`（§5.2）

```cpp
struct PendingApproval { QUuid eventID; QString tool; QString summary; };
struct BoardSession {
    QUuid id;  QString shortID;  QString title;  QString workingDirectory;
    QString machineID;  QString machineName;
    enum class Status { Idle, Running, Approval, Question, Quota, Error } status = Status::Idle;
    QString detail;                       // 「run_shell「rm -rf qt/build」」「windeployqt 找不到 Qt6Quick.dll」
    std::optional<TaskState> task;  int stepsDone = 0;  int stepsTotal = 0;  QString doingStep;
    QList<PendingApproval> approvals;
    std::optional<TurnBrief> lastBrief;   // 断线重连按状态校正用
    std::optional<QDateTime> lastFinishedAt;
    bool assistantDispatched = false;  std::optional<QDateTime> dispatchedAt;  // 助手派的
    bool userWaiting = false;          // 用户在等的（助手派的 / 在焦点里 / watch 过）
    bool watched = false;              // manage_session 的 watch
    bool foreground = false;           // 有客户端正在前台看（不叫醒、不推）
    bool reportOff = false;            // 会话「⋮」菜单的「不向助手汇报」
    QDateTime updatedAt;
};
struct BoardMachine {
    QString id;  QString name;  bool online = false;  QString version;   // 补全：version 来自 hello 的 server.version
    QList<BoardSession> sessions;      // 非空闲的在前
    int idleCount = 0;  BoardSession latestIdle;   // 「空闲 6 个（最近：#c2e8 卡片拖动 昨天 18:20 完成）」
};
class WorldBoard {
public:
    void applySnapshot(const QString& machineID, const QList<BoardSession>& sessions);  // 全量替换该机器的
    void applyEvent(const ActivityEvent& event);                                        // 增量
    void setMachineOnline(const QString& machineID, bool online, const QString& version);
    void setPresence(const std::optional<QUuid>& conversationID, bool foreground);
    QString renderForModel(int budgetTokens, const QDateTime& lastUserTalk) const;       // §5.2 的那段文本
    QVariantList toVariant() const;                                                     // 界面（按机器分组）
    QList<BoardSession> sinceLastUserTalk(const QDateTime& lastUserTalk) const;          // 「上次跟用户说话之后」
    QList<BoardSession> sessions() const;  std::optional<BoardSession> find(const QString& machineID, const QString& shortID) const;
    QStringList projects() const;                                                        // 项目 = machineID + workingDirectory
    QJsonObject toJson() const;                                                         // 给界面 / assistant.board 协议（2026-10-07 按上游 v2.3 实现对齐：无 fromJson、不落 board.json 快照）
    static QString projectOf(const QString& machineID, const QString& workingDirectory);  // 补全
};
```

给模型的渲染格式（逐字照 §5.2）：

```
## 状态板（10:07）
mac-mini（在线）
- #3f2a 跑单测 ~/Friday｜在跑 3/5「跑集成测试」｜助手 10:02 派的，用户在等
win-pc（在线）
- #77c1 打包 ~/Friday｜出错 09:58「windeployqt 找不到 Qt6Quick.dll」
本机
- #9b10 清理构建 ~/Friday｜等确认 run_shell「rm -rf qt/build」
- #51d0 终端停靠 ~/Friday｜在跑 2/4
空闲 6 个（最近：#c2e8 卡片拖动 昨天 18:20 完成）
上次跟用户说话之后：#3f2a 完成、#77c1 出错
```

排序与裁剪：在跑 / 要确认 / 出错 / 用户在等的排前面逐条列；空闲的只给数量和最近一两个；
超预算（约 1.5k token）**从最旧的空闲开始省略**。

### 2.5 焦点（§6.3）

```cpp
// 2026-10-07 按上游 v2.3 实现对齐：无 FocusStack 类与权重衰减，焦点 = 状态板上「最近 30 分钟问过的会话」。
struct BoardEntry {                                // WorldBoard.h
    QString machineID;  SessionActivity session;
    bool dispatched = false;                       // 助手派的：结束时叫醒大脑
    bool watched = false;                          // 用户说过「有结果告诉我」
    QDateTime mentionedAt;                         // 用户最近一次问到它（在焦点里也算）
    QDateTime changedAt;
};
class WorldBoard {
public:
    static constexpr int mentionWindowSeconds = 30 * 60;   // 30 分钟窗口
    void markMentioned(const QString& machineID, const QUuid& conversationID, const QDateTime& now);
    bool isAwaited(const BoardEntry& entry, const QDateTime& now) const;
    // 用户在等的 = 助手派的、关注的、30 分钟内问过的
};
// AssistantRuntime::dossiers(t)：板上 mentionedAt 距今 ≤ mentionWindowSeconds 的会话，
// 按 mentionedAt 新→旧取前 2 个，各生成一段焦点项目档案（短号 + 状态、目录、目标、步骤、最近一次）。
```

焦点由**代码推**：调了哪个会话的工具、回复里提到的短号、用户 `@` 的会话（都会更新 `mentionedAt`）；上游无 `set_focus` 工具。

### 2.6 记忆与日志（§6.4、§10）

```cpp
struct MemoryEntry {
    QUuid id;
    enum class Kind { Preference, Fact, Decision } kind;   // 线上 preference|fact|decision
    QString text;
    std::optional<QString> project;                        // 不属于项目的归「日常」
    QString source;                                        // 日志位置，如 journal/2026-10-07.jsonl#<entryID>
    QDateTime createdAt;  QDateTime updatedAt;
};
struct SearchHit { QString where;   // memory | topic | journal
                   QString id;  QString text;  double score;  std::optional<QString> project;  QDateTime at; };
class AssistantMemory {
public:
    QList<MemoryEntry> all() const;
    void upsert(const MemoryEntry& entry);                 // 去重、同一件事更新旧条目
    bool remove(const QUuid& id);  bool update(const QUuid& id, const QString& text);
    QList<SearchHit> search(const QString& query, const QString& projectFilter, int limit) const;  // 分词 + BM25
    QJsonArray toJson() const;  static AssistantMemory fromJson(const QJsonArray&);
};
struct JournalEntry {
    QUuid id;  QDateTime at;
    enum class Kind { User, Assistant, Receipt, Tool, Event } kind;   // 线上 user|assistant|receipt|tool|event
    QString source;                 // desktop | voice | schat | weixin | telegram | …（补全：渠道 id）
    QString text;
    QStringList projects;           // 涉及的项目
    QStringList refs;               // 涉及的会话（machineID#shortID 或大写 UUID，补全：统一用 UUID）
    bool interrupted = false;       // 被打断的反应
    std::optional<QString> replyTo; // 「已回复到 微信」（补全）
};
struct JournalDay { QString date; int count; QStringList projects; };   // journal/index.json 的一项
class AssistantJournal {
public:
    void append(const JournalEntry& entry);                              // 只追加当天的 JSONL + 更新 index.json
    QList<JournalEntry> page(const QDateTime& before, int limit, const QString& projectFilter) const;  // assistant.history
    QList<JournalEntry> tail(int limit) const;                           // 重启后恢复最近对话
    QList<JournalDay> index() const;
    void clearAll();                                                     // 设置里「清空助手的日志和记忆」
};
struct TopicSummary { QString project; QString summary; QUuid coveredUntil; QDateTime updatedAt; };  // topics.json
```

### 2.7 渠道（§7.1）

```cpp
struct InboundMessage {
    QString channel;      // 渠道 id：schat / telegram / weixin / wecom / feishu / dingtalk / webhook / script
    QString chatKey;      // 平台会话键（SChat：user:<id>）
    QString sender;       // 发送者显示名 / id
    QString text;
    QStringList attachments;                 // 落到 Assistant/media/ 的路径（补全）
    std::optional<QString> replyTo;          // 平台「引用回复」的消息 id
    std::optional<QString> callbackData;     // 按钮点击
    std::optional<QString> audio;            // 语音消息（P2）
    bool fromOwner = false;                  // 是不是绑定的主人（补全：ChannelHub 填）
    bool isGroup = false;
};
struct ChannelButton { QString text; QString data; };
struct OutboundMessage {
    QString chatKey;  QString text;  QList<ChannelButton> buttons;  std::optional<QString> replyTo;
};
struct MessageRef { QString channel; QString chatKey; QString messageID; };
struct ChannelCapabilities {
    bool markdown = false;  bool buttons = false;  bool edit = false;  bool stream = false;
    bool files = false;  bool voice = false;  int maxLength = 0;  int rateLimit = 0;   // 每秒条数（补全）
};
class ChannelAdapter : public QObject {
public:
    virtual QString id() const = 0;  virtual QString title() const = 0;
    virtual bool start(QString* error) = 0;  virtual void stop() = 0;
    virtual ChannelStatus status() const = 0;                 // 界面上的小圆点：connected / connecting / error(text)
    virtual ChannelCapabilities capabilities() const = 0;
    virtual MessageRef send(const OutboundMessage& message) = 0;
    virtual void edit(const MessageRef& ref, const QString& text) {}
    virtual void streamBegin(const QString& chatKey) {}  virtual void streamAppend(const QString& delta) {}
    virtual void streamFinish(const OutboundMessage& final) {}  virtual void streamAbort() {}
signals:
    void inbound(const InboundMessage& message);
    void statusChanged();
};
```

---

## 3. 协议改动（实现时逐条写进 REMOTE.md）

命名与结构风格照 REMOTE.md：方法名 `域.动作` 小驼峰、事件名单词或 `域.名词`、params 用 JSON 对象、
没返回值的回 `{}`、可选字段不写键、错误 `message` 是给用户看的中文。

### 3.1 握手

- `hello` 请求加 `features: ["activity"]`（客户端报自己支持的）；回复的 `server` 里加 `features: [...]`（服务端报自己支持的）。
  服务端支持的：`"activity"`（有 `activity.*`）、`"assistant"`（这台是助手主机，有 `assistant.*`）（补全：后者便于客户端显示「助手在 mac-mini 上跑」）。
  **不加 `protocol` 版本号**；对端没报 `activity` 就退回旧做法（§3.5）。

### 3.2 主机 ↔ 各机器（写进 REMOTE.md §5 事件表 + §6 方法表）

| 方法 / 事件 | params | result / 说明 |
|---|---|---|
| `activity.subscribe` | `{finalText?: true}` | `{sessions:[ActivitySession]}`：全部会话的状态快照；之后推 `activity` 事件。`finalText` 为真时 `finished` 事件多带这一轮最后一条回答的文字（截 2000 字），给主机上的小模型写回执那一句；默认不带 |
| `activity.unsubscribe` | `{}` | |
| 事件 `activity` | `ActivityEvent`（§2.1，含 `conversationID, seq, kind, at` 与该 kind 的字段） | 不合批，一到立即直发（其余状态事件仍 80 ms 合批）（2026-10-07 按上游 v2.3 实现对齐） |

`ActivitySession`（快照的一项）：

```jsonc
{"id":"5B0C…","title":"跑单测","workingDirectory":"/Users/x/Friday",
 "status":"running",                     // idle|running|approval|question|quota|error
 "task":{"goal":"跑一遍单测","steps":[{"text":"装依赖","status":"done"}]},   // 没有任务状态不写
 "lastBrief":{…TurnBrief…},              // 最后一次简报（断线重连按状态校正用）
 "lastFinishedAt":"2026-10-07T10:02:00Z",
 "approvals":[{"eventID":"…","tool":"run_shell","summary":"rm -rf qt/build"}]}
```

| 方法 | params | result |
|---|---|---|
| `conversation.tail` | `{id, count, maxChars}` | `{messages:[ChatMessage 的压缩版]}`：当前路径最后几条，文字截断、工具只留名字和一行结果（补全：压缩后仍是 `ChatMessage` 的 JSON，块被裁；另回 `{truncated:true}` 当有裁剪）。也是「以后的手机 App」分页的第一步 |
| `runner.submit`（改） | 加 `from?: {kind:"assistant", channel?: "schat"}`、`whenBusy?: "queue"\|"interject"`（默认 `queue`） | 会话里显示「助手发送 · 来自 微信」（`QueuedInput.from` / `ChatMessage` 加来源字段） |
| `conversation.create`（改） | 加 `title?`、`engineID?`、`createdBy?: "assistant"` | 助手开的会话在侧边栏带小机器人标记 |

### 3.3 客户端 ↔ 主机（看助手；写进 REMOTE.md 新增一节「助手」）

| 方法 / 事件 | params | result |
|---|---|---|
| `assistant.subscribe` | `{}` | `{state, board, entries, channels}`：`state` = 助手状态与设置（不含密钥值）、`board` = WorldBoard 的 JSON、`entries` = 最近一段日志、`channels` = 各渠道状态；未读数不进协议、只在客户端内存，无 `focus` 字段（2026-10-07 按上游 v2.3 实现对齐） |
| 事件 `assistant.board` | `{board}` | 状态板变了（整份快照，同 REMOTE.md §5 的「完整快照」规则） |
| 事件 `assistant.entry` | `JournalEntry` | 新日志条目（用户的话、助手的话、回执、工具卡片） |
| 事件 `assistant.delta` | `{entryID, text}` | 流式正文增量 |
| 事件 `assistant.speaking` | `{entryID}` 或 `{entryID:null}` | 在播报哪条 / 停了（语音 P2，接口 P0 留好） |
| 事件 `assistant.channels` | `{channels:[ChannelStatus]}` | 渠道连接状态变了 |
| `assistant.history` | `{before, limit, project?}` | `{entries:[JournalEntry], hasMore}`（补全 `hasMore`）：往上翻按页加载 |
| `assistant.say` | `{text, attachments?}` | `{entryID}`（补全）：用户说话。语音 P2 另加 `assistant.audio` |
| `assistant.stop` | `{}` | 打断当前反应 |
| `assistant.memory.list` | `{}` | `{entries:[MemoryEntry]}` |
| `assistant.memory.update` | `{id, text}` | |
| `assistant.memory.remove` | `{id}` | |
| `assistant.settings.update` | 设置里任意几项（只改给了的，同 `settings.update` 的写法） | |
| `assistant.channels.update` | `{channel, config}`（token 只写密钥名；值走 `secrets.set`） | |
| `assistant.bindCode` | `{}` | `{code}`：6 位绑定码（在聊天工具里发 `/bind 123456`） |
| `assistant.export` | `{}` | 分块读出 `Assistant/` 全部文件（补全：`{files:[{path, offset, data(base64), last}]}`，每块 ≤ `RemoteProtocol::chunkBytes`） |
| `assistant.import` | `{path, offset, data(base64), last}` | 写进新主机（换主机） |
| `assistant.presence` | `{conversationID\|null, foreground}` | 客户端报告前台正在看哪个会话（切会话、窗口进出前台时发）；主机据此不叫醒、不推这个会话的事件；**断开即视为没在看** |

`assistant.export` / `assistant.import` **只给本地 socket（SSH / WSL）连接**，同 `server.pair`；WebSocket 进来的回错误 `4`。
没开助手的机器上所有 `assistant.*` 回错误 `3`（找不到）；客户端据此显示「助手在 mac-mini 上」或「打开助手」。

### 3.4 Swift 受管端（Mac 本机，§11.3）

Swift App 在本机 Unix domain socket（只有同一个用户能连）上实现协议**子集**，消息格式、握手、错误码与 REMOTE.md 完全一样：

- `hello` → `server.host: "swift"`、`features: ["activity"]`；`state` 只有 `summaries`、`engines`、`runner`。
- 会话：`conversation.create`（带 §3.2 新字段）、`.remove`、`.rename`、`.setPinned`、`.tail`；事件 `summaries`。
- 运行：`runner.submit`（带 `from` / `whenBusy`）、`.stop`、`.resolveApproval`、`.setEngine`、`.setPermissionMode`、`.setThinking`、`.retry`；事件 `runner`。
- 动静：`activity.subscribe` / `.unsubscribe`、事件 `activity`（Swift 的 `ActivityTap` / `TurnBrief`，规则同 C++）。
- 配置：`engines.update`、`secrets.set`、`settings.update` 只用来**往助手进程推**，受管端自己不接收这几样。
- 其余方法（终端、文件、`conversation.subscribe` 整段推、`server.*`）回错误 `4`（不允许）。
- `friday-server --attach --app`（macOS）：stdin/stdout 桥到这个 socket；App 没开先 `open -g -j -b <bundle id>` 拉起、等 socket 出现（最多 20 秒）。

### 3.5 退回旧做法（老版本服务端，hello 里没有 `activity`）

主机侧 `MachinePool` 退回：比对 `runner.generating` 和 `summaries`；某个会话从 generating 里消失（结束）时
`conversation.subscribe` 它、拿最后一条消息、生成简报、再 `conversation.unsubscribe`。`RemoteDeploy` 会把远程升到新版本，退回只是过渡。

---

## 4. 上下文拼装规则（§6.1）

`ContextBuilder` **每次反应按当前状态重新拼**，不在上次历史后面接着加。

| 顺序 | 层 | 内容来源 | 预算 | 层内截断策略 |
|---|---|---|---|---|
| 1 | 固定前缀 | 人设 + 规则 + 工具定义（`AssistantTools` 的 `ToolSpec`）+ 长期记忆核心（用户是谁、机器叫什么、长期偏好；取 `AssistantMemory` 里 `kind != Decision` 且无 `project` 的条目，按 `updatedAt` 新→旧） | ~3k | 记忆核心超了就只留最新 N 条；**这一层要稳定以命中提示缓存**：内容按固定顺序拼、不含时间戳等每次都变的东西 |
| 2 | 状态板 | `WorldBoard::renderForModel(budget, lastUserTalk)` | ~1.5k | 从最旧的空闲会话开始省略；再超就砍空闲计数行以外的 detail |
| 3 | 焦点项目档案（最多 2 个） | `AssistantRuntime::dossiers()`（30 分钟内问过的会话按 `mentionedAt` 新→旧取前 2 个）：短号 + 状态、目录、目标、步骤、最近一次简报（2026-10-07 按上游 v2.3 实现对齐） | ~2k | 每个项目平均分配；先砍旧简报，再砍步骤列表（保留 goal + 完成数） |
| 4 | 相关记忆 | `AssistantMemory::search(这句话)`（分词 + BM25，按项目和时间过滤） | ~1k | 按 score 取前 N 条，每条截 200 字（补全） |
| 5 | 话题摘要 | `TopicSummaries`：焦点项目的 + 「日常」 | ~1.5k | 摘要本身有上限，超了由后台写回再压一次（越旧越概括）；拼装时按项目均分、砍最旧段落 |
| 6 | 最近对话原文 | `AssistantJournal::tail()`：最近十来轮 | ~4k | 超出窗口的最老几轮由后台写回压进第 5 层摘要；拼装时从最老的开始丢，丢了几轮在层首写明（补全） |
| 7 | 新事件 + 这句话 | 上次反应之后攒下的 `ActivityEvent`（渲染成一行一条）+ 用户这句话（含来源标注） | ~1k | 事件按等级保留：立即 > 及时 > 汇总；同会话同种事件合并成一条（§9 防死循环） |

合计约 **14k token**。规则：

- **变化少的放前面**（缓存命中、首字快、便宜），变化多的放后面；顺序固定为 1→7。
- 各层超预算时**在层内裁剪**（列表省略旧的、摘要截断），**不挤占别的层**。
- 预算按大脑模型的上下文大小缩放，**最小不低于上表**（补全：`Budgets::scaled(modelContextTokens)`，比例 = 上表 / 14k，缩放后每层不低于上表值）。
- 第 6 层只放助手对话自己的原文；`read_session` 取来的会话内容**不写进最近对话**，只用于这次反应（日志里记一句「看了 #3f2a 最近 5 条」）。
- 防注入：回执、会话内容、渠道里别人转来的文字一律放在**数据区**并标明来源，不当指令（§9）。

```cpp
struct ContextLayer { int order; QString title; QString text; int budgetTokens; };
struct AssistantContext { QList<ContextLayer> layers; int totalTokens; QList<EngineMessage> messages; };  // 补全：messages 是给引擎的
class ContextBuilder {
public:
    struct Budgets { int prefix=3000, board=1500, focus=2000, memory=1000, topics=1500, recent=4000, incoming=1000;
                     static Budgets scaled(int modelContextTokens); int total() const; };
    static BuiltContext build(const ContextInputs& inputs, const ContextBudget& budget = ContextBudget());
    // ContextInputs 打包 board / dossiers（焦点档案，AssistantRuntime 按 30 分钟窗口选好）/ memory / topics / journal / pending / inputs / tools
    //（2026-10-07 按上游 v2.3 实现对齐：无 FocusStack 参数）
    static int estimateTokens(const QString& text);   // 补全：沿用 core 现有的估算（约 4 字符/token，中文按字）
};
```

---

## 5. 纯代码规则

### 5.1 `TurnBrief` 的状态和数字（永远由代码写，不经模型）

- 状态：这一轮被用户停 → `Stopped`（`ChatMessage::stoppedByUser`）；有 `error` → `Error`；
  消息里 `MessageBlock::containsPendingApproval` 为真 → `Approval`；`pickLine` 判定整段是问句 → `Question`；否则 `Finished`。
- `filesChanged`：这一轮助手消息里状态为成功的写文件类工具（`write_file`、`edit_file`、`create_tool`、`update_tool`、`create_provider`（补全：按 `ToolRegistry` 里 permission 为 WriteFiles 的内置工具））的**去重目标路径数**。
- `commandsRun` / `commandsFailed`：`run_shell` + `run_in_terminal` 的调用数 / 其中 `ToolStatus::Failed` 的数（`stopped` 的不算失败，补全）。
- `subAgents`：带 `transcript` 或 `agentName` 的工具调用数（`delegate_task` 等）。
- `seconds`：用户消息 `createdAt` → 助手消息 `finishedAt`（缺则 `updatedAt`），显示成「4 分 12 秒」。
- 有任务状态时加「目标 + 完成几步 / 共几步」：`TaskState::goal`、`steps` 里 `Done` 的个数 / 总数。
- 递归进子 Agent 的运行记录（`ToolEvent::transcript`）**不计入**上面的数字（补全：只算主 Agent 这一层，避免重复计数）。

### 5.2 「说了什么」那一句：`TurnBrief::pickLine`（默认规则，逐条照 §4.2）

1. 取最后一个文字块（`MessageBlock::Kind::Text`）；去掉代码块、表格、引用、Markdown 符号和链接地址（**留链接文字**）。
2. 取最后一段（最后一个非空段落）里**第一句陈述句**；跳过问句、客套话（「如有需要…」「希望对你有帮助」）、只有标题的行。
3. 最后一段整段是问句（以「？」/「?」结尾，或命中 `UserStops` 的类别）→ 这一条**不是「完成」**，`status` 改成 `Question`（会话在问你），`line` 放问句原文。
4. 最后一段挑不出来就往前一段找；都没有就用任务状态里**最后完成的那一步**（`steps` 中最后一个 `Done` 的 `text`）。
5. 截到约 **80 字**，在句末或标点处截。

可选小模型（`BriefWriter`，设置「用小模型给回执写一句话」，**默认关**）：在主机上用写回模型，
输入只有这一轮最后一条回答（`finished` 事件按需带 `finalText`）和任务状态，约 1–2k token；
要求一句不超过 **40 字**的结论、**不许改数字**；不占大脑、可以并发；
**2 秒没出来就用规则挑的那一句**；写出来的句子里出现的数字和代码给的对不上，也丢掉、用规则那句。

```cpp
class BriefWriter {
public:
    BriefWriter(EngineStore*, WriteBackQueue*, QObject* parent = nullptr);
    /// 2 秒超时；成功且数字校验通过才回调 line，否则回调空（调用方用规则那句）
    void write(const TurnBrief& ruleBrief, const QString& finalText, const TaskState& task,
               const std::function<void(const QString& line)>& done);
    static bool numbersMatch(const QString& line, const TurnBrief& ruleBrief);   // 纯函数，单测
};
```

### 5.3 `Triage`：什么条件叫醒大脑（§5.1 等级表，设置里能改）

| 事件 / 情况 | 默认等级 | 做什么 |
|---|---|---|
| `approval`、`question`、`error`、`stalled` | 立即 `Immediate` | 叫醒大脑，顺带给下一步建议；用户正在说话 / 讨论别的就等这句说完插一句 |
| `finished` 且**用户在等的** | 及时 `Timely` | 叫醒大脑，接着用户的原话回（「你让跑的单测全过了，要提交吗？」） |
| 同一个项目在一个合并间隔里来了 **3 条以上** | 及时 `Timely` | 间隔结束时过一次大脑，合成一句 |
| 用户隔了一阵回来（**30 分钟**没说话后的第一句） | 随这次反应 | 这次反应**批量讲**攒下的回执（状态板的「上次跟用户说话之后」） |
| `finished` 其他 | 汇总 `Summary` | 只上状态板；按推送规则推**代码模板**，不过大脑 |
| `step`、`tool`、`started`、`quota` | 只上板 `BoardOnly` | 不推、不叫醒 |

- 「用户在等的」= 助手派的（`assistantDispatched`）、用户最近问过的（在焦点里）、`watch` 过的会话。
- **用户正在桌面端前台看着那个会话**（`assistant.presence`）时，这个会话的事件**只上状态板**，不叫醒、不推。
- **大脑挂了也照推**：代码模板的推送不经模型；本该叫醒大脑的那几种，大脑不可用时也按模板推（`TriageContext::brainAvailable == false`）。
- 会话标了「不向助手汇报」（`reportOff`）时一律 `BoardOnly`（补全，来自 §1.1 的菜单项）。

### 5.4 批量策略（原则：要说人话时批量过一次大脑，不是一条过一次）

- **合并间隔 30 秒**（设置 `mergeWindowSeconds`）：间隔内同一项目的多条合成一条推送（SChat 一个会话 5 秒最多 5 条，本来也要合并）。
- 间隔内同一项目 ≥ 3 条 → 间隔结束时**过一次大脑**合成一句；这一次在自动唤醒上限里**只算一次**。
- 「用户隔了一阵回来」的批量讲是**用户说话触发的**，不算进自动唤醒上限。
- 量级依据：8 个会话同时在跑、每个一天 15 轮 ≈ 一天一百多条回执，过大脑的从一百多次降到十几次。
- 自动唤醒上限：非用户触发的反应每小时最多 30 次（设置），超了只更新状态板 + 按固定格式推送。
- 推送规则（设置）：全部推 / **只推助手派的、出错的、要确认的（默认）** / 不推。

---

## 6. 存储格式（§10）

**助手不进 `conversations.json`**（那是所有会话一个 JSON 数组、每次整份重写；`conversation.subscribe` 也是整段推、不分页）。
目录：`<dataDir>/Assistant/`（`<dataDir>` 是主机的数据目录；Mac 上是助手进程的 `~/Library/Application Support/Friday-Assistant/`，
由 `FRIDAY_DATA_DIR` 指定，不碰 Swift 的 `…/Friday/`，也不用 Qt core 在 macOS 默认的 `…/Friday-Qt/`）。

```
<dataDir>/Assistant/
  journal/2026-10-07.jsonl   按天追加，一行一条：
                             {"id":"<大写UUID>","at":"2026-10-07T10:02:00Z",
                              "kind":"user|assistant|receipt|tool|event","source":"weixin",
                              "text":"…","projects":["mac-mini:/Users/x/Friday"],"refs":["5B0C…"],
                              "interrupted":true}          // interrupted / projects / refs 为空时不写键
  journal/index.json         {"days":[{"date":"2026-10-07","count":132,"projects":["…"]}]}（补全：外层键名 days）
  topics.json                {"<项目>":{"summary":"…","coveredUntil":"<消息UUID>","updatedAt":"…"}}
  memory.json                [{"id":"…","kind":"preference|fact|decision","text":"…","project":"…","source":"journal/2026-10-07.jsonl#<id>","createdAt":"…","updatedAt":"…"}]
  state.json                 {"summarizedUntil":"<ISO 时间>"}（2026-10-07 按上游 v2.3 实现对齐：仅此一项；无 board.json，焦点不落盘，未读数只在客户端内存，渠道游标、绑定的主人在 channels.json）
  channels.json              [{"id":"schat","enabled":true,"tokenSecret":"SCHAT_BOT_TOKEN","notify":true,…}]（token 只写 SecretStore 里的密钥名）
  media/                     渠道收到的图片 / 文件、（开了的话）语音录音
```

规则：格式同其他数据文件（大写 UUID、ISO 时间不带毫秒）；**日志不删**；设置里给「清空助手的日志和记忆」（`AssistantJournal::clearAll` + `AssistantMemory` 清空）；
日志**只追加，不整份重读**（`append` 用 `QFile::Append`，`index.json` 单独小文件）。
重启后（§6.6）：状态板不落盘（无 `board.json`），连上各机器后按 `activity.subscribe` 的快照校正；最近对话从日志尾部读（`tail`）；`state.json` 只存 `summarizedUntil`（焦点不落盘，未读数只在客户端内存，渠道游标在 `channels.json`）（2026-10-07 按上游 v2.3 实现对齐）。

### 6.1 设置项（§9 的表，落 `AppSettings`；键名补全）

| 设置 | 键 | 默认 |
|---|---|---|
| 助手 | `assistantEnabled` | **关** |
| 助手跑在 | `assistantHost`（`"local"` 或机器 id） | 本机 |
| 大脑的模型 / 写回的模型 | `assistantBrainEngineID` / `assistantWriterEngineID`（空 = 跟新对话一样，取 `EngineStore::resolved(defaultEngineID)`，没有就第一个引擎） | 跟新对话一样 |
| 大脑思考 | `assistantThinking` | 关 |
| 每次反应的工具轮数 | `assistantToolRounds` | 2 |
| 推送规则 | `assistantPushRule`（`all` / `important` / `none`） | `important`（只推助手派的、出错的、要确认的） |
| 合并间隔 | `assistantMergeWindowSeconds` | 30 |
| 自动唤醒上限 | `assistantWakeLimitPerHour` | 30 |
| 用小模型给回执写一句话 | `assistantBriefSmallModel` | 关 |
| 在聊天工具里批准工具调用 | `assistantApproveInChannel` | **关** |
| 助手（大脑）自己批准工具调用 | `assistantSelfApprove` | **关** |
| 从聊天工具开 Full 权限的会话 | `assistantFullPermissionFromChannel` | **关** |
| 群聊 | `assistantGroupChat`（`pushOnly` / `full`） | 只推送，不接指令 |
| 保存语音录音 | `assistantSaveAudio` | 关 |
| 事件等级表 | `assistantTriageLevels`（补全：kind → level 的覆盖表） | 同 §5.3 |

大脑要能调工具（`EngineConfig::supportsTools`）：选中的（或新对话的）模型不支持时**设置页标红、提示另选**，助手**先只回话不派活**。

---

## 7. 助手的工具（§3.4，`ToolOrigin::Assistant`，只在助手内核里用）

全部**异步**：派活类立即返回句柄，结果以后作为事件回来；取数类有超时（**3 秒**，超了返回「稍后作为事件给你」）。**没有 `wait_*`**。

| 工具 | 参数（补全：按 `ToolDescriptor::schema(QList<ToolParam>)` 生成） | 返回 | 底下用的 |
|---|---|---|---|
| `list_machines` | — | 机器、在线状态、版本、忙不忙 | `MachinePool` |
| `add_machine` / `remove_machine` | `{target, name?, identityFile?}` / `{machineID}` | 句柄；**密码一律不经聊天工具** | `RemoteMachines`、`RemoteKeySetup`、`RemoteDeploy` |
| `sync_config` | `{machineID, engines?, settings?, mcp?}` | 先出预览，确认后写 | `RemoteConfigSync` prepare / apply |
| `list_sessions` | `{machineID?, directory?, status?}` | 会话列表 | 状态板 |
| `create_session` | `{machineID, workingDirectory, firstText?, engineID?, permissionMode?, title?}` | `{conversationID, shortID}` | `conversation.create` + `runner.submit` |
| `send_to_session` | `{machineID, shortID, text, whenBusy?}` | 句柄；忙时默认 `queue`，用户说「马上」才 `interject` | `runner.submit` |
| `read_session` | `{machineID, shortID, count}` | 最近 N 条（压缩过的），**只用于这次反应** | `conversation.tail` |
| `manage_session` | `{machineID, shortID, action: rename\|pin\|stop\|retry\|setEngine\|setPermissionMode\|watch\|remove, …}` | 句柄；`remove` **每次确认** | `conversation.*` / `runner.*` |
| `resolve_approval` | `{eventID, approved}` | **默认不给大脑**（`assistantSelfApprove` 关时不注册这个工具） | `runner.resolveApproval` |
| `recall` | `{query, project?, limit?}` | 搜日志、话题摘要、记忆；结果只用于这次反应 | §6.5 |
| `remember` / `forget` | `{text, project?}` / `{id}` | 用户让它记住 / 忘掉某件事 | `AssistantMemory` |
| `notify` / `post_detail` | `{text, channel?}` / `{text, title?}` | 主动推一条 / 发「给看的」内容 | 渠道层 |
| `think_in_background` | `{task}` | 派一个后台子 Agent 去想，想完作为事件回来 | `SubAgentRunner` |

会话的指代：**「机器 + 短号」**，如 `mac-mini #3f2a`（UUID 前 4 位，同机器内冲突就加长；CLI 的 `-c` 已经按 id 前缀找）。
`SessionRef{machineID, shortID}` → `WorldBoard::find()` 解析成 UUID（补全）。

不影响会话本身（§3.5）：助手转过去的话默认排队到下一轮，不插进正在跑的那一步；看会话只取一小段（`conversation.tail`），不长期订阅；
主机对远程**只订 `activity`，不订每个会话的 `message` 快照**（那是约 10 次 / 秒的整条消息）；助手派的会话就是普通会话，用户随时能打开、接着说、停掉。

---

## 8. 分阶段清单（§13）

### P0 底座（桌面端文字跑通）— 必须

1. `core/agent/TurnBrief.{h,cpp}`：`TurnBrief` 结构、`make` / `count` / **`pickLine`（规则挑那一句）**。
2. `core/agent/ActivityTap.{h,cpp}`：9 种 `kind` 的事件、`tool` 每会话 1 条 / 3 秒、`finalText` 开关。
3. `AgentControl::turnFinished(conversationID, messageID, status)` + **桌面系统通知**（现在没有；不开助手也要有）+ CLI 的 `done` 改用它；补 §1.2 列的 AgentRunner 信号。
4. 协议：`hello.features`、`activity.subscribe` / `.unsubscribe` / 事件 `activity`、`conversation.tail`、`runner.submit` 的 `from` / `whenBusy`、`conversation.create` 的 `title` / `engineID` / `createdBy`、`assistant.presence`、**退回旧做法**（§3.5）；`RemoteProtocol` 的编解码；写进 REMOTE.md。
5. `core/remote/MachinePool.{h,cpp}`：从 `MachineSwitcher` 抽出来；`MachineSwitcher` 改成只管「现在看哪台」。
6. `WorldBoard` + `Triage`：状态板（含快照校正、渲染给模型 / 界面）、分诊（**含合并后过一次、回来时批量讲**、前台在看时不叫醒）。
7. `AssistantRuntime`：调度、打断、异步工具、轮数上限（2）、**说的 / 给看的分开**、自动唤醒上限、`state.json` 恢复。
8. `ContextBuilder` + `Journal` + `TopicSummaries` + `AssistantMemory`（7 层拼装、按天 JSONL 分页、按项目滚动摘要、BM25 检索）+ `WriteBackQueue`（2026-10-07 按上游 v2.3 实现对齐：无 FocusStack，焦点在 WorldBoard）。
9. `AssistantTools`（P0 至少：`list_machines`、`list_sessions`、`create_session`、`send_to_session`、`read_session`、`manage_session`、`recall`、`remember` / `forget`、`notify`、`post_detail`、`sync_config`、`add_machine` / `remove_machine`；`resolve_approval` 受 `assistantSelfApprove` 控制，默认不注册）+ `ToolOrigin::Kind::Assistant`（2026-10-07 按上游 v2.3 实现对齐：无 `set_focus` 工具）。
10. 界面（Qt）：侧边栏助手行（未读数、渠道小圆点）、时间线（按天分隔、按页加载、项目筛选）、回执行（一行一条、状态图标、折叠「另外 N 条」、批准 / 拒绝）、状态板卡片（可停靠，卡片 id `board`）、来源标签（「来自 微信」「助手发送 · 来自 微信」）、`@` 选会话、设置 › 助手（含「助手记住的事」能看能改能删）、会话「⋮」加「不向助手汇报」、助手开的会话带小机器人标记。
11. 换主机：`assistant.export` / `assistant.import` + 设置里「助手跑在」。
12. `DesktopService.cpp` 的 daemon 交接条件加「助手开着」。
13. **语音接口留好但不实现**（§8）：内核输入输出统一是「一句话」的流、能取消、「说的」和「给看的」分开、`assistant.speaking` / `assistant.audio` 占位。
14. **Mac（P0 内）**：Friday.app 带 friday-server（CI 构建、签名 hardened runtime、公证；`Friday.app/Contents/Helpers/`；`latest.json` 不另出 macOS 服务端包）、`AssistantHost.swift`（`SMAppService` 登录项、拉起、引擎和密钥自动推）、Swift 的 `ActivityTap.swift` / `TurnBrief.swift`（规则同 C++）、`ManagedEndpoint.swift`、`friday-server --attach --app`、`RemoteDeploy` 认出 macOS（不下载服务端包，把 `~/.friday-server/bin` 指到 App 里的 helper；找不到 Friday.app 就提示在 Mac 上装 Friday）、Swift 助手页。
15. 测试：`tst_assistant`、`tst_turnbrief`、`tst_remote` 加 activity 与退回旧做法（§10）。

### P1 渠道

`ChannelHub` + `ChannelAdapter` + `ChannelRender`；适配器顺序 **SChat → Telegram → 微信 ClawBot → 企业微信 / 飞书 / 钉钉**；
绑定码（`/bind 123456`，只听主人私聊，别人发的忽略并记日志，群里默认只推送）、推送规则、合并、
**回复回执直达会话**（引用回复或纯文本 `#短号` 开头 → 直接 `runner.submit`，`from` 标渠道，回一句「已转给 #3f2a」，不经过大脑）、
在渠道里确认（受 `assistantApproveInChannel` 控制，只在能加按钮的平台出现，否则写「请到 Friday 里确认」）、
`/` 命令（`/sessions` `/machines` `/new` `/stop #短号` `/status` `/bind 绑定码`）、
daemon 交接条件加「绑了渠道」、`BriefWriter`（小模型写回执那一句，默认关）。
渠道 token / secret 存 `SecretStore`，`channels.json` 里只写密钥名。

### P2 语音

流式识别 / 合成 provider（百炼、方舟；做成新的能力 provider；现有 `SpeechReader` 系统 TTS 兜底）、VAD、语音模式界面（麦克风、实时字幕、波形、「停」）、
打断（用户一开口就算打断）、SChat 语音终端（m5core2 绑 `boundBotId`）。延迟目标：用户说完约 1 秒出声。录音默认不保存，只存识别出的文字。

### P3

QQ、Slack、Discord、Matrix、邮件；通用 Webhook / 本机 HTTP 接口（只绑 127.0.0.1，带令牌）；脚本渠道（stdin/stdout JSON 行协议 `{"type":"inbound",…}` / `{"type":"send",…}`）+ `create_channel` 元工具；向量检索（引擎支持 `/embeddings` 时）。

**可后置**：P1/P2/P3 全部；P0 里 `sync_config`、`add_machine` / `remove_machine`、`think_in_background` 可排在 P0 后段（不阻塞桌面端文字跑通）；
Mac 那一组（P0 第 14 项）与 Qt 侧并行，但**每个阶段 Qt 和 Swift 的界面一起做**（§13 末）。

---

## 9. 安全约束（§9，必须落地的检查点）

| 检查点 | 落地位置 |
|---|---|
| 助手默认**关**；打开时主机默认本机（可改） | `assistantEnabled` / `assistantHost`；关时 `assistant.*` 回错误 `3` |
| 删除会话、添加 / 移除机器、同步配置：**每次确认**（从聊天工具发起的就在聊天工具里确认） | `AssistantTools`：`manage_session(action=remove)`、`add_machine` / `remove_machine`、`sync_config` 一律先出预览 / 确认卡片，确认后才 apply；渠道侧走按钮，`assistantApproveInChannel` 关时回「请到 Friday 里确认」 |
| 在聊天工具里批准工具调用：默认**关**；开了也只在能加按钮的平台出现 | `ChannelCapabilities::buttons` + `assistantApproveInChannel` |
| 助手（大脑）自己批准工具调用：默认**关** | 关时**不注册** `resolve_approval` 工具 |
| 从聊天工具开 Full 权限的会话：默认**关** | `create_session` 的 `permissionMode` 来自渠道时钳制（`assistantFullPermissionFromChannel`） |
| **SSH 密码不走聊天工具**（会明文留在别家服务器） | `add_machine` 要密码时转到 Friday 里的安全输入框（`request_secret` 那套 / `RemoteKeySetup`），渠道里只回「请到 Friday 里输入」 |
| **防注入** | 回执、会话内容、渠道里别人转来的文字一律当**数据**：放在上下文的数据区、标明来源，不当指令；删除、批准这类动作**不能只凭模型一句话**（要有用户确认事件） |
| **防死循环** | 助手自己的输出不变成给自己的事件；自动唤醒有上限（每小时 30）；同一会话的同一种事件合并 |
| **审计** | 大脑的每次工具调用都留在助手时间线上（工具卡片，`JournalEntry::kind == Tool`）和日志里 |
| 只听主人私聊；别人发的忽略（记一条日志）；群里默认只推送、不接指令 | `ChannelHub`：`InboundMessage::fromOwner` / `isGroup` |
| 渠道密钥只以名字出现 | `channels.json` 只写 `tokenSecret`，值在 `SecretStore`；同现有密钥一样只给客户端传名字 |
| `assistant.export` / `import` 只给本地 socket | 同 `server.pair`：WebSocket 进来的回错误 `4` |
| 换主机时两边不同时开着 | 同一个机器人被两处同时长轮询会互相抢消息：新主机起来、各渠道连上后**旧主机上的停掉** |
| SChat 端到端加密的消息机器人收不到，助手对话在 SChat 服务端是明文 | 设置里写明（§7.4） |
| Swift 受管端 socket 只有同一个用户能连；其余方法回错误 `4` | `ManagedEndpoint.swift` |

---

## 10. 验收要点（设计稿里「必须如此」的硬性要求，逐条对照测试）

**内核 / 调度**
1. 助手是常驻内核（`AssistantRuntime`），**不是一个会话**；对话、事件、干活三条线分开，谁也不等谁。
2. 同一时间**只有一次反应**在跑；反应期间到的输入和事件攒着，下一次一起处理。
3. **用户一开口就打断**：正在生成的反应立即取消（引擎请求断开），已流出去的话保留并标「被打断」，语音播报立即停，带上这句话重来；已发出的工具调用不撤回；**不用** `AgentRunner::takeInterjection`。
4. 优先级：用户输入 > 立即级事件 > 及时级事件；用户正在输入 / 说话时事件触发的反应推迟。
5. 自动唤醒上限：非用户触发的反应每小时最多 30 次（设置），超了只更新状态板 + 固定格式推送。
6. 主机上没有任何客户端和渠道时也照常运转（事件照样记、推送照样发）。
7. 工具全是异步的：派活立即返回句柄；取数类 3 秒超时后返回「稍后作为事件给你」；**没有 `wait_*`**。
8. 每次反应工具调用**最多 2 轮**（设置），超了先回话、剩下的作为下一次反应。
9. 输出分「说的」（正文，短、口语化，语音时念出来）和「给看的」（`post_detail`，长内容 / 列表 / 链接，语音时不念）；文字渠道两部分都显示。

**上下文**
10. 每次调用大脑的输入**按当前状态现拼**（固定约 14k token），**不在历史后面接着加**；对话进行多久带进去的都是这么多。
11. 7 层顺序与预算照 §4 的表；变化少的在前（命中提示缓存）；各层超预算**在层内裁剪，不挤占别的层**；预算按模型上下文缩放但**最小不低于表里的值**。
12. 上下文最多带 **2 个**焦点项目的档案和摘要，其余项目只在状态板上占一行。
13. 焦点用 **30 分钟窗口**：最近一次被问到（`mentionedAt`）距今 ≤ 30 分钟（`WorldBoard::mentionWindowSeconds` = 1800）的会话算在焦点里，按问到时间新→旧取前 2 个；权重衰减（置 1 / 乘 0.7 / 低于 0.2 移出）不再使用（2026-10-07 按上游 v2.3 实现对齐）。
14. `read_session` 取来的内容**只用于这次反应，不写进最近对话**；日志里记一句「看了 #3f2a 最近 5 条」。
15. 项目细节留在各自会话里，助手只拿指针和一行状态；A 项目以什么为准看 A 的会话，不看助手的记忆。

**事件与回执**
16. 每个会话有动静就推**轻量 `activity` 事件**，**不推整条消息**；主机对远程只订 `activity`，不订 `message` 快照。
17. `tool` 事件**每个会话最多 1 条 / 3 秒**。
18. **回执不每条过大脑**：状态和数字**永远由代码写**；「说了什么」那一句默认按规则挑（`pickLine` 的 5 条规则逐条可测）；小模型写这一句**默认关**，开了也要 2 秒超时退回、数字对不上就丢掉。
19. 模型挂了 / 等额度 / 很慢时**推送不能停**（代码模板不经模型）。
20. 要说人话时**批量过一次大脑**：用户在等的、要紧的、同一项目扎堆的（合并间隔 30 秒内 ≥ 3 条）、用户隔了一阵（30 分钟）回来时；合并后过的那一次在自动唤醒上限里只算一次，回来时批量讲不算。
21. 用户正在桌面端前台看着那个会话时，这个会话的事件**只上状态板**，不叫醒、不推；客户端断开即视为没在看。
22. 推送规则默认「只推助手派的、出错的、要确认的」。
23. **断线不补事件，按状态校正**：重连后重新 `activity.subscribe` 拿快照，和状态板一比就知道断线期间谁结束了（快照带每个会话最后一次的简报和时间），照常出回执；状态板是「状态」不是「日志」，丢事件不影响对错。
24. 老版本服务端（hello 里没有 `activity`）退回比对 `runner.generating` 和 `summaries`，结束时订阅一下拿最后一条消息再退订。
25. `activity` 事件**不合批**，一到立即直发给订了的客户端（直发前先 flush 攒着的状态；其余状态事件仍走 80 ms 合批）（2026-10-07 按上游 v2.3 实现对齐）。

**界面**
26. 侧边栏最上面固定一行「助手」：未读回执数、渠道连接状态；**不进工作目录分组**；主机是别的机器时显示「助手 · mac-mini」。
27. 时间线按天分隔、往上翻按页加载（`assistant.history`）、顶上按项目筛选；用户消息上方标来源，助手回复下面标「已回复到 微信」。
28. 回执行：一行一条、不用气泡；左边状态图标（完成绿 / 要确认橙 / 出错红 / 停了灰）；连续几条折叠成「另外 N 条」；要确认的那条直接带「批准 / 拒绝」（走 `runner.resolveApproval`）。
29. 状态板和大脑每次看到的是**同一份**；按机器分组，在跑 / 要确认 / 出错的在前，空闲的折叠成数字；点一行打开那个会话；能停靠（卡片 id `board`）。
30. 界面上的状态板渲染超预算（约 1.5k token）时从最旧的空闲开始省略；末尾有「上次跟用户说话之后」。
31. `@` 选会话：插入会话标签，发出去**直接给那个会话，不经过大脑**。
32. 普通会话里：助手转来的消息标「助手发送 · 来自 微信」；助手开的会话侧边栏带小机器人标记；会话「⋮」菜单有「不向助手汇报」。
33. 会话跑完时的**桌面系统通知**随 `turnFinished` 一起做，**不开助手也有**。
34. 在另一台机器上打开助手时提示「助手在 mac-mini 上跑」，给「改到这台」。

**主机 / 部署**
35. 助手**全局一个**，只在主机上跑；别的客户端连到主机去看它（`assistant.*`）；没开助手的机器回错误 `3`。
36. Windows / Linux：桌面端开着时在 `DesktopService` 里；退出时 daemon 交接条件**加「助手开着」**（P1 再加「绑了渠道」）。
37. 换主机 = 搬家：`assistant.export` / `import` 传 `Assistant/` 全部文件，密钥按名字重新同步（`secrets.set`），新主机起来、各渠道连上后**旧主机上的停掉**；两边不同时开着。
38. macOS：助手内核跑在 Friday.app 自带的 friday-server 里，用**单独的数据目录** `~/Library/Application Support/Friday-Assistant/`（`FRIDAY_DATA_DIR`），不碰 Swift 的数据；`SMAppService` 注册成登录项，Friday.app 退出后渠道照常在线，关掉助手就注销；引擎 / 密钥 / `defaultEngineID` 由 Swift App 自动推过去。
39. Swift App 兼当受管端：本机 socket 上实现协议子集，其余方法回错误 `4`；App 没开时后台拉起（`open -g -j -b`，不弹窗口）；用户退出 App 后 Mac 本机会话停下，状态板显示「本机离线」。
40. `RemoteDeploy` 发现对方是 macOS 就不下载服务端包，把 `~/.friday-server/bin` 指到 App 里的 helper；找不到 Friday.app 就提示在 Mac 上装 Friday。

**大脑 / 模型**
41. 大脑和写回的模型都能选，**默认「跟新对话一样」**（取主机上 `defaultEngineID`，没有就第一个引擎，同 `EngineStore::resolved`；用户改了新对话的模型，助手跟着变）。
42. 大脑要能调工具（`supportsTools`）：不支持时设置页标红、提示另选，助手**先只回话不派活**。
43. 大脑**默认不开思考**（首字延迟要紧，可在设置里开）；语音模式下设置页提示「首字慢可以给大脑换个快模型」。
44. 大脑调用复用 `OpenAIChatEngine`（流式、取消）；`think_in_background` 复用 `SubAgentRunner`。

**存储**
45. 助手**不进 `conversations.json`**；单独的 `Assistant/` 目录，日志按天追加、按页读；日志不删；设置里给「清空助手的日志和记忆」。
46. 重启后：状态板不落盘（无 `board.json`）、连上各机器后按 `activity.subscribe` 校正；最近对话从日志尾部读；`state.json` 只存 `summarizedUntil`（焦点不落盘，未读数只在客户端内存，渠道游标在 `channels.json`）（2026-10-07 按上游 v2.3 实现对齐）。

**渠道**
47. 所有渠道、桌面端、语音共用**一个助手对话**（同一份日志、同一个焦点、同一份记忆），每条标来源，**从哪来回哪去**；回执推到设置里的「通知渠道」（可多选）。
48. **回复某条回执 = 直接跟那个会话说话**（引用回复，或纯文本里 `#短号` 开头），不经过大脑。
49. 认主人：粘好 token（或扫码登录）后显示 6 位绑定码，聊天工具里 `/bind 123456` 成为主人（可绑多个账号）；**只听主人私聊**。
50. 适配器用 core 现有的 `Http`（含 `streamLines`）和 QtWebSockets，**不引入 Node**；一个平台出错时状态标红、**不影响别的渠道**。
51. SChat：`getUpdates` 长轮询（`offset`、`limit ≤ 100`、`timeoutSeconds ≤ 25`），游标存 `state.json`；`sendMessage` 必填 `clientMsgId`（UUID v4）、**只有纯文本**；流式 `streams` → `/:id/append {index, delta}`（index 连续）→ `/finish` / `/abort`，一个会话同时只能一个流、每秒最多 10 次 append、总长 64 KiB、空闲 120 秒自动结束；inline 键盘最多 10 行 × 4 个，点击是 `callback` 更新、10 分钟内 `answerCallback`；**私聊里点按钮 `callback.chatKey` 是 `user:<botId>`，回复要发给 `user:<fromUserId>`**；限流每会话 5 秒 5 条、全局每秒 30 条；自签证书按 SPKI 指纹认（`GET /server-info`）。
52. Telegram：`sendMessageDraft` 只是 30 秒临时预览，**说完必须再 `sendMessage` 落成正式消息**。
53. 飞书：流式用 CardKit 卡片实体（编辑普通消息有隐性次数上限，长回复会静默失败）。钉钉：AI 卡片流式要 `Card.Instance.Write`、`Card.Streaming.Write` 权限。企业微信：`wss://openws.work.weixin.qq.com`，连上先发认证帧（botId + secret），心跳、指数退避重连，流式单条上限 20480 字节。
54. 微信 ClawBot 协议没有公开文档、可能会变：**全部关在适配器里**。

**语音（P2，接口 P0 留好）**
55. 内核的输入输出统一是「一句话」的流；语音只是又一个适配器；以一句话为单位收发、能取消、「说的」和「给看的」分开从 P0 就做进内核。
56. 用户一开口就算打断（停播报、取消反应）；「给看的」不念。延迟目标：用户说完约 1 秒出声。

### 10.1 测试（§12 末）

- `tst_assistant`：Triage（合并、回来时批量、前台在看）；ContextBuilder 的预算（各层不超、不挤占、缩放后不低于下限）；Journal 分页（按天、按项目筛、`before` 游标）；WorldBoard 从事件和快照校正（丢事件也对）；打断（用户输入取消在跑的反应、已流出文字标 interrupted）。
- `tst_turnbrief`：`pickLine` 的各种结尾（问句、代码块、表格、客套话、只有任务状态、链接留文字、80 字截断在标点）；小模型的数字校验和 2 秒超时退回。
- `tst_remote`：加 `activity.*`、`conversation.tail`、`runner.submit` 的 `from` / `whenBusy`、`assistant.presence`、以及**退回旧做法**；渠道适配器对假服务端（SChat 的长轮询 / streams / callback chatKey 陷阱）。
- 登记方式：每个 `tst_*.cpp` 定义一个 QObject 测试类并 `FRIDAY_TEST(类名)`（`qt/tests/TestRegistry.h`），`file(GLOB)` 自动收。
