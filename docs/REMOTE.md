# 远程 Friday：friday-server 与客户端协议

v2.1 新增。Agent 在远程机器上一直跑，本机只负责显示和操作：本机关掉，远程的任务照常进行；再连上时看到的是远程的全部会话、
正在生成的内容、待确认的调用和后台任务。

这份文档是**协议的唯一真相来源**：Qt 客户端（`friday --remote …`）、Swift 客户端（macOS），以及以后的手机 App 都按它实现。
改协议先改这里，再改两边代码；不兼容的改动要把 `protocol` 加一。

## 1. 角色

```
客户端（界面，可以有多个，可以随时断开）                  远程机器（Linux / Windows / WSL）
  Qt：侧边栏底部切到远程机器（§11）            ┐
  Swift：同上                                    ├── 字节流 ──→ friday-server（常驻、独占数据目录）
  以后：手机 App                                ┘               或开着的 Friday 桌面端（Qt 版，§12）
                                                                FridayCore：会话、AgentRunner、工具、MCP、密钥
```

- **friday-server**：没有界面，链接 `friday_core`（和图形界面同一套核心）。v2.8 起桌面端、命令行、服务端都叫 `friday`：
  `friday server <选项>`（v2.6–v2.7 是 `friday-cli server`）；程序文件叫 `friday-server` 时直接是服务、参数不带 `server`——远程自动装的
  服务端包里 `bin/friday` 旁边放着这个名字的别名（`src/cli/ServerCommand.h`），所以远程的布局、客户端运行的命令和以前一样。它拿数据目录的锁
  （`util/InstanceLock.h`），所以同一个数据目录上不能同时开命令行的 friday；同时开 Qt 桌面端时由桌面端接过来（§12）。没有客户端连着时照常干活：
  生成、子 Agent、排队的消息、等额度恢复后接着做。遇到要确认的调用就挂起，等有客户端连上来确认（默认行为，§7）。
- **Qt 桌面端兼当服务端**（§12）：那台机器上开着 Friday 桌面端时，连上来的是桌面端自己（同一个本地 socket 名、同一个 WebSocket 入口），
  桌面端和远程客户端实时看到同一批会话；关掉窗口桌面端还在托盘里接着服务，退出就全停（v2.8 起不再交给后台的 friday-server）。
- **客户端**只是一个视图：不读写远程的文件，不在本机执行工具，一切通过本协议。会话内容、Agent 的运行状态都以服务端为准。
- 工具、MCP、密钥、在线大模型的请求都在远程机器上执行 / 发出；密钥只存在远程的密钥库里。

## 2. 传输

协议本身只要求一条**双向、有序、可靠的字节流**，与传输方式无关：

| 传输 | 客户端怎么连 | 认证 |
|---|---|---|
| SSH 到 Linux / macOS | `ssh -T -o BatchMode=yes user@host '~/.friday-server/bin/friday-server --attach'` | SSH 自己（密钥 / ssh-agent / 指定的私钥文件；密码只在添加时用一次，用来装上本机的公钥，§11） |
| SSH 到 Windows | cmd：`"%USERPROFILE%\.friday-server\bin\friday-server.exe" --attach`；PowerShell：`& "$env:USERPROFILE\…" --attach` | 同上 |
| WSL | `wsl.exe -d <发行版> --exec sh -c 'exec "$HOME/.friday-server/bin/friday-server" --attach'` | Windows 当前用户 |
| WebSocket | `wss://host:7443`（没有 TLS 时 `ws://`），一条文本帧就是一条消息 | 配对令牌（`hello.token`）+ 证书指纹（§8） |

SSH 连接前客户端先跑一次 `echo FRIDAY-PROBE %OS% $env:OS` 探测远程的 shell（cmd 展开 `%OS%`、PowerShell 展开 `$env:OS`、
sh 都不展开），按它拼 friday-server 的路径。远程还没装 friday-server 时客户端自动装上去（§9）。

`friday-server --attach` 是一个桥：连上本机的服务（本地 socket：Unix 域套接字 / Windows 命名管道，只有同一个用户能连；
开着的 Qt 桌面端也在这个名字上提供服务，§12），把自己的标准输入 / 输出接过去；都没在跑就先把 friday-server 拉起来（脱离当前会话，SSH 断开后照常运行。Windows 的 OpenSSH
会在会话结束时杀掉整个作业对象：先试脱离作业，不允许就经 WMI 的 Win32_Process.Create 起，挂在 WmiPrvSE 下面；
常驻进程只继承它自己那三个指向 NUL 的标准句柄（PROC_THREAD_ATTRIBUTE_HANDLE_LIST），不然 sshd → cmd 一路传下来的可继承句柄里
指向会话管道的那些被它拿着，sshd 等不到管道关闭，`--devices` 这类管理命令现拉起服务后 ssh 就不返回）。管理命令（`--websocket`、
`--pair`、`--devices`、`--revoke`）没在跑时也这样拉起服务。
SSH / WSL 不开任何网络端口，认证、加密、跳板机、`~/.ssh/config` 全交给 SSH；WebSocket 入口默认关着（§8）。

**分帧**：字节流上每条消息是一行紧凑的 UTF-8 JSON，以 `\n` 结尾（JSON 里的换行都是转义过的）。WebSocket 上一帧一条，不加 `\n`。

## 3. 消息格式（JSON-RPC 2.0）

```jsonc
// 请求（客户端 → 服务端；id 是客户端自己编的整数，同一条连接里不重复）
{"jsonrpc":"2.0","id":7,"method":"conversation.subscribe","params":{"id":"5B0C…"}}
// 回复
{"jsonrpc":"2.0","id":7,"result":{…}}
{"jsonrpc":"2.0","id":7,"error":{"code":3,"message":"没有这段会话"}}
// 事件（服务端 → 客户端，没有 id）
{"jsonrpc":"2.0","method":"message","params":{…}}
```

- 回复的顺序不保证和请求一致，按 `id` 对。同一条连接上，服务端在回复某个请求之前发出的事件，一定反映了处理这个请求之前的状态；
  回复之后的事件反映处理之后的状态。
- 错误码：JSON-RPC 的 `-32700`（不是 JSON）、`-32600`（格式不对）、`-32601`（没有这个方法）、`-32602`（参数不对）、`-32603`（内部错误）；
  本协议的 `1`（协议版本不对）、`2`（还没 hello）、`3`（找不到会话 / 消息 / 子 Agent）、`4`（现在不能这么做，`message` 说明原因）、
  `5`（WebSocket 进来的设备没配对、令牌不对或已被吊销：回完就断开，客户端不再自动重连）。
  `message` 一律是给用户看的中文。

**数据格式**：与 `conversations.json` 相同（ARCHITECTURE.md §5，Swift Codable 规则）——UUID 是大写字符串，日期是 ISO 8601，
`Conversation`、`ChatMessage`、`EngineConfig` 等直接用存档里的 JSON。Qt 和 Swift 两边本来就能互读这些 JSON，客户端不用另写一套模型。
可选字段没有值时不写键。

## 4. 握手

连上后第一条必须是 `hello`，之前的其他请求回 `2`。

```jsonc
// 请求
{"protocol":1,"client":{"name":"Friday","version":"2.1.3","platform":"windows","kind":"desktop"},"features":["messagePatch"],"token":"…以后用…"}
// 回复
{"protocol":1,
 "server":{"version":"2.1.3","platform":"linux","arch":"x64","hostname":"ubuntu","user":"root","home":"/root",
           "dataDir":"/root/.local/share/Friday","pid":4242,"startedAt":"2026-10-05T12:00:00Z"},
 "state":{"summaries":[…],"runner":{…},"team":{…},"engines":{…},"secrets":[…],"directories":{…},"terminals":{…}}}
```

`protocol` 不一样时服务端回错误 `1`，`message` 里写明两边的版本；客户端据此换一个匹配的 friday-server（§9）。
经 WebSocket 连进来的必须带 `token`（配对时拿到的），对不上回错误 `5`。`server` 里另有 `defaultWorkingDirectory`
（没给会话选工作目录时用的目录，远程的 设置 › 智能体），以及 `host`：`"server"` 是 friday-server，`"desktop"` 是开着的 Qt 桌面端（§12；
客户端据此显示「friday-server 2.1.12」或「Friday 桌面端 2.1.12」，也不对桌面端自动升级），`"swift"`（2.3 起）是 Mac 上的 Swift 版 Friday
（受管端：只实现给助手用的那部分方法，其余回错误 `4`，ASSISTANT.md §11.3；客户端不切过去看它的会话、不对它自动升级）。
没有这个键的是 v2.1.11 及以前的 friday-server。
`state` 是全量快照（§5），之后靠事件保持同步。
`server.features`（2.3 起）列出可选功能，客户端据此决定用不用：`activity`（`activity.*`，§6「会话的实时动静」）、
`tail`（`conversation.tail`）、`submitFrom`（`runner.submit` 的 `from` / `whenBusy`）、`assistant`（`assistant.*`，§6「助手」；
2.3.2 的服务端已经有 `assistant.*` 但 features 里还没写，客户端直接调、`-32601` 再当没有）、
`services`（`services.setEnabled`、EnginesState 的 `disabledServices`，§5、§6「密钥、引擎」）、`powers`（`powers.*`，§6「本地能力」）、
`messagePatch`（增量消息，§5「增量消息」）、`config`（2.4 起：`config.*`、`tools.*`、`capabilities.*`、新的 `mcp.*`，`settings.update` 认工作目录 /
工作方式 / 停下来，§6「设置页的配置」）、`serviceOps`（2.4 起：`services.install` / `apply` / `uninstall` / `installCustom` / `fetchModels`，
§6「在线服务」）、`serviceUsage`（2.6 起：`services.usage`，服务的余额、套餐用量，§6「在线服务」）、`models`（2.8 起：`models.*`，§6「本地模型」）、
`storage`（2.8 起：`storage.*`，§6「文件存放位置」）。没有这个键的服务端都不支持；可选功能不改 `protocol` 版本号。
客户端在 hello 的 `features` 里说自己会什么（2.3 起：`messagePatch`）：服务端只对说了的发增量，没说的照旧发整条。

## 5. 状态与事件

**规则**：服务端是唯一的真相来源；每个事件带的是它所指那样东西的**完整快照**（不是增量），重复收到、乱序覆盖都没关系。
例外是声明了 `messagePatch` 的客户端收的 `message.patch` 和 partial 的 `conversation`（见下面「增量消息」）：它们带版本号，对不上就要整条。
客户端可以先在本地改（乐观更新），之后以服务端的事件为准。断线重连不补发错过的事件：重新 `hello` 拿全量、重新订阅打开着的会话即可。

| 事件 | params | 什么时候发 |
|---|---|---|
| `summaries` | `{"summaries":[Summary]}` | 会话列表或某段会话的元信息变了（新建、删除、改名、置顶、时间、目录、模式、大脑……） |
| `conversation` | `{"conversation":Conversation}` | 订阅了的会话结构变了（加消息、分叉、切分支、会话级设置） |
| `message` | `{"conversationID":…,"message":ChatMessage,"version"?}` | 订阅了的会话里某条消息的内容变了（流式输出；同一条消息合并，每秒最多十几次）。`version` 只给声明了 `messagePatch` 的 |
| `message.patch` | `{conversationID, messageID, base, version, fields?, blocks?}` | 2.3 起，只给声明了 `messagePatch` 的：同上，但只带变了的部分（见下面「增量消息」） |
| `runner` | RunnerState | 有对话开始 / 停止生成、待确认的多了或少了、计量变了 |
| `runtime` | `{"conversationID":…,"runtime":Runtime}` | 订阅了的会话的运行状态变了（排队、可不可以重试、生效的大脑和档位…） |
| `team` | TeamState | 子 Agent 登记 / 注销、状态、步数、速度、排队的消息变了 |
| `engines` | EnginesState | 在线引擎增删改（含密钥到了以后自动接入的）、新对话默认的大脑、子 Agent 可以用的大脑、关着的在线服务变了 |
| `secrets` | `{"names":[…]}` | 密钥库里的名字变了（只有名字，值永远不出服务端） |
| `directories` | DirectoriesState | 这台机器最近选择的工作目录变了（2.1.13 起） |
| `terminals` | TerminalsState | 内置终端开了、关了、标题或状态变了（2.2 起，TERMINAL.md） |
| `term.output` | `{"id":…,"data":base64}` | 接着的终端（`term.open` / `term.attach` 过的）有输出：原始字节，马上转、不合并 |
| `attention` | `{}` | 出现了要用户确认的调用：客户端提醒用户（任务栏闪烁、通知） |
| `activity` | ActivityEvent | 订了 `activity.subscribe` 的客户端：一段会话的动静（2.3 起，§6「会话的实时动静」）。不合批，马上转 |
| `assistant.entry` | `{entry: JournalEntry}` | 订了 `assistant.subscribe` 的客户端：助手日志里加了 / 改了一条（流式的回复同一条合并，80 毫秒一批；2.3 起，§6「助手」） |
| `assistant.board` | `{board: Board}` | 〃：状态板变了 |
| `assistant.state` | AssistantState | 〃：开关、设置、在不在说、大脑用的模型变了 |
| `assistant.push` | `{text, kind, targets}` | 〃：助手要推给用户的（`kind`：reply / notice / receipt / ack（直达会话的「已发给」）；`targets` 是回到的渠道 id——空的才在客户端弹通知）。马上转 |
| `assistant.channels` | `{channels: [Channel]}` | 〃：渠道的配置、连接状态、绑定的人变了 |
| `assistant.reset` | `{}` | 〃：日志、记忆清空了（`assistant.clear` 或主机上清的）、换成搬来的了（`assistant.import.finish`）：重新 `assistant.subscribe` 拿全量 |
| `powers` | PowersState | 订了 `powers.subscribe` 的客户端：这台的本地能力装了哪些、状态、下载进展变了（80 毫秒合并一次，没变不发；2.3 起，§6「本地能力」） |
| `models` | ModelsState | 订了 `models.subscribe` 的客户端（客户端连上就订）：这台的模型库、本地模型的下载进展、在等下完的对话、本地大模型能不能用变了（80 毫秒合并一次，没变不发；2.8 起，§6「本地模型」） |
| `modelsFailed` | `{id, message}` | 只发给调 `models.use` / `models.download` 的那个客户端：在这台上下载 / 换上失败了（取消的不发；2.8 起） |
| `storage` | StorageState | 看过这台文件存放位置（`storage.get`、`storage.setModels`、`storage.adoptModels`）的客户端：搬模型的进度、结果变了（80 毫秒合并一次，没变不发；搬的途中不带 `leftoverModels`；2.8.7 起，§6「文件存放位置」） |
| `config` | `{settings?, tools?, mcp?, capabilities?}` | 订了 `config.subscribe` 的客户端：这台的智能体设置、工具、MCP 服务、能力提供方哪一份变了就带哪一份（整份；80 毫秒合并一次，和上次发的一样不发；2.4 起，§6「设置页的配置」） |
| `bridgeError` | `{"message":…}` | 只由 `--attach` 桥发：连不上 / 拉不起服务（`message` 里附服务日志的最后几行），随后桥退出 |

同一轮合并发送时，`conversation` 排在 `message` 前面（新消息先出现在结构里，再收到它的内容）。

**增量消息**（2.3 起，`messagePatch`；用户 2026-10-09「连接远程服务器容易卡死」）。子 Agent 的运行记录每 0.1 秒写进主 Agent 那条消息的工具卡片，
以前每 80 毫秒整条重发：一轮 6 个子 Agent、55 秒就推了 113 MB（这段会话存盘才 0.56 MB），慢网络上越积越多，回复排在几十 MB 旧快照后面，界面像卡死。
现在声明了 `messagePatch` 的客户端只收变了的部分，同样一轮 3.7 MB：

- 服务端给每段被订着的会话记一份**基线**：每条消息上次推出去的样子和版本号（全局递增）。消息变了：版本加一；客户端手里的版本（记在服务端）
  等于变之前的版本就发 `message.patch`（`base` = 变之前、`version` = 现在），否则发整条 `message`（带 `version`）。
- `message.patch` 的 `fields` 是消息除 `blocks` 以外的全部字段（没变就没有）；`blocks` 是一层块的补丁：`ids`（这一层块的完整顺序，
  增删、换顺序时才有）、`set`（整块换掉的：新加的、变了的文字 / 思考 / 媒体块，运行记录有无变了的工具块）、
  `tools`（工具块只改了一部分：`[{id, tool?（ToolEvent 去掉 transcript）, transcript?（下一层的块补丁）}]`），一层层递归进子 Agent 的运行记录。
  实现：Qt `core/remote/MessagePatch`，Swift `Remote/MessagePatch.swift`（客户端只打补丁）。
- 客户端手里的版本对不上 `base`、或者打不上（缺块、种类不对）：调 `message.get {conversationID, messageID}` 拿整条
  （`{message, version}`），这期间来的这条消息的补丁不管。
- `conversation` 快照也只带客户端没有或变了的消息：`{conversation（nodes 只有这些）, partial: true, nodeIDs（全部消息的顺序）, versions}`，
  其余用客户端手里的；手里缺了就重新 `conversation.subscribe` 拿整段。订阅回复是整段，另带 `versions`（之后的补丁从这些版本算）。

**积压**（2.3 起）：一个客户端还没写出去的超过 256 KB（本地 socket 看 `bytesToWrite`，WebSocket 自己记）时，`conversation` / `message` /
`message.patch` 先不发，记成欠着的；写出去了（`bytesWritten`）再补最新的（整条或快照，补丁的版本断了）。小事件（runner、runtime、summaries、
回复……）照常发。经 `--attach` 桥时，ssh 慢了桥写标准输出会阻塞、不再读 socket，积压一样传回服务端。
同样每秒只读 500 KB 的旧客户端：以前一轮推 113 MB、要追好几分钟、服务端内存涨到 173 MB；现在 15.6 MB、跟得上、内存不涨。

**超时与心跳**（2.3 起）：客户端的请求等不到回复的到时间以错误回调（`hello` 30 秒、`file.*` 3 分钟、2.4 起要在那台上跑一阵的——脚本保存 / 自测 / 回滚 / 新建、提供方自测 / 导入 / 导出、拉模型列表——10 分钟，其余 60 秒），`hello` 等不到就断开重连。
WebSocket 两边都发 ping：客户端每 15 秒、45 秒没有任何回音（消息、pong）就断开重连；服务端每 30 秒、90 秒没有回音就断开它（半断的连接不会一直积压）。
SSH 靠 `ServerAliveInterval=15`、`ServerAliveCountMax=3`。

**只订看着的**（2.3 起）：客户端切到别的机器后，原来那台退订它选中的会话（`conversation.unsubscribe`），后台照样连着（侧边栏、待确认、运行状态照常更新），
切回来重新订阅；助手用的连接不订阅会话。

**Summary**（== `ConversationSummary`，侧边栏要的元信息，不含消息）：
`{id, title, updatedAt, pinned, isEmpty, hasDraft, workingDirectory?, workingDirectoryName?, permissionMode, thinking?, teamMode?, engineID?}`
（`teamMode` 2.9 起：这段对话的任务模式，没有 = 跟随那台的新对话默认），
按 `updatedAt` 从新到旧排好。

**RunnerState**（全局）：
`{generating:[会话id], stopping:[会话id], pendingApproval:[会话id], backgroundAgents:[会话id], liveAgents, liveSpeed, queueRevision}`。

**Runtime**（某段会话的运行状态，服务端算好，客户端不重算业务规则）：

```jsonc
{"generating":false,"stopping":false,
 "queue":[QueuedInput],"queuePaused":false,
 "pendingApproval":false,
 "failedMessageID":"…",          // 停在一条出错的助手消息上（没有不写）
 "canRetry":true,
 "pendingTurnChanges":{"model":false,"thinking":false},
 "canSend":true,
 "engine":EngineConfig,           // 这段对话实际用的在线引擎（一个都没接入时不写）
 "thinkingLevel":"medium","thinkingLevels":["off","low","medium","high"],
 "permissionMode":"auto","workingDirectory":"/root/project",   // 没选目录时不写
 "mainLiveSpeed":0}
```

QueuedInput：`{id, text, attachments:[服务端路径], createdAt, from?}`。
「在等额度」那一行的文字由客户端按出错消息的 `quotaWait`（存档字段）和当前时间自己算（文案见 `AgentRunner::quotaWaitText`），
因为剩余时间每分钟都在变。

**TeamState**：`{"runs":[Run]}`，Run：

```jsonc
{"id":"…","name":"…","parentKey":"…","conversationID":"…","background":true,"depth":1,
 "running":true,"detached":true,"stopRequested":false,
 "activity":{"kind":"tool","title":"执行命令"},      // generating | tool | waitingChildren | waitingApproval | queued；没有不写
 "steps":5,"started":"…","lastOutput":"…","deadline":"…",
 "speed":12.3,"passTokens":420,"queue":[QueuedInput]}
```

状态行（「在执行「执行命令」 · 12 秒前有动静 · 第 5 步 · 已运行 3 分 2 秒」）由客户端按这些字段和当前时间算，每秒刷新不用服务端推。

**EnginesState**：`{"engines":[EngineConfig],"defaultEngineID":"…","disallowedSubAgentEngines":["…"],"disabledServices":["…"]}`。EngineConfig 里只有密钥的名字（`apiKeySecret`），没有值；
关掉的服务的模型带 `"disabled": true`（2.3 起；开着的不写这个键）：配置留着，模型菜单里不列，对话、子 Agent、助手都不用它。
2.5 起 EngineConfig 可以带 `tier`（`fast` / `balanced` / `strong`）、`billing`（`plan` / `metered` / `free`）：用户在模型的高级设置里改过的档位和计费，
没设不写（按模型名、接口地址推断，ASSISTANT.md §15.6）。
`disallowedSubAgentEngines`（2.3 起）是这台机器上不给子 Agent 用的大脑（设置 › 服务与密钥「子 Agent 可以用的服务」里关掉的，界面上按服务开关、
一家的模型一起改，以后这家新加的模型跟着它）：子 Agent 在服务端跑，客户端的设置页看着远程时看、改的都是这一份（`engines.setSubAgentAllowed`），
不是客户端本机的；没有这一项的是旧服务端，当成全部允许。
`disabledServices`（2.3 起）是这台机器上接入了、但关着的在线服务的 id（目录服务的 id，同一家重复添加的第 n 个是 `<目录 id>#n`（2.4 起，密钥名 `<目录密钥名>_n`），
自定义服务是 `custom-<密钥名或接口地址>`；
有模型的服务看模型、只有能力提供方的看提供方）：设置 › 服务与密钥 每行的开关看着远程时看它、经 `services.setEnabled` 改；没有这一项的是旧服务端，开关不能用。

**DirectoriesState**（2.1.13 起）：`{"recent":[{"path":"/root/project","exists":true}]}`，这台机器最近选择的工作目录（最新在前，最多 10 条；
就是它自己 settings 里的 `recentDirectories`：在那台机器上开着的桌面端、连它的各个客户端看到、改的是同一份），`exists` 按发出时看。
客户端的工作目录菜单看着远程时列这一份：选了目录发 `directories.push`，「×」发 `directories.remove`，打开菜单时 `directories.get` 重新看在不在。
hello 的 `state` 里没有 `directories` 的是旧服务端：菜单里不列，选了的也不记。

**TerminalsState**（2.2 起，TERMINAL.md）：`{"terminals":[TerminalInfo],"available":true,"reason"?:"…"}`，这台机器上全部内置终端（各段会话的）。
TerminalInfo：`{id, conversationID, title, shell, command?, running, exitCode, cols, rows}`（`command` 是 Agent 用 run_in_terminal 开的那条；
`shell` 是服务端算好的标题，客户端没有更好的标题时显示它）。经 WebSocket 连进来的客户端 `available` 为假、`reason` 写明原因（终端就是这台机器上的
shell，同配对：只有登录了这台机器的连接可以用），`term.*` 一律回错误 `4`。hello 的 `state` 里没有 `terminals` 的是 2.1.x 的服务端。

## 6. 方法

除 `hello` 外，`params` 里的会话 / 消息 / 子 Agent 一律用 UUID 字符串。没写返回值的回 `{}`。

**会话**

| 方法 | params | result |
|---|---|---|
| `conversation.subscribe` | `{id}` | `{conversation, runtime, versions?}`；之后这段会话的 `conversation` / `message` / `runtime` 事件发给这个客户端（`versions`：声明了 `messagePatch` 的才有，§5「增量消息」） |
| `message.get` | `{conversationID, messageID}` | `{message, version}`：整条消息（补丁对不上时要；2.3 起） |
| `conversation.unsubscribe` | `{id}` | |
| `conversation.create` | `{id, workingDirectory?, permissionMode?, thinking?, teamMode?, title?, engineID?, createdBy?}`（`teamMode` 2.9 起） | `{id}`：用客户端给的 id 新建空白会话（id 已存在就什么也不做）。客户端自己判断要不要复用空白会话。2.3 起可带标题（发第一条消息时保留它、不起临时标题）、只给这段的模型和深度思考档位（`thinking`：off / low / medium / high / ultra，不写跟随这台「上次选的」）；`createdBy: "assistant"` 是助手开的。客户端新建的会话带上界面上显示的权限、深度思考（客户端本机「上次选的」，见下） |
| `conversation.tail` | `{id, count?, maxChars?}` | 当前路径最后 `count`（默认 5，最多 50）条的压缩版（2.3 起）：`{messages:[{id, role, createdAt, text, sentBy?, error?, tools:[{name, title, status, summary, result}]}], truncated}`。不含子 Agent 交回的；文字合计不超过 `maxChars`（默认 4000），每条最多 2000 字（助手的留结尾、用户的留开头）；工具只列顶层的最后 8 个，结果只留第一行 |
| `conversation.fork` | `{id, messageID, newID}` | `{id}`：从某条消息处 fork 成 `newID` |
| `conversation.remove` | `{id}` | 停掉它的生成和子 Agent、删会话和它的媒体文件 |
| `conversation.rename` | `{id, title}` | |
| `conversation.setPinned` | `{id, pinned}` | |
| `conversation.setDraft` | `{id, draft}` | 草稿存在服务端，换一台设备接着写 |
| `conversation.search` | `{query}` | `{hits:[{conversationID, messageID?, title, snippet}]}` |

**对话的运行**（和本机 `AgentRunner` 的同名方法一一对应）

| 方法 | params |
|---|---|
| `runner.submit` | `{conversationID, text, attachments:[服务端路径], replacing?, from?, whenBusy?}`：空闲就发，忙就排队（== 输入框）。2.3 起：`from: {kind: "assistant", label}` 是替用户发的（label 如「助手 · 微信」，记进用户消息的 `sentBy`，界面标「助手发送 · 来自 微信」）；`whenBusy: "queue"` 忙时排到下一轮（不在这一轮做完当前一步时插进去），`"interject"` 同输入框 |
| `runner.stop` | `{conversationID, agents?}`：2.8 起 `agents: true` 连它派出去的后台子 Agent 一起停、停掉的不再新开一轮交回结果（助手 / 聊天工具里「让它停下」用；界面的停止键不带，仍只停这一轮） |
| `runner.stopAll` | `{}` |
| `runner.resolveApproval` | `{eventID, approved}` |
| `runner.removeQueued` / `runner.resumeQueue` | `{conversationID, itemID}` / `{conversationID}` |
| `runner.setEngine` | `{conversationID, engineID?}`（不写 = 跟随新对话默认）。2.5 起只改这段对话，不再同时改新对话的默认（改默认用 `settings.update` 的 `defaultEngineID`） |
| `runner.setPermissionMode` / `runner.setThinking` | `{conversationID, mode}` / `{conversationID, level}`。客户端那边同时把它记成本机「上次选的」（见下） |
| `runner.setTeamMode` | `{conversationID, mode}`（2.9 起，`server.features` 有 `teamModePerConversation`）：这段对话的任务模式（`solo` / `delegatesHeavy` / `delegatesOnly` / `delegatesWhenNeeded` / `ultra`，不认识的回 `-32602`），服务端同时记成它的新对话默认；客户端那边也记成本机「上次选的」。没有这个功能的旧服务端：客户端改那台的全局 `settings.update teamMode` |
| `runner.setWorkingDirectory` | `{conversationID, path?}` |
| `runner.selectBranch` | `{conversationID, child, parentKey}` |
| `runner.resetSession` / `runner.retry` / `runner.cancelQuotaWait` | `{conversationID}` |
| `agent.stop` | `{agentID}` |
| `agent.send` | `{agentID, text, attachments:[服务端路径]}` |
| `agent.removeQueued` | `{agentID, itemID}` |

**新会话的权限、深度思考**：「上次选的」记在客户端本机的设置里（`defaultPermissionMode`、`thinkingLevel`），本机和各台远程共用一份——
在哪台机器的会话里改了都记下来，新会话（本机的、远程的）都按它。客户端本地新建、还没在服务端建出来的空白会话跟着它显示
（连上远程时放的那段可能是很早建的，期间在别处改过），第一次用到时 `conversation.create` 带上 `permissionMode`、`thinking`；
「新对话」复用服务端已经有的空白会话时，客户端发 `runner.setPermissionMode` / `runner.setThinking` 把它改成现在的。

**会话的实时动静**（2.3 起，`server.features` 有 `activity`；助手用，docs/ASSISTANT.md §4）：

| 方法 | params | result |
|---|---|---|
| `activity.subscribe` | `{}` | `{sessions:[SessionActivity]}`：全部有内容的会话现在的样子（按更新时间从新到旧）；之后这个客户端收 `activity` 事件 |
| `activity.unsubscribe` | `{}` | |

- **SessionActivity**：`{id, title, workingDirectory?, status, task?: {goal, steps:[{text, status: todo|doing|done}]}, lastBrief?: TurnBrief, approvals:[PendingApproval], currentTool?, startedBy?, updatedAt}`；
  `status` 是 `idle` / `running` / `approval`（在等确认）/ `question`（上一轮以问句结束）/ `quota` / `error`。任务状态、最近的简报只记服务端这次运行以来跑过的会话的。
- **ActivityEvent**：`{conversationID, kind, seq, at, title, workingDirectory?, …}`，`kind`：
  `started`（一轮开始；`by`：`user` / `agent`（后台子 Agent 交回结果）/ `assistant`（替用户发的））、
  `step`（任务状态的目标或步骤变了；`goal`、`steps`）、`tool`（值得一提的工具开始了：跑命令、写文件、派子 Agent、上网、生成媒体；`tool: {eventID, tool, title, summary}`，每段会话每 2 秒最多一条）、
  `approval`（待确认的调用变了；`approvals`：现在全部的，空 = 都处理完了）、`quota`（开始等额度；`resumeAt?`）、
  `stalled`（生成中 300 秒没动静；`seconds`）、`finished`（一轮结束；`brief`：TurnBrief）。「出错」「在问你」「被停」都在简报的 `status` 里。
  2.5 起：一轮结束时派出去的子 Agent 还在后台跑（主 Agent 派完就收尾），这一轮不推 `finished`、会话还是 `running`——推一条 `tool`
  （`tool.tool` 是 `background_agents`，`title`「等后台的子 Agent 交回结果」），也不推 `stalled`；它们交回、主 Agent 说完那一轮才推 `finished`
  （简报的数字是这几轮加起来的，用时从第一轮算）。子 Agent 都没事了、主 Agent 却没再开一轮时，过 4 秒按留着的简报推。
- **TurnBrief**（一轮的简报，状态和数字由代码写）：`{messageID?, status: done|stopped|error|question|quota, line, question?, filesChanged, commands, commandsFailed, subAgents, seconds, goal?, stepsDone, stepsTotal, finishedAt?, answer?}`；
  `line` 是从这一轮最后一条回答里按规则挑的一句（docs/ASSISTANT.md §4.2）；`answer`（2.5 起）是最后那条回答去掉 Markdown 的正文（最多 1200 字），助手核对做到了没有用。
- **PendingApproval**：`{eventID, tool, title, summary}`。断线不补事件：重连后重新 `activity.subscribe` 拿快照校正。

**助手**（2.3 起；看这台机器上的助手——它是主机时，docs/ASSISTANT.md §11.2）：

| 方法 | params | result |
|---|---|---|
| `assistant.status` | `{}` | `{enabled, reacting}`：只看这台的助手开没开，不订阅（别的电脑提示「助手在这台上跑」） |
| `assistant.subscribe` | `{count?}` | `{state: AssistantState, board: Board, entries: [JournalEntry], channels: [Channel]}`：最近 `count`（默认 100）条日志；之后收 `assistant.*` 事件 |
| `assistant.unsubscribe` | `{}` | |
| `assistant.history` | `{before?, limit?, project?}` | `{entries, hasOlder}`：`before`（ISO 时间）之前的 `limit` 条（默认 100）；`project` 只要涉及这个项目的 |
| `assistant.say` | `{text, source?, attachments?}` | `{entry}`：用户说话（`source`：desktop / voice / 渠道 id，默认 desktop；`attachments`：这台上的文件路径，客户端先 `file.upload` 传过来，主机拷进助手目录、记在 `entry.data.attachments`）。助手在说时打断它重来；关着时只记日志。以 `#短号` 开头（「#3f2a 再跑一遍」）且对得上状态板上的会话时，后面的话原样发给那个会话、不经过大脑（`entry.data.direct = {machineID, conversationID}`），结果记成一条 notice |
| `assistant.stop` | `{}` | 打断正在说的 |
| `assistant.suggestion` | `{id, accept, source?}` | 2.5 起：助手提议的下一步（回复的 `data.suggestion`）——`accept` 真把那句交给会话（`source` 记谁点的，默认 desktop），假 = 不用。已经交过、说过不用、找不到时回错误 `3` |
| `assistant.voice.confirm` | `{id}` | 2.6 起：通话里被声纹拦下的那条说明（`data.voiceprintVector`）上点了「是我说的」——那句的声纹记成临时的（14 天，ASSISTANT.md §16.3），说明的 `data` 去掉声纹、标 `voiceConfirmed`（发 entry 事件）。找不到、没声纹、主机上没登记声纹时回错误 `3` |
| `assistant.presence` | `{conversationID?}` | 客户端前台正在看这台机器上的哪段会话（空 = 没在看）：那段会话的事件只上状态板 |
| `assistant.settings.update` | AssistantConfig 里要改的键 | `{config}` |
| `assistant.memory.list` / `.add` / `.update` / `.remove` | `{}` / `{text, kind?, pinned?}` / `{id, text?, kind?, pinned?}` / `{id}` | `{items}` / `{item}` / / |
| `assistant.resolveApproval` | `{machineID, eventID, approved}` | 回执上的「批准 / 拒绝」：转给那台机器 |
| `assistant.clear` | `{}` | 清空日志、记忆、话题摘要 |
| `assistant.channels.list` | `{}` | `{channels: [Channel]}`：这台主机上的聊天工具渠道（docs/ASSISTANT.md §7） |
| `assistant.channels.add` | `{kind, options, token?, owners?}` | `{id}`：加一个（现在只有 `kind: schat`；`options.server`、`options.pin`）。`token` 存进这台的密钥库，配置里只记密钥名；`owners: [{senderID, chatKey, name}]` 预先绑好的人（一键接入），连上后跟他们打招呼 |
| `assistant.channels.update` | `{id, enabled?, notify?, groupCommands?, name?, options?, token?, clearPin?}` | `options` 的键合并；换了 `token` 就重连；`clearPin` 清掉记下的证书指纹 |
| `assistant.channels.remove` | `{id}` | 去掉渠道和它存的 token |
| `assistant.channels.removeOwner` | `{id, senderID}` | 解除一个人的绑定 |
| `assistant.channels.removeGroup` | `{id, chatKey}` | 不再推到这个群 |
| `assistant.channels.bindCode` | `{id}` | `{code}`：6 位绑定码（30 分钟、用一次；在聊天工具里给机器人发 `/bind 绑定码`） |
| `assistant.export.list` | `{}` | `{files: [{path, size}]}`：换主机（搬家）时要搬的全部文件，`path` 相对助手目录（`journal/2026-10-07.jsonl`、`memory.json`、`settings.json`…；不含 `host.json`；配了带 token 的聊天工具时还有只在内存里组装的 `channel-secrets.json`）。服务端这时拍下整份，之后的 read 从这份里给 |
| `assistant.export.read` | `{path, offset}` | `{data, eof}`：从 `offset` 起最多 1 MB（base64）。2.3.21 及更早的服务端按磁盘读，读不到 `channel-secrets.json`（「没有这个文件」，配了 SChat 的主机换不走）：客户端遇到它读不出就跳过，搬好后提示到新主机上重新填聊天工具的 token |
| `assistant.import.begin` | `{}` | 开始接收搬来的文件（这条连接上的，之前没 finish 的作废） |
| `assistant.import.write` | `{path, offset, data}` | 追加一块（`offset` 要等于这个文件已收到的长度；`data` base64，最多 1 MB） |
| `assistant.import.finish` | `{}` | `{backup?}`：换上收到的全部文件——这边原有的先挪进 `<数据目录>/Assistant-backups/<时间>/`（`backup` 是它的路径），重读日志、记忆、话题摘要、设置（开关保持这边的；这边没有的模型改回跟新对话一样），之后推 `assistant.reset` |

- **AssistantState**：`{enabled, reacting, reactingEntryID?, config: AssistantConfig, brainEngineTitle?, engines: [{id, title, model, supportsTools}]}`。
- **AssistantConfig**：`{enabled, brainEngineID?, writerEngineID?, brainThinking, toolRounds, autoWakesPerHour, digestSeconds, pushRule: important|all|none, briefWriter,
  confirmDestructive, approveFromChannels, brainApproves, fullFromChannels, autoNextStep, idleMinutes, name}`（ASSISTANT.md §9；`autoNextStep` 2.5 起：提议的下一步不等点头直接做，默认关）。
- **JournalEntry**：`{id, at, kind: user|assistant|receipt|tool|notice, source?, text, projects?, refs?, interrupted?, data?, streaming?}`；
  回执的 `data`：`{machineID, conversationID, level, status, event, brief?, approvalEventID?, woke?}`；工具的：`{name, arguments, result, reply}`；
  助手回复的：`{silent?, error?, details?, suggestion?}`（silent 的不显示；`suggestion` 2.5 起：`{id, label, text, session, machineID, conversationID, state: pending|accepted|dismissed|auto}`，界面上「好 / 不用」）。
- **Board**：`{machines: [{id, name, online, idle, entries: [{machineID, conversationID, shortID, title, directory, workingDirectory, status, statusText,
  line, progress, awaited, dispatched, watched, approvals}]}]}`（`directory` 是目录名，`workingDirectory` 是完整路径；项目键 = `machineID:workingDirectory`）。
  `machineID` 是主机自己的说法：`local` 是主机本身，其余是主机机器列表里的 id。
- **Channel**：`{id, kind, title, enabled, notify, groupCommands, status: off|connecting|online|error, statusText, botName, server, hasToken, pin,
  owners: [{senderID, name}], groups: [{chatKey, name}]}`
  （不含 token）。
- 没有助手的服务端（2.3 之前）回 `-32601`。`assistant.export.*` / `assistant.import.*` 同 `server.pair`，只认本地 socket 进来的连接（SSH / WSL），WebSocket 的回 `4`；
  路径带 `..`、绝对路径的回 `-32602`。

**密钥、引擎**

| 方法 | params |
|---|---|
| `secrets.set` | `{name, value}`：值写进远程的密钥库（Agent 用 `request_secret` 申请时，客户端先 `secrets.set` 再 `runner.resolveApproval`；同一条连接上按顺序处理）。已知服务的密钥会自动接入它的大模型 |
| `secrets.remove` | `{name}` |
| `engines.update` | `{engine:EngineConfig}`：新增或修改一个在线引擎 |
| `engines.remove` | `{id}` |
| `engines.setSubAgentAllowed` | `{id, allowed}`：给不给子 Agent 用这个大脑（2.3 起；改的是这台机器的设置，之后照常发 `engines` 事件。客户端的开关先在本地改，请求失败了退回 `engines` 里的。设置页按服务开关时一家的几个模型各发一条） |
| `services.setEnabled` | `{id, enabled}`：开 / 关这台机器上接入了的一家在线服务——它的全部模型（`disabled`）和能力提供方一起（2.3 起，`server.features` 有 `services`）；没接入回错误 `3`。之后照常发 `engines` 事件（`disabledServices`）。客户端的开关先在本地改，失败了退回 |

**Agent 设置、MCP**（同步本机配置用，§11；2.1.5 起）

| 方法 | params | result |
|---|---|---|
| `settings.get` | `{}` | SettingsState |
| `settings.update` | SettingsState 里任意几项：只改给了的（`defaultEngineID` 空 = 跟随第一个引擎；`userStops` 是 `{key: 停不停}`，只改给了的几件事）。改了 `defaultEngineID` 服务端照常发 `engines` 事件；不认识的权限模式、工作方式回错误 `-32602` | 2.4 起：改完的 SettingsState |

- **SettingsState**：`{systemPrompt, enableThinking, requireApproval, confirmPaidCalls, defaultPermissionMode, defaultEngineID}`；2.4 起还有
  `defaultSystemPrompt`（只读）、`workingDirectory`（会话没指定时用的工作目录）、`teamMode`（主智能体的工作方式：`delegatesHeavy` 简单的自己做、
  其余派子智能体（2.5 起，默认）/ `delegatesOnly` 只派活和验收 / `delegatesWhenNeeded` 自己做、需要时派 / `solo` 不派子智能体；2.4 的服务端不认
  `delegatesHeavy`；2.9 起多一个 `ultra` 极限性能（同 `delegatesHeavy`，另外思考开到最高、子智能体能再往下派），2.9 之前的服务端回错误 `-32602`；
  2.9 起任务模式每段对话各自记（`runner.setTeamMode`），这一项只是**新对话的默认**，新装的是 `solo`；派发工具关掉（`tools.setEnabled delegate_task`）是总开关）、`userStops`（这些事停下来交给用户：`[{key, title, stops}]`）。2.5 起还有 `lightEngineID`（小活用的模型：空 = 自动、
  `follow` = 跟对话用的一样、其余是引擎 id）、`quotaFallback`（主智能体额度用完时：`switch` 换一个能用的接着做，默认 / `wait` 等恢复）；
  2.4 的服务端没有这两项，客户端按默认显示。不认识的 `quotaFallback` 回错误 `-32602`。2.8 起还有 `subAgentEngineID`（子智能体用的模型：
  空 = 跟对话用的一样，默认 / `auto` = 按活自动挑 / 其余是引擎 id，ASSISTANT.md「子智能体用的模型」）；2.8 之前的服务端没有这一项，它照旧按活挑，
  客户端显示成「按活自动挑」。
  2.6 起 `settings.update` 还认只写的 `voiceOnlyEnrolled`（只听你的声音）、`voiceprintStrictness`（声纹严格程度 0–2）、`voicePatience`（说完一句等多久 0–2：
  很快交 / 标准 / 边想边说）：这台的助手接的通话照这个按声纹过滤（ASSISTANT.md §16.2）、等你说完再交；
  不在 SettingsState 里，旧服务端忽略。
| `mcp.list` | `{}` | `{servers:{名字:配置}}`：mcp.json 里的全部 server（配置同 mcp.json 的一项） |
| `mcp.update` | `{servers:{名字:配置}}`：按名字加上或覆盖，没提到的不动；改了的重新连。有没存上的回错误 `-32602`，说明哪几个。2.4 起会马上起命令的这个同终端：经 WebSocket 配对进来的设备回错误 `4`（客户端的同步预览里 MCP 一项写明经 SSH 连才能同步） | |

旧服务端回「没有这个方法」（错误 `-32601`），客户端说明「远程的 friday-server 太旧」，自动升级后再同步。

**设置页的配置**（2.4 起，`server.features` 有 `config`；用户 2026-10-09「设置页面里的每个页面是不是根据我选择的服务器设置该服务器的配置」）：
客户端的 设置 › 智能体、设置 › 工具（工具、MCP 服务、能力提供方）看着这台时看、改的就是这一份——设置开着时订阅，关掉退订。改动经同一套本机实现
（和这台自己的设置页同一份逻辑），之后照常推 `config` 事件。客户端的开关先在本地改（界面马上有反应），之后以推回来的为准；旧服务端没有这一组方法，
客户端的页面说明「这台机器上的 Friday 太旧」，整页只读。工具卡片在没订阅时按客户端本机的工具显示（内置工具两边一样）。

| 方法 | params | result |
|---|---|---|
| `config.subscribe` | `{}` | `{settings: SettingsState, tools: ToolsState, mcp: McpState, capabilities: CapabilitiesState}`；之后这个客户端收 `config` 事件，服务端顺带按这台的密钥重查一遍提供方能不能用 |
| `config.unsubscribe` | `{}` | |
| `tools.setEnabled` | `{name, enabled}` | |
| `tools.scriptVersions` | `{name}` | `{versions: [ScriptToolVersion]}`（清单 + 源码，新的在前） |
| `tools.saveScript` | `{manifest, code}`：存成新版本并自测。`manifest` 的键同 ScriptTool，没给的按这台已有的（`parametersText` / `testArgumentsText` 是 JSON 文本） | `{passed, summary}`（自测完才回，最多等 10 分钟） |
| `tools.testScript` | `{name, arguments}`（参数的 JSON 文本） | `{passed, summary}` |
| `tools.rollback` | `{name, version}` | `{passed, summary}` |
| `tools.createScript` | `{fields: {name, title, description, permission, language}}`：照新建表单建一个（模板源码按这台的系统给） | `{error}`（空 = 建好了） |
| `tools.deleteScript` | `{name}` | |
| `mcp.setDisabled` / `mcp.setAutoApprove` | `{name, disabled}` / `{name, value}` | |
| `mcp.remove` / `mcp.reconnect` | `{name}` | |
| `mcp.reloadAll` | `{}` | |
| `mcp.saveServer` | `{name, config, oldName?}`：表单存一个服务（`oldName` 是改名前的名字），存上后只重连它 | `{error, mcp}` |
| `mcp.saveConfigText` | `{text}`：整个 mcp.json | `{error, mcp}` |
| `mcp.importText` | `{text}`：粘贴来的配置（Claude Desktop / Cursor / VS Code 的格式都认） | `{ok, names, error?, mcp}` |
| `capabilities.setEnabled` | `{id, enabled}` | |
| `capabilities.setKindAllowed` | `{id, kind, allowed}` | |
| `capabilities.setOption` | `{id, key, value}`（空 = 恢复默认） | |
| `capabilities.setTitle` | `{id, title}` | |
| `capabilities.deleteJob` | `{id}` | |
| `capabilities.refreshAvailability` | `{}`：重查一遍，结果经 `config` 事件 | |
| `capabilities.retest` | `{id}`：脚本提供方重新自测 | `{error}`（自测完才回） |
| `capabilities.import` | `{name, data(base64)}`：客户端读好的 .fridayprovider / .zip（导入文件夹时客户端先打成 zip），最大 32 MB | `{ok, id, title, version, replaced, permissionTitle, permissionDetail, error?}` |
| `capabilities.export` | `{id}` | `{name, data(base64)}`：客户端写到自己那边 |
| `capabilities.deletePackage` | `{id}`：移到废纸篓 | `{error}` |

- 存 MCP 的三个请求（`mcp.saveServer` / `saveConfigText` / `importText`，v2.5）回复里的 `mcp` 是存完以后的 McpState：发请求的客户端收到回复就用它更新列表，不等 `config` 推送（订着的话服务端把它记成已推过）；旧服务端的回复里没有，照旧等推送。
- **ToolsState**：`{descriptors: [Tool], scriptTools: [ScriptTool], toolsDirectory}`；Tool 同设置页用的 `{name, title, description, symbol, permission,
  origin（builtin / meta / script / capability / agents / mcp:服务名）, enabled, alwaysConfirm, consumesQuota, problem, parametersText?}`
  （非脚本工具带参数 schema 的文本）；ScriptTool 是清单的各键 + `{code, directory, entry, hasLastTest, lastTest: {passed, output, date}}`。
- **McpState**：`{servers: [{name, config, summary, status（disabled / connecting / connected / failed）, error, tools, toolCount, log, autoApprove,
  disabled, isRemote}], configPath, homeDirectory, configText, missingSecrets: {服务名: [缺的密钥名]}}`。
- **CapabilitiesState**：`{entries: [Provider], packages, problems, jobs, availability: {"提供方 id|能力": {ready, needs}}, optionSpecs: {提供方 id: [选项]},
  packageFor: {提供方 id: 包}, providersDirectory}`；Provider 是 ProviderEntry 的键（`id, type, title, options, enabled, disabledKinds`）+ 这台算好的
  `{kinds, kindInfos: [{raw, title, symbol, toolName, detail, description, isBuiltin, allowed}], secretNames, active, isLocal, isScript, createdByAgent, symbol}`
  （新能力的标题等由这台给：定义在这台的提供方包里）。日期一律是 ISO 8601 字符串。
- 文件、目录都在这台上：客户端不显示「在访达 / 资源管理器里打开」，选目录用 `fs.list` 浏览这台的；导入提供方时客户端先读好自己那边的文件传过来。
- 会马上在这台上跑代码、不经过智能体确认的——`tools.saveScript` / `testScript` / `rollback` / `createScript`（都会自测）、`mcp.saveServer` /
  `saveConfigText` / `importText` 和同步用的 `mcp.update`（会起配置里的命令）、`capabilities.import` / `retest`——同终端，只给登录了这台机器的连接（SSH / WSL / 本机）：
  经 WebSocket 配对进来的设备回错误 `4`（看、开关、改选项照常可以）。

**在线服务**（2.4 起，`server.features` 有 `serviceOps`）：设置 › 服务与密钥 看着这台时加、改、删在线服务都在这台上做——它的能力提供方由这台自己建、
改、删，不碰客户端那台的（以前客户端在自己那边改这台的密钥和引擎副本，能力提供方却建在客户端本机）。旧服务端退回原来的做法，但不再动客户端本机的提供方。

| 方法 | params | result |
|---|---|---|
| `services.install` | `{id, secret, models, baseURL, title}`：同添加服务的表单（`id` 是目录服务的 id，重复添加的是 `id#n`；`baseURL` 空 = 目录里的默认地址；`title` 是重复添加时的显示名） | |
| `services.apply` | `{id, title, baseURL, secretName, models, secretValue}`：同服务的「设置」（`secretValue` 填了就换密钥；`models` 是勾着的模型，没有对话模型的服务不看） | `{error}` |
| `services.uninstall` | `{id}`：密钥、全部模型、能力提供方一起去掉；没接入回错误 `3` | |
| `services.installCustom` | `{title, baseURL, models, secretName, key, supportsVision}`：自定义 OpenAI 兼容服务 | |
| `services.fetchModels` | `{baseURL, secretName?, apiKey?}`：在这台上拉模型列表（`apiKey` 空就用这台密钥库里的 `secretName`；这台连得到的地址，比如局域网里的 Ollama） | `{models}`（出错回错误，最多等 10 分钟） |
| `services.usage` | `{baseURL, secretName, custom, observedOnly}`（2.6，`serviceUsage`）：用这台的密钥查这家服务的余额 / 套餐用量（ServiceUsage：DeepSeek、Kimi、Kimi Code、硅基流动、OpenRouter、智谱 / Z.ai 编程套餐，`custom` 的另外试 one-api 这类中转），连同这台对话时顺手记下的限流余量、额度用完（`service-usage.json`）；`observedOnly` 只要顺手记的 | `{supported, report?, error?, observed}`（`supported`：这个地址有查询接口；查不到时 `error` 写原因，不回错误） |

找不到这家服务回错误 `3`。之后照常发 `engines`、`secrets`、`config` 事件。密钥的值不出这台：经 WebSocket 配对进来的设备调 `services.fetchModels`
只给 `secretName` 时，这个地址得本来就用这个密钥（这台的模型、或目录里这家服务的默认地址），不然回错误 `4`（免得指个别的地址把密钥拿走）；
自己填了 `apiKey` 的照常。`services.usage` 一样：要查的话这个地址得本来就用这个密钥，不然回错误 `4`。

**本地能力**（2.3 起，`server.features` 有 `powers`）：这台机器上的本地能力（Friday.Powers，装在哪台就用哪台的显卡；那台的 Agent 听录音 / 说话、
它当助手主机时的通话都用它）。客户端的 设置 › 能力 看着远程时看、管的就是这一份：页面开着时订阅，关掉退订。语音模式（助手页的麦克风）在客户端
那台电脑上听，用的是客户端本机的，不走这里。

| 方法 | params | result |
|---|---|---|
| `powers.subscribe` | `{}` | PowersState；之后这个客户端收 `powers` 事件 |
| `powers.unsubscribe` | `{}` | |
| `powers.refresh` | `{}`：重新取 Friday.Powers 的索引（检查更新） | |
| `powers.install` | `{id, target?}`：在这台上下载安装（`target` 是平台包，不给用这台默认的）；装了的就是更新 / 换平台。进展经 `powers` 事件 | |
| `powers.cancel` / `powers.uninstall` | `{id}` | |
| `powers.setEnabled` | `{id, enabled}`：关掉的不再自动启动，能力层也不用它 | |
| `powers.setOption` | `{id, key, value}`（2.8）：改装好的这一版清单里的选项（如本地大模型的「同时处理的请求」，Friday.Powers 的 power-spec.md §1 `options`）；只认 `choices` 里的值，在跑的进程重新启动后生效 | |
| `powers.start` / `powers.stop` | `{id}` | |

- **PowersState**：`{powers: [Power], refreshing, indexError, platformTargets: [平台包 id], available, unavailableReason?}`；
  Power 同设置页用的：`{id, title, detail, latestVersion, installedVersion, target, targetTitle, targets: [{id, title, detail, size}], status, statusText,
  progress, error, enabled, updateAvailable, capabilities, downloadSize, downloadSizeText, options}`（`options`，2.8：`[{key, title, detail, value, choices: [{value, title}]}]`）（`status`：notInstalled / downloading / installing / installed / starting / running / failed）。
- `available` 为假时（2.7 及以前 Mac 上 Friday.app 自带的助手进程只读：装、开关归 Swift App）改东西的方法回错误 `4`，`unavailableReason` 写原因。
- 旧服务端没有这一组方法：客户端的页面说明「那台的 Friday 太旧」，按钮不能点。

**本地模型**（2.8 起，`server.features` 有 `models`；用户 2026-10-12「修复连接远程服务器无法加载 llm 的 bug 和选择模型在该服务器下载」）：
这台机器上的本地大模型（模型库里下的 GGUF，在这台的 friday-llm 里跑；生图 / 生视频模型在 friday-diffusion 里跑）。客户端看着远程时，
模型菜单的「本地」列的是这一份，选、删都在这台上做；模型库窗口（「显示更多模型…」）里选的在这台上下载、跑，卡片上的进度、对勾也是这台的。
以前客户端列的是自己那台的、在自己那台下，再把本地引擎的 id 设到这台的会话上——这台没有这个模型也没有这个引擎，加载不了。
客户端连上就订阅（胶囊上要显示这台在下的进度）。

| 方法 | params | result |
|---|---|---|
| `models.subscribe` | `{}` | ModelsState；之后这个客户端收 `models` 事件 |
| `models.unsubscribe` | `{}` | |
| `models.refresh` | `{}`：重新扫一遍这台的模型库、取模型家族（打开模型菜单、模型库时） | |
| `models.use` | `{id, kind?, source?, conversationID?}`：用起来——没下载的在这台上下载（要用的 friday-llm / friday-diffusion 没装的先装），下好了大模型给 `conversationID` 那段会话换上（建本地引擎，经 `engines` 事件下发；没给会话 = 设成新对话默认），生图 / 生视频设成这台本地生图 / 生视频用的模型。`kind`（text / multimodal / image / video）不给按这台的模型家族认；`source`（modelScope / huggingFace）不给用这台设置里的下载源。马上回，进展经 `models` 事件，失败了发 `modelsFailed` | |
| `models.download` | `{id, kind?, source?}`：只在这台上下载、不换上（模型库的「下载」；要用的 Power 不装），`kind`、`source` 同 `models.use`。马上回，进展经 `models` 事件，失败了发 `modelsFailed`；2.8 早期的服务端没有这个方法（回 -32601，客户端提示升级） | |
| `models.cancel` / `models.retry` / `models.dismiss` | `{id}`：取消下载（等着它的会话也不再换上）/ 失败的重下 / 不再列出失败的 | |
| `models.release` | `{conversationID}`：这段会话改选了别的模型，之前在等下完的那个下好了也不再给它换上 | |
| `models.delete` | `{id, source?}`：删模型文件，同模型菜单「删除模型文件」，强制删（friday-llm 还在就停掉再删，删不掉隔半秒重试，最多 15 秒）。删掉了才回；没有这个模型回错误 `3`，删不掉回错误（`message` 是原因） | `{}` |

- **ModelsState**：`{available, machine, chatAvailable, local: [LocalModelInfo], downloads: [Download], waiting: [{conversationID, id}]}`。
  `machine`：这台的 `{memory（字节）, gpus: [{name, memory（显存，字节）}]}`（独立显卡，显存从大到小；客户端模型库标题下显示，「适合内存」按它判断；
  2.8 早期的服务端没有，客户端退回只写「在『X』上下载和运行」、按本机内存判断）；
  `chatAvailable`：这台的本地大模型能用（friday-llm 装了且开着；不能用时模型菜单不列「本地」和「显示更多模型…」）；
  LocalModelInfo：`{id（组织/仓库:量化档）, source, name, quant, sizeBytes, modelType, hasVision, kind（text / multimodal / image / video / other）, modelFile, mmprojFile?, directory, modified}`
  （路径是这台上的）；Download：`{id, name, kind, running, progress（0…1）, completed, total（字节，还不知道为 0）, error}`，按开始下的先后；
  `waiting` 是在等下完的大模型的会话（胶囊上显示进度）。`available` 为假（这台的 Friday 不管本地模型）时改东西的方法回错误 `4`。
- 旧服务端没有这一组方法：客户端的模型菜单不列那台的本地模型，模型库的「使用」说明「那台的 Friday 太旧」。

**文件存放位置**（2.8 起，`server.features` 有 `storage`；用户 2026-10-12「服务器上无法修改通用里的文件存放位置」）：设置 › 通用 › 文件存放位置
看着远程时看、改的是这台的（Qt 版 StorageLocation：数据、缓存、本地能力整体换到别的文件夹，登记后重启时搬；模型跟着位置走，也可以单独选文件夹，
换的时候搬过去，见下面 `storage.setModels`）。
客户端选文件夹用 `fs.list` 浏览这台的目录；路径都用这台的分隔符。

| 方法 | params | result |
|---|---|---|
| `storage.get` | `{}` | StorageState |
| `storage.measure` | `{}`：数据、缓存、本地能力各占多少（在这台的工作线程里算，等 10 分钟） | `{data, cache, powers, total（字节）, text}` |
| `storage.move` | `{folder, usage?}`：登记「重启时搬到 folder」（空 = 搬回默认；文件夹不空就在里面建 Friday 子文件夹）。检查能写、不在现在的目录里、目标是空的、空间够（`usage` 是 `storage.measure` 的 total，不给现算）；不行回错误 `-32602`（`message` 是原因），环境变量指定了位置回 `4` | StorageState |
| `storage.cancel` | `{}`：取消待办的搬家 | StorageState |
| `storage.restart` | `{}`：现在重启这台的 Friday 服务端——先回复，起一个新服务（`server --daemon --takeover`：等这个放开数据目录最多 30 秒，拿到锁后先搬文件、再监听），这个退出；客户端的连接断一下后自动重连。服务是开着的桌面端时回错误 `4`（在那台上重启 Friday 时搬） | |
| `storage.setModels` | `{folder}`：模型文件夹（空 = 不选了，跟着位置走：<根>/Models，没换过位置时是魔搭 / HF 的默认缓存），把模型一起搬过去（2.8.7 起；用户 2026-10-12「模型文件下载放到了哪里，为什么改不了」→「跟着位置走 + 换时搬过去」）：正在下的先停、friday-llm / friday-diffusion 先停，搬完记下新文件夹、接着下（从断点续）；从默认缓存搬时只搬 Friday 的（有 GGUF 或没下完的 .part 的仓库、生图 / 生视频的配套文件仓库），别的程序的不动。马上回（`modelsMove.active` 为真），进度和结果经 `storage` 事件；搬不成什么都不改、原因在 `modelsMove.error`。新下的放 `<folder>/modelscope/models`、`<folder>/huggingface/hub`。建不了、写不进去回错误 `-32602`，正在搬回 `4`。2.8.6 的服务端是只改记录、不搬 | StorageState |
| `storage.adoptModels` | `{}`：把魔搭 / HF 默认缓存里 Friday 的模型搬到现在的模型文件夹（换位置以前下的；StorageState 的 `leftoverModels` 是还有多少）。同上，异步、经 `storage` 事件（2.8.7 起） | StorageState |

- **StorageState**：`{dataDir, cacheDir, powersDir, root（自定义的根，空 = 默认位置）, overridden（FRIDAY_DATA_DIR 等环境变量指定了，不让换）, pending?（有待办的搬家：要搬到的根，空串 = 搬回默认）,
  modelsDir（选的模型文件夹，空 = 没选）, modelsEffective（实际的模型文件夹：选了的，没选、换了位置时是 <根>/Models，空 = 默认缓存）, modelScopeDir, huggingFaceDir（新下的模型放哪）,
  modelsMove?（`{active, done, total（字节）, to, error}`：正在搬 / 上次没搬成）, leftoverModels?（`{count, bytes}`：默认缓存里还没搬过来的 Friday 模型）,
  canRestart（服务是开着的桌面端时为假）}`。`MODELSCOPE_CACHE`、`HF_HUB_CACHE`、`HF_HOME` 只换默认缓存的位置，不再锁住模型文件夹（2.8.6 有 `modelsOverridden`，2.8.7 去掉）。
- 换存放位置（`storage.move` + 重启）时，没选模型文件夹的话模型跟着搬：<旧根>/Models（旧根是默认位置时是默认缓存里 Friday 的模型）→ <新根>/Models；
  同一个盘改名，不同盘复制再删，没搬成的留在原处、记成选了的模型文件夹（照样认）。
- 常驻服务（`friday server --daemon`）起来时和桌面端一样：拿到旧数据目录的锁之后、建核心对象之前搬（日志等搬完再开），搬好换到新目录的锁、删掉旧的空文件夹；
  搬失败照旧用原来的位置，日志里写原因。

**终端**（2.2 起，TERMINAL.md；只给 SSH / WSL / 本机连进来的客户端）

| 方法 | params | result |
|---|---|---|
| `term.open` | `{id, conversationID, cols, rows, workingDirectory?, command?}`：用客户端给的 id 开一个终端（没给目录：这段会话的工作目录，再没有是设置里的默认目录；`command` 起好后打进去回车），开好后这个客户端就接着它 | |
| `term.attach` | `{id}`：接上一个终端（之后它的输出经 `term.output` 转来） | `{data(base64), cols, rows}`：最近的原始输出（最多 512 KB），客户端清屏后重放出屏幕 |
| `term.detach` | `{id}` | |
| `term.write` | `{id, data(base64)}`：用户打的字（已经按终端的规矩编码好） | |
| `term.resize` | `{id, cols, rows}`：伪终端跟着改（几个客户端看同一个时以最后改的为准） | |
| `term.close` | `{id}`：结束 shell 和它起的进程，从列表去掉 | |

找不到终端回错误 `3`。

**文件**

| 方法 | params | result |
|---|---|---|
| `directories.get` | `{}` | DirectoriesState（重新看一遍目录还在不在；2.1.13 起） |
| `directories.push` | `{path}`：选了一个工作目录，记进最近选择的（去重、提到最前、最多 10 条） | |
| `directories.remove` | `{path}`：从最近选择的里去掉 | |
| `fs.list` | `{path?, hidden?}` | `{path, parent?, entries:[{name, path}], roots:[{name, path}], truncated}`：某个目录下的子目录（选工作目录用；不给 path 是主目录，隐藏目录默认不列，最多 5000 个）。`roots` 是主目录和各个盘（Linux 是 `/`） |
| `file.upload` | `{uploadID, name, offset, data(base64), last}` | 最后一块回 `{path}`：服务端上的路径，用在 `attachments` 里。每块不超过 1 MB |
| `file.read` | `{media, offset, length}` | `{data(base64), size, eof}`。`media` 是消息里附件的 `relativePath`（媒体目录之内） |

客户端按附件的 `relativePath` 把媒体缓存在自己的媒体目录下同样的位置，第一次显示时用 `file.read` 取。

**服务**（`server.status` 以外的只认本地 socket 进来的连接，也就是 SSH / WSL；WebSocket 进来的设备回错误 `4`）

| 方法 | params | result |
|---|---|---|
| `server.status` | `{}` | 版本、主机名、用户、数据目录、客户端数… |
| `server.shutdown` | `{ifIdle?}`：保存后退出（正在生成的会被中断，客户端要先问用户）。`ifIdle` 为真时有对话在生成、有子 Agent 在跑就不退，回错误 `4`（自动升级用，§9）。本地 socket 上可以不先 hello：新版本的客户端协议版本不同时也能让旧服务退出。服务由桌面端提供（`host` 是 `"desktop"`）时一律回错误 `4`：关桌面端才行（§12） | |
| `server.websocket` | `{enabled?, port?, bind?, cert?, key?}`：开 / 关 WebSocket 入口（记在 `remote-websocket.json`），不给参数只查 | `{enabled, listening, bind, port, secure, fingerprint?, urls, error?}` |
| `server.pair` | `{name, host?}`：配对一台设备。`host` 是客户端连这台机器时用的地址，放在链接里最先 | `{deviceID, name, token, urls, secure, fingerprint?, link}`，令牌只给这一次 |
| `server.devices` | `{}` | `{devices:[{id, name, createdAt, lastSeen?}]}` |
| `server.revoke` | `{id}`：吊销，经它连着的马上断开 | |

## 7. 没有人在时

- 遇到要确认的调用（写文件、执行命令、删除类操作、申请密钥…）：**默认挂起**，等有客户端连上来确认；
  待确认的会话在 `runner.pendingApproval` 里，客户端在侧边栏标橙色「待确认」，连上时发 `attention`。
- 不想被挂起：把这段对话的权限模式切到「完全」。以后在 设置 › 智能体 里加「没人在时自动允许」的选项（默认关）。
- 等额度恢复、排队的消息、后台子 Agent 的结果交回主 Agent，都和本机一样照常进行。

## 8. 安全与 WebSocket 入口

- 本地 socket 只对同一个系统用户开放（`QLocalServer::UserAccessOption`）。能连上它的人本来就能以这个用户登录这台机器。
- Agent 以 SSH 登录的那个用户的身份执行命令：用 root 连接时，「完全」模式下的 Agent 能对整台机器做任何事。
- 密钥的值只从客户端流向服务端（`secrets.set`），服务端不会把值发给任何客户端。

**WebSocket 入口**（给手机等设备直接连，不经 SSH；默认关）：

- 打开：看着经 SSH / WSL 连的远程机器时，机器列表 › 配对手机或其他设备…，或者在远程机器上 `friday-server --websocket on [--port 7443] [--bind 0.0.0.0]`
  （装着 Friday 桌面端的机器上是 `friday server --websocket on …`；下面的 `--pair`、`--devices`、`--revoke` 同样）。
  设置记在 `<数据目录>/remote-websocket.json`，服务重启后照样开。
- **TLS**：给了 `--cert / --key`（PEM）就用；没给就用 openssl 在 `<数据目录>/tls/` 生成一张十年的自签名证书（Linux 是 EC P-256；
  Windows 上 Qt 的 TLS 后端是 schannel，只认 RSA、PKCS#1 格式的私钥，生成的是 RSA 2048）。客户端不认 CA，只认配对时拿到的
  **证书指纹**（SHA-256），对不上就拒绝（换了证书要重新配对）。没有 openssl、生成不了时退回不加密的 `ws://`，令牌照样要；
  这时只在可信的网络里用，或者放进 Tailscale / WireGuard。
- **配对**：`server.pair` 给每台设备生成一个随机令牌（32 字节），服务端只存它的 SHA-256（`remote-devices.json`，只有本用户可读写）。
  配对链接 `friday://pair?u=<地址>&t=<令牌>&f=<指纹>&n=<主机名>` 只显示这一次：粘贴到「添加远程机器…」（或 `friday --remote <链接>`），
  客户端把令牌和指纹存在本机的 `<数据目录>/Remotes/pairings.json`（只有本用户可读写），之后连 `wss://…` 就行；以后的手机 App 扫码。
  链接里的地址：从客户端配对时用 SSH 连的那个主机名 / IP；`--pair` 命令行配对时按网卡挑，局域网私有地址优先，其次 100.64/10
  （Tailscale），198.18/15（Clash、Surge 一类代理的 TUN 虚拟网卡）放最后，跳过 Docker / 虚拟机网桥。
- macOS 客户端连局域网地址要「本地网络」权限：第一次连时系统会问；拒绝过的话 URLSession 报「似乎已断开与互联网的连接」，
  客户端提示到「系统设置 › 隐私与安全性 › 本地网络」里打开 Friday（经 SSH 连不受影响）。
- 吊销：`friday-server --devices` 查 id，`friday-server --revoke <id>`；连着的那台马上断开，之后连不上（错误 `5`，不自动重连）。
- 配对、开关入口、吊销只能经 SSH / WSL 进来的连接做：WebSocket 进来的设备不能给别的设备开权限。
- **Windows 防火墙**默认拦住别的机器连进来：在那台 Windows 上用管理员身份运行一次
  `New-NetFirewallRule -DisplayName "Friday server" -Direction Inbound -Protocol TCP -LocalPort 7443 -Action Allow -Profile Private`
  （Friday 不替用户改防火墙）。Linux 上有 ufw 之类的也要放行这个端口。

## 9. 部署与生命周期

- **现在的状态**：Qt 客户端 + friday-server 已实现，在真实的 Ubuntu 24.04 服务器和 Windows 11 上验证过（SSH、WebSocket）。
  Swift 客户端（代码在 `Friday/Friday/Remote/`；切换、添加、同步同 Qt，§11）也做了，
  从 Mac 经 SSH、wss 连 Windows 11 验证过（会话列表、流式输出、子 Agent 的待确认在本机允许、浏览远程目录、配对）。
  自动安装 / 升级（下面）在 Windows 11（cmd）和 Ubuntu 24.04 上验证过：Qt 客户端装、升级、远程在生成时等它做完再换；
  Swift 客户端装、升级；两边都走过 latest.json → 下载 → 校验的路径。
- **friday-server 包**（CI 打，Friday.build 的 Release 里）：`Friday-<标签>-server-linux-x64.tar.gz`、`Friday-<标签>-server-windows-x64.tar.gz`，
  v2.8 起还有 `Friday-<标签>-server-macos-arm64.tar.gz`（别的机器经 SSH 连 Mac 时的桥，`bin/` 旁边是 `Frameworks/`、`PlugIns/`；ASSISTANT.md §2.2），
  顶层目录 `friday-server/`：`bin/friday`（没有图形界面的 friday，构建目标 friday-headless）和它的别名 `bin/friday-server`（v2.8 起；Linux / macOS
  是符号链接、Windows 是一份拷贝，客户端运行的仍是它；v2.6–v2.7 只有 `friday-server`，是 friday-cli 换个名字。Linux 另有 `lib/`、`plugins/`、`bin/qt.conf`，RUNPATH `$ORIGIN/../lib`，除 glibc、libstdc++、
  libgcc_s 外的共享库都在包里——QtGui 要的 GL、X11、fontconfig 服务器上多半没装；在 Ubuntu 22.04 上编，要 glibc ≥ 2.35；
  Windows 的 `bin\` 里是 friday.exe、friday-server.exe、Qt 运行库与插件、VC++ 运行库），`server.json` 是 `--version` 的输出加标签。
  latest.json 里挂在 `assets["<平台>-<架构>"].server` 下：`{name, url, sha256, size, version, protocol}`（打包见 qt/ci/package_*）。
- **远程的布局**：`~/.friday-server/versions/<版本>/` 是解开的包，`~/.friday-server/bin` 指向当前版本的 `bin/`（Linux 是符号链接，
  Windows 是目录联接）。客户端照旧运行 `bin/friday-server --attach`；常驻进程从 `versions/<版本>/` 里的真实路径起（`--attach` 拉起时
  解开链接），换版本只是把 `bin` 指到别处，在跑的不受影响。手动放在 `bin/` 里的老布局第一次自动升级时挪进 `versions/` 下。
- **自动安装**（SSH、WSL；WebSocket 连进来的、用 `FRIDAY_REMOTE_SERVER` 指定了别处的不装）：`--attach` 还没 hello 就断了、
  一句协议也没说过、也不是 ssh 自己连不上（退出码 255）时，客户端跑探测命令（`uname` / `PROCESSOR_ARCHITECTURE`、`bin/friday-server`
  在不在、`versions/` 下有哪些版本）。没装就从 latest.json 取客户端这个渠道的包（Beta 渠道在正式版和 Beta 里取高的）、下到本机缓存
  （`<数据目录>/Remotes/ServerPackages/`，核对大小和 SHA-256），经 ssh / wsl.exe 的标准输入流过去，远程用 tar 解到 `versions/<版本>/`
  （Windows 用系统自带的 tar.exe；cmd、PowerShell 当默认 shell 都跑同一段 `powershell -EncodedCommand` 脚本），跑一次 `--version`
  核对能运行（Linux 太旧、缺库时在这里报出来），再把 `bin` 指过去、删掉旧版本，然后照常连。失败了在横幅里说明，等用户点「重新连接」。
- **自动升级**：连上后服务端的版本比客户端旧（客户端是正式版时；开发版只在给了 `FRIDAY_SERVER_PACKAGES` 时以包为准），
  而且 latest.json 里的包协议版本和客户端一样，就在后台照上面装好新版本、切换 `bin`（不打断当前连接，横幅说「已经装好，远程空闲时自动换上」）；
  等远程空闲（没有对话在生成、没有子 Agent 在跑，停下来再等 10 秒）发 `server.shutdown {ifIdle: true}`，连接断开后马上重连，
  `--attach` 拉起的就是新版本。自己编的服务端（版本 0.0.0）不动。Windows 上 `bin` 还是普通目录、而且挪不动时，
  等旧服务退出后再切换。换过一次还是旧版本就不再试，横幅里说明。
- 环境变量：`FRIDAY_SERVER_PACKAGES=<目录>` 用那个目录里的包（文件名同 CI 的产物，取标签最新的；离线、测试）；
  `FRIDAY_SERVER_MANIFEST=<地址>` 改读那里的 latest.json（测试、内网镜像）。
- 手动装：解开 friday-server 包，把 `friday-server/` 放成 `~/.friday-server/versions/<版本>/`，再把 `~/.friday-server/bin` 指过去
  （或者直接放成 `~/.friday-server/bin/` 这种老布局）；自己编的话，Linux 上用 Qt 6.11.3（qtbase + qtwebsockets）
  `cmake -S qt -B build -G Ninja -DFRIDAY_BUILD_GUI=OFF -DFRIDAY_BUILD_TESTS=OFF`、`ninja -C build friday-headless`，
  把 `build/src/cli/friday-headless` 拷成 `~/.friday-server/bin/friday-server`（或者拷成 `friday`，用 `friday server …`）。
- `friday-server --attach`：桥（§2），没在跑就拉起常驻进程；`--daemon`：前台运行服务（给 systemd 用）；`--stop`：保存并退出（服务由开着的桌面端提供时不退，打印原因）；
  `--status`：打印运行状态；`--version`：打印 `{"version":…,"protocol":…}`。
- 常驻进程的日志：`<数据目录>/Logs/friday-server.log`。
- 有的发行版（logind 的 `KillUserProcesses=yes`）登出时会杀掉用户的所有进程。要确保一直在，装成 systemd 用户服务：

  ```ini
  # ~/.config/systemd/user/friday-server.service
  [Unit]
  Description=Friday server
  [Service]
  ExecStart=%h/.friday-server/bin/friday-server --daemon
  Restart=always
  [Install]
  WantedBy=default.target
  ```

  `systemctl --user enable --now friday-server`，再 `loginctl enable-linger $USER`（没登录时也运行）。`Restart=always`：自动升级时
  旧服务退出后 systemd 马上用 `bin` 指向的新版本拉起来（要停就 `systemctl --user stop`）。
- **WSL**：WSL 在本机上，电脑关机它也停，只能做到「关掉 Friday 窗口任务照常跑」。另外最后一个 wsl.exe 会话结束后，
  WSL2 可能会停掉发行版（看 WSL 版本和 `.wslconfig`），需要保活时用登录时启动的计划任务挂一个后台的 wsl.exe。

## 10. 以后的手机 App

手机上跑不了 Agent（没有 shell、不能长时间后台运行），所以手机 App 就是本协议的又一个客户端，结构和桌面端连远程完全一样。
v1 为它预留的：

- 协议与传输无关（§2）：WebSocket 上的消息格式和 SSH 上的完全一样。
- 客户端不碰服务端的文件系统，附件上传、媒体读取都走协议（§6）。
- 运行状态（Runtime、RunnerState）由服务端算好，客户端只显示，手机端不用重写业务规则。
- 断线重连只靠 `hello` + 重新订阅，适合手机网络频繁切换、App 被挂起。

到时候要补的：

- **WebSocket 入口与配对已经做了**（§8）：手机 App 连 `wss://…`、扫配对链接的二维码、按指纹认证书即可。还要补：二维码（桌面端现在只显示链接）。
- **网络可达**：手机不在同一个局域网时连不到服务器。先用 Tailscale / WireGuard 这类组网直接连；
  以后可选一个中转服务（服务端主动连出去，手机连中转），省去组网。
- **推送**：App 在后台时 socket 会断，待确认、任务完成要靠 APNs / FCM 推送，这需要一个发推送的服务端（可以和中转放在一起）。
- **会话太长**：给 `conversation.subscribe` 加分页（先给当前路径最后 N 条，往上翻再取），手机端省流量和内存。
- **实现**：iOS 用 SwiftUI，和 macOS 的 Swift 客户端共用协议与副本模型的代码；Android 可以复用 Qt 客户端（Qt for Android），
  界面按手机重新排版。

## 11. 客户端：切换机器、添加远程、同步配置

**切换**：主窗口侧边栏最底下一行，左边是现在看的机器（本机 / 远程的名字，远程带连接状态的小圆点：绿 = 连着、橙 = 没连上），
右边是设置按钮。点机器名弹出列表：本机、每台远程（副标题是连接状态，有待确认调用的带橙点）、「添加远程机器…」；看着远程时还有
「同步本机配置到这台…」「配对手机或其他设备…」「断开」。每台远程右边一个「×」：从列表移除。

- **直接切换，不重启**：同一个窗口、同一个进程。本机的 FridayCore 一直在跑（切到远程时本机的生成、子 Agent 照常进行，
  托盘 / 菜单栏图标始终是本机的）；连过的远程在后台一直连着（RemoteSession 不删），切回去是瞬间的。
  切换时换掉界面用的「当前机器」对象：会话库、AgentRunner、Agent 团队、密钥库、引擎库（远程的是副本：会话库副本、
  引擎库副本、密钥库副本只有名字），设置里的引擎、密钥页面跟着看当前机器的。
  - Qt：QML 单例都用 `qmlRegisterSingletonType` 的回调注册（`qmlRegisterSingletonInstance` 的实例只能给一个引擎），
    切换时新建一个 QQmlApplicationEngine 加载完、接过旧窗口的位置大小和状态再显示，旧窗口隐藏后删掉，没有闪烁。
  - Swift：主窗口的环境对象换成当前机器的那一套，内容视图按机器 id 重建。
- 别的机器上有要确认的调用：机器名旁边亮橙点、发系统通知；托盘 / 菜单栏照常提醒本机的。
- 上次看的机器记在本机数据目录的 `Remotes/remotes.json`，下次启动回到它：

  ```json
  {"remotes": [{"id": "<UUID>", "target": "ssh:user@host", "name": "可选的显示名",
                "identityFile": "可选的私钥文件", "addedAt": "<ISO 8601>"}],
   "current": "local 或某个远程的 id"}
  ```

  Qt 和 Swift 同一个格式。第一次读时把 v2.1 早期的「最近连过的」（`Remotes/recent.json`）并进来。
- 命令行（Qt）`friday --remote ssh:user@host`：没开着就启动并切到它（不在列表里就加上）；已经开着就把地址交给开着的那个
  （InstanceLock 的激活消息 `activate <百分号编码的参数>`），由它切过去，自己退出。
- 「断开」：先切回本机，再断开连接（远程的 friday-server 照常运行）；「×」（从列表移除）断开并从 remotes.json 删掉，不用确认（远程机器上的 friday-server 和会话都还在，以后可以再添加），菜单不收起。

**添加远程机器**：地址（`user@host[:端口]`、`wsl:发行版`、配对链接 `friday://pair…` / `wss://…`）、可选的显示名。
SSH 再选登录方式：

| 方式 | 做法 |
|---|---|
| 密钥（默认） | ssh-agent 或 `~/.ssh` 里的默认密钥，要已经能免密登录 |
| 指定私钥文件 | 存进 `identityFile`，连接时加 `-i <文件> -o IdentitiesOnly=yes`。有口令的私钥要先 `ssh-add` 加进 ssh-agent（BatchMode 不问口令） |
| 密码 | **只用一次、不保存**：本机没有 `~/.ssh/friday_ed25519` 就生成（`ssh-keygen -t ed25519 -N ""`）；用密码登录一次（`SSH_ASKPASS` 指向一个临时脚本，密码经环境变量交给它，`SSH_ASKPASS_REQUIRE=force`，只试一次密码），探测远程的 shell，把公钥追加进远程的 `~/.ssh/authorized_keys`（Windows 上管理员组的账号写 `%ProgramData%\ssh\administrators_authorized_keys` 并用 icacls 收紧权限，同 OpenSSH 的要求）；再用这把密钥 BatchMode 登录验证，成功后这台机器按「指定私钥文件」保存 |

用密码时「添加并连接」后对话框留着（Qt 的 Alert 用 `closesOnConfirm: false`：确认按钮换成 ActionRole，点了 Dialog 不关），下面显示进展
（正在生成密钥 / 正在用密码登录 / 正在装公钥 / 正在用密钥登录），装好了才关、切到那台机器；失败时对话框照样留着，写出原因，改了再点一次。
常见的连不上（对方拒绝连接、超时、找不到主机、主机密钥变了、网络不通）换成给人看的说明（`RemoteTarget::sshFailureReason`，
平时连接失败的横幅也用它），密码不对是「密码不对，或者这台机器不允许用密码登录」后面带上 ssh 的原话。

**同步本机配置**：把本机的配置写到当前看的远程机器，只往远程写，不改本机。勾选要同步的部分（默认全勾）：

- **在线服务**：本机的在线引擎（`engines.update`，按 id 加上或覆盖）和它们引用的密钥。2.3 起按服务挑：本机接入的每家服务一个勾（默认全勾），
  只同步勾了的那几家；只有能力提供方的服务（火山语音等）带它的密钥过去，远程收到已知服务的密钥会自己接入。
- **Agent 设置**：系统提示词、深度思考、调用前确认、付费调用确认、新会话默认权限、默认大脑（`settings.update`，只写不一样的；
  默认大脑只在远程有这个引擎或这次一起同步时才写；本机的系统提示词还是默认的不写——各端的默认提示词写着自己的系统，远程用它自己的默认）。
- **MCP 服务器**：mcp.json 的每一项（`mcp.update`，按名字加上或覆盖）和它们引用的密钥（`${secret:名字}`）。命令和路径要远程也有才能用。
  经配对链接（WebSocket）连着时不同步这一项（2.4 起：会在那台上直接起命令，同终端只给 SSH / WSL）。

先读远程的现状（`engines` 事件里的引擎、`settings.get`、`mcp.list`、密钥名字），列出每一项是「新增 / 覆盖 / 一样」，
确认后按 密钥 → 引擎 → 设置 → MCP 的顺序写过去。远程只告诉客户端密钥的名字、不给值，所以远程已有的同名密钥总是显示「覆盖」。
密钥会存到远程机器的密钥库：能登录那个账号的人都能读到，对话框里写明。

## 12. 桌面端兼当服务端（Qt 版）

一台机器上开着 Friday 桌面端（Windows / Linux 的 Qt 版），别的电脑又经 SSH / WebSocket 连这台机器：两边用的是同一个数据目录，
不能各开一个 FridayCore（会互相覆盖会话记录）。做法是**谁拿着数据目录，谁就提供服务**：

| 这台机器上 | 远程连上来的是 | `server.host` |
|---|---|---|
| 开着桌面端 | 桌面端自己（`DesktopService`：桌面端的 FridayCore 上开一个 RemoteServer，同一个本地 socket 名、同一个 WebSocket 入口） | `desktop` |
| 没开桌面端 | 后台的 friday-server（`--attach` 拉起） | `server` |

桌面端和远程客户端实时看到同一批会话、同一组正在跑的任务；要确认的调用哪边都能点（桌面端照常弹通知、托盘提醒）。

**桌面端退出**（菜单的「退出 Friday」、没有托盘时关掉窗口；有托盘时关掉窗口程序还在托盘里，照常服务）：

- 还有对话在生成、有子 Agent 在跑：窗口和托盘先收起来，在后台跑完再退（期间再打开 Friday 就回到窗口，不退了）。
- 退出就全停：连着的客户端断开，WebSocket 入口、助手跟着停，不留后台进程（v2.8 起，用户定 2026-10-11「根本不需要启动后台服务器」；
  v2.1–v2.7 退的时候还有人连着会起后台的 `friday-server --daemon` 接着服务，v2.7 还要打开设置 › 通用的「关闭 Friday 后在后台继续运行」，这个设置已去掉）。
- 自动更新换包重启：新版本马上起来，客户端连回新的桌面端。

**桌面端启动时数据目录被后台的 friday-server 拿着**（远程连过这台机器后它留下来接着跑）：先在本地 socket 上发
`server.shutdown {ifIdle: true}`（不用 hello，§6）——

- **它空闲**：它保存退出，桌面端拿到锁照常启动，接过服务（远程客户端自动重连到桌面端）。
- **它在跑任务、协议版本一样**：不让用户等——桌面端马上打开，「本机」这一项先连着它用（`RemoteTarget::Kind::Local`，经本地 socket，
  显示名「本机（后台服务）」）：会话、正在跑的任务、待确认的调用都在，能直接发消息。这段时间（`StandbyTakeover`）：
  - 桌面端自己的 FridayCore 用临时数据目录（复制一份 settings.ini，界面的样子不变；不碰真正的数据目录），不检查更新、不连 MCP；
    远程机器列表、配对信息仍读写真正的数据目录（`FRIDAY_LOCAL_DATA_DIR`）。
  - 顶部横幅说明「本机的后台服务还在跑任务，桌面端先连着它用；做完后自动切回桌面端」，机器列表里「本机」的副标题、托盘提示同样说明；
    设置暂时打不开（打开时提示原因），「同步本机配置」不能用。
  - 每秒看一次：它连续两秒空闲，就再请它 `server.shutdown {ifIdle: true}`（先断开自己的连接）；它答应退出，桌面端起一个新的自己
    `friday --takeover --geometry=x,y,宽,高[,max] --select=<会话 id>`（等它放开数据目录、窗口放回原来的位置大小、选中原来的会话），
    新窗口开始接「把窗口拿到前台」的请求后旧进程退出——看上去是窗口刷新了一下。连不上它了（有人 `friday-server --stop`）也这样试。
- **它在跑任务、协议版本不同**（桌面端连不上它）：弹框问「等它做完」（进度框，做完后自动打开 Friday）/「现在停掉」（正在生成的会中断）/「取消」。

服务由桌面端提供时，`server.shutdown`（包括 `friday server --stop`、客户端的自动升级）一律回错误 `4`，说明退出桌面端即可；
桌面端自己会自动更新，客户端不对它装、换 friday-server。

macOS 的 Swift 桌面端没有这一套（它的数据目录也和 Qt 版不同），只做客户端；2.3 起另外兼当**受管端**：Mac 上的 friday-server
（v2.8 起是装在 `~/.friday-server/` 的 macOS 服务端包，只做桥、不起服务；2.3–2.7 是 Friday.app 自带的 `Contents/Helpers/`）的 `--attach` 桥到
Swift App 的本机 socket，别的机器上的助手经 SSH 管 Mac 本机的会话、v2.8 起也能看这台 Mac 上的助手（`assistant.*`）（`server.host` 是 `"swift"`，
ASSISTANT.md §2.2、§11.3）。客户端自动安装时认出对方是 macOS：
App 里带 helper（2.7 以前）就把 `~/.friday-server/bin/friday-server` 链接到它，没有就同 Linux 装 `macos-arm64` 的包（§9）。
