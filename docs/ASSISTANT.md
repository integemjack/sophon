# 助手（v2.3，设计稿）

用户要求（2026-10-07）：在 Friday 上加一个「助手」。它能新建会话、跟指定的会话说话、列出会话、添加远程服务器、同步配置、管理各台机器上的会话；
**所有会话一有结果就用简短的内容推给助手**（像 WebSocket 那样推，助手不去等、不去拉）；助手要**实时**跟用户对话、听懂意思，以后还要加
**语音输入输出**；能绑定市面上的聊天工具（包括 SChat 的机器人），用户在手机上就能跟它说话、收它的汇报。
关键约束：**同时管很多项目、互不影响**；用户跟它的对话不是一轮，上下文也不清空，会一直进行下去——**不能因此无限变长**。

状态：**P0 在做**（2026-10-07 起，v2.3 分支）。已实现的见 §13 的「进度」；协议的改动写进了 REMOTE.md（协议以那里为准），这里已实现的部分写的是实现后的样子。
v2.5（2026-10-09 起）的上下文、核对、预测下一步、习惯学习见 §15。
Qt（Windows / Linux）和 Swift（macOS）界面一致。助手的内核 Windows / Linux 上是 C++ core 的（在 Qt 桌面端或 friday-server 里），
macOS 上 v2.8 起是 Swift 版自己的（Friday.app 里，同一套规则、提示词、JSON，§2.2）；friday-server 只是让别的电脑连进来的能力，助手不靠它。

---

## 0. 要点

- **助手不是一个会话，而是一个常驻的实时内核**（`AssistantRuntime`）。对话、事件、干活三条线分开，谁也不等谁（§3）。
- **会话主动推**：每个会话有动静就推一条轻量的 `activity` 事件（开始、步骤推进、要确认、要问用户、出错、一轮结束…），不推整条消息（§4）。
- **事件先过代码**：分诊和状态板是纯代码、不调模型；只有要紧的事才叫醒大脑（§5）。
- **回执不每条过大脑**：状态和数字永远由代码写；「说了什么」那一句按规则挑，可选用小模型写（默认关）；
  要说人话时**批量过一次大脑**——用户在等的、要紧的、同一项目扎堆的、用户隔了一阵回来时（§4.2、§5.1）。
- **用户说话永远优先**：大脑同一时间只做一次短反应，用户一开口就取消重来；工具全是异步的，派活立即返回（§3）。
- **对话无限、上下文固定**：每次调用大脑的输入**按当前状态现拼**（固定约 14k token），不在历史后面接着加；项目的细节留在各自的会话里，
  助手只拿指针和一行状态（§6）。
- **主机默认本机、可改**；所有渠道、桌面端、语音共用**一个助手对话**；大脑和写回的模型可选，默认跟新对话一样（§2、§3.3、§7.5）。
- **Mac 上的助手在 Friday.app 里**：v2.8 起内核用 Swift 重写（同 C++ 的规则），本机的会话进程内直接管；别的电脑经受管端看它（§2.2）。
- **助手不进 `conversations.json`**：单独的 `Assistant/` 目录，日志按天追加、按页读（§10）。
- **渠道层**接聊天工具，先接不要公网地址的（SChat、Telegram、微信 ClawBot、企业微信、飞书、钉钉…），其余靠通用入口和脚本渠道（§7）。
- **语音**放在 P2，但「以一句话为单位收发、能取消、说的和给看的分开」从 P0 就做进内核（§8）。
- **默认保守、都做成设置**：删除、加机器、同步要确认；在聊天工具里批准工具调用、助手自己批准，默认都关（§9）。

---

## 1. 用户看到的

### 1.1 桌面端

```
┌──────────────┬──────────────────────────────────────────┬───────────────────────┐
│ 搜索对话…     │ 助手                 ● 微信 ● SChat ○ 飞书  │ 状态板                 │
│▌助手        3 │ ─ 10月7日 ─────────────────────────────── │ mac-mini               │
│  微信、SChat  │                   来自 微信 · 10:02        │  跑单测 #3f2a  3/5 ⟳   │
│ ~/Friday      │        让 mac-mini 跑一遍单测，跑完告诉我 │ 本机                   │
│  清理构建   ● │ 好，在 mac-mini 的 ~/Friday 开了会话去跑。 │  清理构建 #9b10 要确认 │
│  终端停靠   ⟳ │ ┌ 新建会话 · mac-mini · 跑单测 #3f2a [打开]┐│  终端停靠 #51d0  ⟳     │
│  卡片拖动   助│ 已回复到 微信                              │ win-pc                 │
│ ~/Secret-Chat │ ── 回执 ─────────────────────────────────  │  打包 #77c1  出错      │
│  机器人接口   │ ✓ mac-mini · 跑单测 #3f2a  312 个全过 [打开]│ 空闲 6 个 ▸            │
│               │ ⚠ 本机 · 清理构建 #9b10 rm -rf … [批准][拒绝]│                        │
│               │ ✕ win-pc · 打包 #77c1 找不到 Qt6Quick.dll   │                        │
│               │ ▸ 另外 4 条已完成                          │                        │
│ ● 本机    ⚙   │ [@] 跟助手说，或 @ 一个会话直接发给它…  [麦]│                        │
└──────────────┴──────────────────────────────────────────┴───────────────────────┘
```

- **名字**：默认「Friday 个人助理」（用户定 2026-10-07；设置里能改，回执、聊天工具里的落款、侧边栏和助手页的标题都用它；旧设置里存的默认名「助手」读成新的）。
- **侧边栏最上面固定一行**（助手的名字）：未读回执数、各会话的大致状态；不进工作目录分组。主机是别的机器时显示「Friday 个人助理 · mac-mini」（§2）。
- **助手页复用对话页**（用户要求 2026-10-07）：工具栏、四周停靠的卡片、消息列宽、输入框都和对话页一样。工具栏只有标题（助手的名字）和状态板卡片的开关——
  不显示模型名、项目筛选、设置按钮（没意义；设置在 设置 › 助手）。
  - 时间线按天分隔，往上翻按页加载（`assistant.history`）。日志条目仍标项目（§6.4），界面上不再按项目筛。
  - 时间线（Qt：`AssistantTimeline`）的滚动和对话页的消息列表一样（用户要求 2026-10-08）：按锚点排版（`AnchoredList`，v2.6 起，同对话页；`AssistantEntriesModel` 第 0 行是最新的、排在最下面），停在底部时跟随（回复边写边长、新条目）、往上翻时看着的内容不动，自己说了话（打字、语音模式）回到底部；在工具栏下沿剪掉、两边渐隐，输入栏后面是模糊底板；条目变了只通知变了的字段（流式时只有正文），行一直建着、视口外的剔除。
  - 用户消息上方标来源（「来自 微信」「语音」），助手回复下面标「已回复到 微信」。
  - 派活、管理会话的工具卡片带「打开」：切到那台机器的那个会话（`Machines.switchTo` + 选中）。
  - **回执行**：一行一条，不用气泡；左边状态图标（完成绿 / 要确认橙 / 出错红 / 停了灰），连续几条折叠成「另外 N 条」。
    要确认的那条直接带「批准 / 拒绝」（同会话里的确认，走 `runner.resolveApproval`）。
  - 工具栏的渠道小标签点开是设置 › 助手 › 聊天工具。
- **状态板**（实时）：和大脑每次看到的是同一份（§5.2）；按机器分组，在跑 / 要确认 / 出错的在前，空闲的折叠成数字；点一行打开那个会话。
  它是一张卡片，和对话页的后台任务、终端一样停在四周、能拖、能最大化（TERMINAL.md §2 的停靠，卡片 id `board`；布局记在固定的一份里——Qt 的
  PanelLayout「assistant」、Swift 固定的 UUID；第一次进助手页时自动打开在右边，之后按用户关没关）。
- **输入框**：对话页的那个（Qt 的 ComposerView 不接会话时只留附件和 leadingTool；Swift 用同一套零件拼）：**附件**（回形针、拖放、粘贴图片）、
  `@` 选会话（插入它的 `#短号`，输入框上面标「直接发给 …，不经过助手」；发出去就原样给那个会话，不经过大脑——和聊天工具里以 `#短号` 开头一样，
  在内核的 `say` 里认）；Return 发送，助手在说时空着输入框是「停」；麦克风进语音模式（P2，§8）。
- **附件**：`assistant.say` 带 `attachments`（主机上的路径；主机是远程时客户端先 `file.upload`）。主机把它们拷进 `<助手目录>/attachments/<时间>-<随机>/`，
  日志里记 `data.attachments: [{path, name}]`（时间线上画在用户气泡上方：这台上有的图片画缩略图，其余一行名字）；这一次交给大脑时文字里列出路径
  （「[附件]」），图片在大脑能看图时一起交给它；大脑派活时在 `create_session` / `send_to_session` 的 `attachments` 里原样带上（只认主机上真有的文件），
  `#短号` 直达也带过去。交给远程机器的先 `file.upload` 过去（Swift 受管端也收 `file.upload`，存在 `…/Friday/Uploads/`）；本机的直接给路径。
- **普通会话里**：助手转来的消息标「助手发送 · 来自 微信」（`QueuedInput.from` / `ChatMessage` 加来源）；助手开的会话在侧边栏行上带小机器人标记；
  会话的「⋮」菜单加「不向助手汇报」。
- **设置 › 助手**：开关与主机（默认本机，可选任一台远程，§2.1）、大脑和写回的模型（默认跟新对话一样，§3.3）、汇报规则（§5.1）、回执那一句用小模型写（§4.2）、聊天工具（沿用「服务与密钥」的卡片画廊）、
  权限（§9）、「助手记住的事」（记忆条目，能看、能改、能删，§6.4）。
- 会话跑完时的**桌面系统通知**（现在没有）随 `turnFinished` 一起做（§4.1），不开助手也有。

### 1.2 聊天工具里

能加按钮的平台（SChat、Telegram、飞书、钉钉、企业微信、Slack、Discord）：

```
完成｜mac-mini｜跑单测 #3f2a
312 个测试全部通过
跑了 2 条命令 · 4 分 12 秒
[打开详情] [接着做…]

要确认｜本机｜清理构建 #9b10
run_shell：rm -rf qt/build
[批准] [拒绝]                 ← 只有打开了「在聊天工具里批准」才有（§9），否则写「请到 Friday 里确认」
```

只有纯文本的平台（微信 ClawBot、短信式的）：

```
【完成】mac-mini｜跑单测 #3f2a
312 个测试全部通过
跑了 2 条命令 · 4 分 12 秒
回复「#3f2a 内容」可接着跟它说
```

- **回复某条回执 = 直接跟那个会话说话**（平台的「引用回复」，或纯文本里以 `#短号` 开头），不经过大脑，又快又省。
- 普通消息交给大脑理解：问状态的多数直接看状态板回答，要干活的派给会话，指代不清就反问。
- `/` 命令（支持的平台注册成命令菜单）：`/sessions` `/machines` `/new` `/stop #短号` `/status` `/bind 绑定码`。

---

## 2. 跑在哪：助手主机

助手要随时能从手机上叫到，所以跑在一直开着的地方，**不跑在界面里**。Windows / Linux 上内核在 Qt 桌面端或 friday-server 里（C++ core），
macOS 上在 Friday.app 里（Swift 版，§2.2）。

### 2.1 主机

- **主机默认是本机，用户可以改**（用户定 2026-10-07）：设置 › 助手 ›「助手跑在」：本机（默认）/ 机器列表里的任一台远程（比如一台常开的服务器）。
- **Windows / Linux 的本机**：在桌面进程里（`DesktopService`）；关掉窗口桌面端还在托盘里，助手照常跑；退出 Friday 助手就停（v2.8 起；
  v2.1–v2.7 退出时助手开着会起后台的 `friday-server --daemon` 接着跑）。要一直能叫到，把主机改成一台常开的机器（下面「换主机」）。
- **全局一个助手**：只在主机上跑；别的客户端（另一台电脑、以后的手机 App）连到主机去看它（`assistant.*`，§11.2）。
  在另一台机器上打开助手时提示「助手在 mac-mini 上跑」，给「改到这台」。
- **换主机 = 搬家**：把 `Assistant/`（日志、话题摘要、记忆、状态、渠道配置，§10）传到新主机（`assistant.export` / `assistant.import`，§11.2），
  渠道用到的密钥按名字重新同步（`secrets.set`），新主机起来、各渠道连上后旧主机上的停掉。两边不同时开着：同一个机器人被两处同时长轮询会互相抢消息。
- **主机主动连其他机器**：现在 `MachineSwitcher`（app 层）给每台远程保持一个常连的 `RemoteSession`。把这部分抽到 core 做成 **`MachinePool`**
  （`core/remote/MachinePool`）：持有 `remotes.json` 里每台机器的连接、重连、在线状态；`MachineSwitcher` 在它上面只管「现在看哪台」。
  这样没有界面的 friday-server 也能连着所有机器。主机连别的机器用它自己的 SSH 密钥（`~/.ssh/friday_ed25519`）或配对的 wss（`pairings.json`）。
- 主机本机上的会话（Windows / Linux）在进程内直接调（`ConversationStore` / `AgentRunner`），不走协议。

### 2.2 macOS：助手在 Friday.app 里

用户定（2026-10-11）：「助手是助手，friday-server 两者分开，开启助手也不应该要使用 friday-server」。v2.8 起 Mac 上助手的内核在 Friday.app 里、
用 Swift 重写（`Friday/Friday/Assistant/Core`，和 C++ 的 `core/assistant` 一一对应：同样的规则、提示词、JSON；Qt 的 tst_assistant 用例搬进了
Swift 的自检）。2.3–2.7 是 Friday.app 自带的 friday-server（C++ core 的 macOS 构建）当「助手进程」跑内核、Swift 经受管端让它管本机的会话，
这一套 v2.8 删掉了（Swift 的 AssistantHost / AssistantServer / AssistantLauncher，C++ 的 `FRIDAY_ASSISTANT_LOCAL` / `FRIDAY_ASSISTANT_APP`、
`RemoteFleetMachine` 的叫醒、`PowerManager` 的只读模式）。

- **聊天工具渠道**（§7）：`Assistant/Channels`（同 C++ 的 `channels/`）：`ChannelHub`（认主人、绑定码、命令、回执按钮、流式回复、媒体、语音回复）、
  `SChatChannel`（长轮询、排队发、上传下载、流式、自签证书按 SPKI 指纹认）、`TelegramChannel`（草稿流式）、`ScriptChannel`；`channels.json`
  同 Qt 的格式、随搬家走。扫码一键接入（`SChatSetup`，二维码用 CoreImage 画，机器人头像是 `HaloPainter.avatarJPEG`）在设置 › 助手 ›「添加 SChat 机器人」里，
  默认扫码、也能手动填 token；流程在这台 App 里跑，建好的机器人交给主机（`assistant.channels.add` 带上预先绑好的主人）。
- **换主机、别处也开着**（§2.1，同 Qt 的 AssistantModel）：设置里换「助手跑在」先问一句，可以勾「把日志、记忆和设置也搬过去」（`AssistantMove`：
  经 `assistant.export.*` 从原来的主机分块读出、换过去、新主机订上了再经 `assistant.import.*` 分块写进去，那边原有的先备份；读不出来就不换、
  写不进去说明原因）；助手页问连着的远程 `assistant.status`：这台关着、别处开着时写「助手在 mac-mini 上跑」（去看那边的 / 在这台打开），
  两边都开着时顶上一条提醒（关掉那边的 / 去看那边的）。主机换到别处时这台的助手关掉。
- **通话、P2P 文字**（§8）：`Assistant/Calls`（同 C++ 的 `calls/`）：`AssistantCalls`、`AssistantTextSession`、`VoiceActivity`、`CallAvatar`
  （CoreGraphics + vImage 画光环、转 I420）、`CallVideo`（画、编两条线程，按机器分档）。底下是同一套 Secret-Chat 通话内核（`qt/vendor/schat-call`
  的 P2PCallManager + DcMediaEngine，依赖同 `qt/cmake/FridayCallDeps.cmake`），包一层 C 接口（`Friday/Vendor/FridayCall/include/FridayCall.h`：
  内核的回调排队、叫醒，主线程上 drain 时才调出来）编成静态库 `libfridaycall.a`——Xcode 的「通话内核」构建阶段调 `Friday/Tools/CI/build_call_engine.sh`
  （要 cmake；按 Xcode 的 ARCHS 每个架构各编一份再 lipo，CI 出的通用 App 也链得上）。SChat 的通话连接在 `SChatChannel`（URLSessionWebSocketTask，
  和 HTTP 共用一个 URLSession，同样按 SPKI 指纹认证书）。装了本地语音（Power）时通话里合成走边写边读（`RealtimeSpeaker`），声纹走 `POST /v1/audio/speaker`。
- **内核**：`AssistantRuntime`（§3）、状态板、分诊、日志、记忆、话题摘要、上下文、回执写作、学习惯 / 作息。本机那台（`LocalFleetMachine`）
  在进程里直接调 `AgentRunner`、会话库、`ActivityTap`；远程机器（`RemoteFleetMachine`）复用切换机器的那条连接（`MachineSwitcher.connection`，
  助手开着时机器列表里每台都连上）。
- **数据目录沿用** `~/Library/Application Support/Friday-Assistant/Assistant/`（2.7 及以前助手进程用的同一个，日志、记忆、设置接着用）；
  引擎、密钥直接用 Swift App 自己的（不再推）。启动时 2.7 留下的助手进程还在跑（它的 `friday-server.sock` 连得上）就请它退出（`server.shutdown`），
  免得两个内核写同一份日志、抢同一个机器人。
- **协议面**：`AssistantService` 实现 `assistant.*`（§11.2，同 RemoteServer 的那段：条目、状态板、状态合批 80 毫秒推，推送、重置马上转）。
  这台 App 的助手页经 `LocalAssistantLink` 进程内直接调；别的电脑选了这台 Mac 当主机时经受管端看、用这里的助手（hello 的 features 带 `assistant`）。
- **登录时在后台运行**：设置里「登录时在后台运行助手」（默认开；助手开着、主机是这台时才注册）。登录项（`SMAppService`，
  `Contents/Library/LaunchAgents/com.example.Friday.assistant.plist`，build.sh 放的）拉起 Friday 自己带 `--assistant-daemon`：已经有一个 Friday
  在跑就退出，否则照常起来、把窗口藏起来（点 Dock 图标或再打开 Friday 就出来）；崩了 launchd 再拉起，正常退出不拉（`AssistantLoginItem`）。
- **Swift App 兼当受管端**（`ManagedEndpoint`，§11.3）：别的主机上的助手经 SSH 管这台 Mac 的会话，跑 `~/.friday-server/bin/friday-server --attach`
  （没装时 `RemoteDeploy` 同 Linux 装 macOS 的服务端包，`qt/ci/package_macos_server.sh`）。装好的包只做桥：`--attach` 接到 Swift App 的本机 socket，
  `--daemon` 等别的命令一律拒绝（Mac 上不起服务）。Swift App 没开时桥在后台拉起它（`open -g -j -b <bundle id>`，不弹窗口）再连；
  用户退出 App 后不拉（`friday-managed.quit`），状态板显示那台离线。
- **Swift 显示端**（`AssistantModel` + `AssistantView` / `AssistantSettingsView`）：同 Qt 的界面，经协议看助手（`assistant.subscribe` 拿全量、
  `assistant.*` 事件增量）；主机是这台时是进程内的 `LocalAssistantLink`，是远程时用那台的连接（和切换机器共用，RemoteClient 可以挂多个监听者）。

---

## 3. 实时内核（`AssistantRuntime`）

### 3.1 三条线

```
用户说话（文字 / 语音 / 各渠道）──→ 调度器（用户优先、随时打断）──→ 大脑：一次短反应 ──→ 回话（渠道 / 语音，先说后做）──→ 后台写回
                                        ↑                          │  ↑                                                （日志、摘要、记忆）
会话事件流（各机器实时推）──→ 分诊 + 状态板（纯代码）── 要紧的才叫醒 ┘  └── 状态板进上下文
                                        ↑                          │
                                        └──── 会话再推事件 ←── 派给会话（立即返回，不等结果）
```

- **耳朵和嘴**（渠道、语音、桌面端）：一直在收，永远不被干活卡住。用户的话是一个输入事件。
- **事件**：会话推来的 activity 事件由代码更新状态板（§5），要紧的才进调度器。
- **大脑**：一次调用模型做一次**短反应**（几秒）：理解这句话 / 这批事件，回一句话，按需派活。重活一律交给会话。

### 3.2 调度与打断

- **同一时间只有一次反应在跑**（只有一张嘴，同时说两件事会自相矛盾）。反应期间到的输入和事件攒着，下一次一起处理。
- **用户一开口就打断**：正在生成的反应立即取消（引擎请求断开，已经流出去的话保留并标「被打断」），语音播报立即停，带上这句话重新来。
  已经发出去的工具调用（派活、发消息）不撤回——它们是一瞬间的，结果照常作为事件回来。
  现有会话的「插话」要等当前一步走完（`AgentRunner::takeInterjection`），助手不用这个机制。
- **优先级**：用户输入 > 立即级事件 > 及时级事件（§5.1）。事件触发的反应在用户正在输入 / 说话时推迟到停下来之后（语音按 VAD，文字按输入框有字）。
- **自动唤醒有上限**：不是用户触发的反应每小时最多 30 次（设置），超了只更新状态板、按固定格式推送。
- 主机上没有任何客户端和渠道时也照常运转（事件照样记、推送照样发）。

### 3.3 大脑的一次反应

1. `ContextBuilder` 按当前状态拼上下文（§6.1）。
2. 流式调用大脑的模型。回复正文边出边送到来源渠道（能流式的流式，§7.2）和桌面端（`assistant.delta`）。
3. 工具调用：**每次反应最多 2 轮**（设置，`assistantToolRounds`）；超了就先回话，剩下的作为下一次反应。
4. 结束：写日志、更新焦点（§6.3）、排后台写回（§6.4）。

**大脑和写回的模型都能选，默认「跟新对话一样」**（用户定 2026-10-07）：设置 › 助手 › 大脑 / 写回各一个选择框，第一项「跟新对话一样」
（默认；取主机上新对话用的模型 `defaultEngineID`，没有就第一个引擎，同 `EngineStore::resolved`；用户改了新对话的模型，助手跟着变），
下面是全部引擎。大脑要能调工具（`supportsTools`）：选中的（或新对话的）模型不支持时设置页标红、提示另选，助手先只回话不派活。
大脑默认不开思考（首字延迟要紧，可在设置里开）；语音模式下设置页提示「首字慢可以给大脑换个快模型」。

输出分两部分：**说的**（正文：短、口语化，语音时念出来）和**给看的**（`post_detail` 工具：长内容、列表、链接，发到屏幕 / 渠道，语音时不念）。
文字渠道两部分都显示。

### 3.4 助手的工具（`ToolOrigin::Assistant`，只在助手内核里用）

都是**异步**的：派活类立即返回句柄，结果以后作为事件回来；取数类有超时（3 秒，超了返回「稍后作为事件给你」）。没有 `wait_*`。

| 工具 | 做什么 | 底下用的 |
|---|---|---|
| `list_machines` | 机器、在线状态、版本、忙不忙 | `MachinePool` |
| `add_machine` / `remove_machine` | 加 / 移除远程（自动装服务端）；密码一律不经聊天工具（§9） | `RemoteMachines`、`RemoteKeySetup`、`RemoteDeploy` |
| `sync_config` | 同步本机配置到某台：先出预览，确认后写 | `RemoteConfigSync` prepare / apply |
| `list_sessions` | 按机器、目录、状态筛 | 状态板 |
| `create_session` | 在某台机器某目录开会话，可带第一句话、模型、权限模式、标题 | `conversation.create` + `runner.submit` |
| `send_to_session` | 给会话发话；它在忙时默认排到下一轮（`whenBusy: queue`），用户在纠正、追问、叫停它正在做的事或说「马上」时插话；排到下一轮时结果说清它这一轮跑了多久（v2.8，§17） | `runner.submit` |
| `read_session` | 取会话最近 N 条（压缩过的），只用于这次反应 | `conversation.tail`（§11.1） |
| `manage_session` | 改名、置顶、停止、重试、换模型 / 权限、关注（`watch`：有结果立即报）、删除（每次确认） | `conversation.*` / `runner.*` |
| `resolve_approval` | 批准 / 拒绝某会话的待确认调用；**默认不给大脑**（§9） | `runner.resolveApproval` |
| `recall` | 搜日志、话题摘要、记忆 | §6.5 |
| `remember` / `forget` | 用户让它记住 / 忘掉某件事 | `AssistantMemory` |
| `notify` / `post_detail` | 主动推一条 / 发给看的内容 | 渠道层 |
| `think_in_background` | 牵涉多个项目的复杂编排：派一个后台子 Agent 去想，想完作为事件回来 | `SubAgentRunner` |

会话的指代：「机器 + 短号」，如 `mac-mini #3f2a`（UUID 前 4 位，同机器内冲突就加长；CLI 的 `-c` 已经按 id 前缀找）。

### 3.5 不影响会话本身

- 助手转过去的话默认排队到下一轮，不插进正在跑的那一步。
- 看会话只取一小段（`conversation.tail`），不长期订阅；主机对远程只订 `activity`，不订每个会话的 `message` 快照（那是约 10 次 / 秒的整条消息）。
- 助手派的会话就是普通会话：用户随时能在界面里直接打开、接着说、停掉，助手从事件里看到变化。

---

## 4. 会话的实时事件（`activity`）

### 4.1 会话侧：`ActivityTap`

`core/agent/ActivityTap` 挂在 `AgentControl`（本机的 `AgentRunner`）和会话库的信号上，把一段会话的动静转成小事件，并记着每段会话现在的样子
（`SessionActivity`）。`AgentControl` 加了 **`turnFinished(conversationID, TurnBrief)`**（在 `generatingChanged` 之后发，此时 `isGenerating` 已是 false），
桌面系统通知也用它。开始、结束、待确认随信号即时推；工具、步骤、等额度、卡住在生成中每 2 秒扫一次。每条事件都带会话的标题和工作目录
（没见过这段会话的一方据此上状态板）。格式见 REMOTE.md §6「会话的实时动静」。

| 事件 `kind` | 什么时候 | 带的内容 |
|---|---|---|
| `started` | 一轮开始 | 谁发起的：`user` / `agent`（后台子 Agent 交回结果）/ `assistant`（用户消息有 `sentBy`） |
| `step` | 出现新的成功的 `update_task_state`、重放出的目标或步骤变了 | 目标、步骤列表 |
| `tool` | 值得一提的工具开始了：跑命令、写文件、派子 Agent、上网、生成媒体 | 那个工具（最深的那个在跑的）；**每段会话每次扫描最多 1 条** |
| `approval` | 待确认的调用多了或少了 | 现在全部的（空 = 都处理完了） |
| `quota` | 开始等额度 | 预计恢复时间 |
| `stalled` | 生成中 300 秒没动静（等确认、等额度不算） | 多久 |
| `finished` | 一轮结束 | **简报** `TurnBrief`（§4.2）；「出错」「在问你」「被停」「等额度」是简报的 `status` |

### 4.2 简报 `TurnBrief`

用户定（2026-10-07）：**回执不每条过大脑**。理由：
- 大脑同一时间只做一次反应（§3.2），每次几秒；回执扎堆时用户说话要排在后面，接语音后它会不停插嘴，自动唤醒的上限也很快被回执用光。
- 回执要准：模型改写会把「3 个测试失败」说成「基本通过」。
- 模型挂了、等额度、很慢时，推送不能停。
- 每条会话输出都进一次大脑，被网页里埋的指令带偏的机会就多一次（§9「防注入」）。

简报分两部分，在会话所在的机器上生成（`agent/TurnBrief`，纯函数，不拖慢会话）：

- **状态和数字永远由代码写**，不经模型：状态（完成 / 被停 / 出错 / 要确认 / 在问你）、机器、会话标题和短号、这一轮改了几个文件、
  跑了几条命令（失败几条）、派了几个子 Agent、用时；会话有任务状态时加「目标 + 完成几步 / 共几步」。
- **「说了什么」那一句**，默认按规则从这一轮最后一条回答里挑（`TurnBrief::pickLine`）：
  1. 取最后一个文字块；去掉代码块、表格、引用、Markdown 符号和链接地址（留链接文字）。
  2. 明说了结论就用它：「结论：…」那一行，或标题里含「结论」的那节（「## 结论」「## 最终结论」「## 一、…最终结论（…）」，v2.8 起不要求标题正好是「结论」）
     下面那段的第一句；那段只有一句冒号结尾的引子时，引子本身写得够实（去掉冒号 15 字以上、不是「…如下：」）也用它（v2.8）。
  3. 否则取最后一段（最后一个非空段落）里第一句陈述句；跳过问句、客套话（「如有需要…」「希望对你有帮助」）、只有标题的行、
     还没做的打算（「回来后我会…」）、转折带出的旁话（「但…」）、在等用户的那句（v2.8，见 4）。
  4. 最后一段整段是问句（以「？」/「?」结尾）→ 这一条不是「完成」，改成 `question`（会话在问你，§4.1），放问句原文。
     v2.8：最后一段里有一句在等用户拍板 / 确认（「等 / 待 / 需要 / 请 + 你 / 用户 + 拍板、确认、决定…」「你 / 用户 + 拍板、确认… + 后 / 再 / 才」，
     如「你拍板出路后我继续」「等你确认后再删」），也改成 `question`，问句是「那一句。等的那句」；已经照用户决定做完的（「已按你确认的方案改完」）、客套话不算。
     （起因：.51 上一轮 5 小时的出片任务最后写「你拍板出路后我继续」，回执却是「完成」。）
  5. 最后一段挑不出来就往前一段找；都没有就用任务状态里最后完成的那一步。
  6. 截到约 80 字，在句末或标点处截。
- **最后那条回答的正文**（`TurnBrief::answer`，最多 1200 字，助手核对、想下一步用，§4.2 不推送给用户）：v2.8 起**从收尾那段往前收**——
  长回合里前面是一路的进度播报，结论在收尾的报告里；收尾那段本身超长时截它的前面（报告先说结论），不够长就往前补上前面几段，
  补不全的那段留后半截、前面加「…」。（以前截整条回答的开头：.51 上 5 小时那一轮只交出了开头的「收到，按拍板执行…」，
  收尾报告里「视频没生成出来」没到助手手上，助手对用户说成了「成片合格」。）
- **可选：小模型写这一句**（设置「用小模型给回执写一句话」，**默认关**）：在主机上用写回模型（§6.4），输入只有这一轮最后一条回答
  （`finished` 事件按需带上，§11.1）和任务状态，约 1–2k token；要求一句不超过 40 字的结论、不许改数字。不占大脑、可以并发；
  **2 秒没出来就用规则挑的那一句**；写出来的句子里出现的数字和代码给的对不上，也丢掉、用规则那句。

模板（各渠道再渲染，§7.2）：

```
完成｜mac-mini｜跑单测 #3f2a
312 个测试全部通过                                  ← 「说了什么」那一句
目标：跑一遍单测 3/3 · 改了 0 个文件 · 跑了 2 条命令 · 4 分 12 秒   ← 代码写
```

### 4.3 传到主机

- 主机上的会话：进程内直接收。
- 远程：`activity.subscribe` 先给一份**全部会话的状态快照**，之后推 `activity` 事件（§11.1）；`activity` 事件不合批，一到就立即直发给订了的客户端（直发前先把攒着的状态 flush 出去，前后不乱）（2026-10-07 按上游 v2.3 实现对齐）。
- **断线不补事件，按状态校正**：重连后重新 `activity.subscribe` 拿快照，和状态板一比就知道断线期间谁结束了（快照里带每个会话最后一次的简报和时间），
  照常出回执。状态板是「状态」不是「日志」，丢事件不影响对错。
- **老版本服务端**（hello 里没有 `activity` 功能）：退回比对 `runner.generating` 和 `summaries`，某个会话结束时订阅一下拿最后一条消息再退订；
  `RemoteDeploy` 会把远程升到新版本，退回只是过渡。

---

## 5. 分诊与状态板

### 5.1 分诊（`Triage`，纯函数）

| 事件 / 情况 | 默认等级 | 做什么 |
|---|---|---|
| `approval`、`question`、`error`、`stalled` | 立即 | 叫醒大脑，顺带给下一步建议；用户正在说话 / 讨论别的就等这句说完插一句 |
| `finished`：用户在等的 | 及时 | 叫醒大脑，接着用户的原话回（「你让跑的单测全过了，要提交吗？」） |
| 同一个项目在一个合并间隔里来了 3 条以上 | 及时 | 间隔结束时过一次大脑，合成一句 |
| 用户隔了一阵回来（30 分钟没说话后的第一句） | 随这次反应 | 这次反应**批量讲**攒下的回执（状态板的「上次跟用户说话之后」，§5.2） |
| `finished`：其他 | 汇总 | 只上状态板；按推送规则推代码模板（§4.2），不过大脑 |
| `step`、`tool`、`started`、`quota` | 只上板 | 不推，不叫醒 |

- 「用户在等的」：助手派的、用户最近问过的（在焦点里）、`watch` 过的会话。等级表在设置里能改。
- **用户正在桌面端前台看着那个会话**（客户端报告，§11.2）时，这个会话的事件只上状态板，不叫醒、不推——用户已经看见了。
- **原则：要说人话时批量过一次大脑，不是一条过一次。** 按 8 个会话同时在跑、每个一天 15 轮左右估，一天一百多条回执，
  过大脑的从一百多次降到十几次，基本都是用户本来就在等的。合并后过的那一次在自动唤醒上限（§3.2）里只算一次；回来时批量讲是用户说话触发的，不算。
- **推送规则**（设置）：全部推 / 只推助手派的、出错的、要确认的（默认）/ 不推；**合并间隔 30 秒**：间隔内的多条合成一条推送（SChat 一个会话 5 秒最多 5 条，本来也要合并）。
- **大脑挂了也照推**：代码模板的推送不经过模型；本该叫醒大脑的那几种，大脑不可用时也按模板推。

### 5.2 状态板（`WorldBoard`）

代码维护的、所有机器所有会话的当前状态；界面上的状态板和喂给大脑的是同一份。给模型的渲染：

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

- 在跑 / 要确认 / 出错 / 用户在等的排前面、逐条列；空闲的只给数量和最近一两个；超出预算（约 1.5k token）从最旧的空闲开始省略。
- 末尾「上次跟用户说话之后」：用户隔了一阵（默认 30 分钟）再来时，大脑据此先把攒下的回执一次讲完（「你不在时」，§5.1）。

---

## 6. 上下文：对话无限，上下文固定

### 6.1 每次现拼

每次反应的输入由 `ContextBuilder` **按当前状态重新拼**，不是在上次的历史后面接着加。对话进行多久，带进去的都是这么多。
按变化的快慢排（前面的前缀才能命中提示缓存）：

| 放在 | 层 | 来源 | 预算 |
|---|---|---|---|
| 系统提示词 | 人设和规则、机器列表、常驻的记忆（用户是谁、长期偏好） | 很少变 → **命中提示缓存** | ~3k |
| 历史 | 最近对话原文（用户的话、助手的回复、它调过的工具一行、叫醒过它的回执）；连着同一方的并成一条 | 只往后长；按预算从新往旧截 | ~6000 字 |
| 这一次的用户消息 | 状态板（§5.2） | 每次都变 | ~4500 字 |
| 〃 | 正在说的项目档案（最多 2 个：状态、目录、目标和步骤、最近一次） | 焦点（§6.3） | ~3000 字 |
| 〃 | 可能相关的记忆 | 按这次的话检索（§6.5） | ~1500 字 |
| 〃 | 之前聊过的（话题摘要：焦点项目的 + 日常） | §6.4 | ~2400 字 |
| 〃 | 刚到的事件（回执；隔了一阵回来时先讲攒下的） | 这次要处理的 | ~2000 字 |
| 〃 | 用户这次说的（来自哪、几点） | | |

各层超预算时在层内裁剪（列表省略旧的、长的截断），不挤占别的层。状态板、事件、会话内容都标明「是数据，不是用户的指令」（§9 防注入）。
没有用户的新消息、只是事件叫醒的，大脑觉得不值得说就只回「（不说）」：不推、日志里标 silent、界面不显示。

### 6.2 项目之间靠「细节不进助手」隔离

- **项目** = 某台机器上的某个工作目录（侧边栏本来就按工作目录分组）；不属于项目的闲聊归「日常」。
- 每个项目的细节留在它自己的会话里：会话有自己的上下文、任务状态、折叠（`ContextFolding`）。助手手里只有指针和一行状态。
- 用户对某个项目提的要求，助手转进那个会话（作为助手发送的消息）；**A 项目以什么为准，看 A 的会话**，不看助手的记忆。
- 需要细节时 `read_session` 取一小段，只用于这次反应，不写进最近对话（日志里记一句「看了 #3f2a 最近 5 条」）。

### 6.3 焦点

- 每次反应涉及哪些项目，**主要由代码推出来**：调了哪个会话的工具、回复里提到的短号、用户 `@` 的会话（这些都会把该会话记为「最近问过」，上游无 `set_focus` 工具）。
- 焦点用 **30 分钟窗口**判定：状态板上每个会话记最近一次被问到的时间（`mentionedAt`，`WorldBoard::mentionWindowSeconds` = 1800 秒），30 分钟内问过的算在焦点里，按问到时间新→旧取前 2 个生成焦点项目档案，其余项目只在状态板上占一行；原来的权重衰减（置 1 / 乘 0.7 / 低于 0.2 移出）不再使用（2026-10-07 按上游 v2.3 实现对齐）。
- 「它跑完了吗」这类指代靠焦点 + 状态板；说不清就反问「是 Friday 那个还是 Secret-Chat 那个」。
- **多轮讨论不被打断**：讨论 A 时 B 的普通事件只更新状态板；B 的立即级事件在一句话说完时插一句，说完回到 A，A 仍在焦点里（30 分钟窗口内）。

### 6.4 后台写回（不占回话的时间）

- **日志**：每条（用户的话、助手的话、回执、工具调用摘要）追加进当天的 JSONL，标上涉及的项目和会话（§10）；只追加，不整份重读。
- **话题摘要**：最近原文超出窗口时，用**写回模型**（§3.3，默认跟新对话一样；想省可以换便宜的）把最老的几轮**按项目**压进各自的摘要（不压成一大团）；
  摘要也有上限，超了再压一次（越旧越概括），原文始终在日志里。
- **记忆**：用户说出的偏好、事实、决定（「测试机是 mac-mini」「提交用 integemjack 身份」「打包失败先看 windeployqt」）由写回模型提成条目，
  去重、同一件事更新旧条目；每条带来源（日志位置）和时间。设置 › 助手 › 「助手记住的事」能看、能改、能删；`remember` / `forget` 也能改。
  v2.6 起记错的会忘掉（§16.6）：给大脑和写回模型看的每条带短编号 `[m1a2b]`，`forget` 按编号删、`remember` 的 `replaces` 改掉那条，
  写回 / 快学的 JSON 也能带 `forget` 和 `replaces`。
- **回执那一句**（开了「用小模型给回执写一句话」时，§4.2）：也用写回模型，和上面几样共用一个并发队列，但排在最前（要赶 2 秒）。

### 6.5 检索（`recall`）

- 先用关键词检索（分词 + BM25）搜记忆、话题摘要、日志，按项目和时间过滤；以后引擎支持 `/embeddings` 时加向量检索。
- 每次反应自动检索「这句话」拿几条记忆放进第 4 层；`recall` 工具给大脑主动翻旧账，结果只用于这次反应。

### 6.6 重启

重启后：状态板不落盘（无 `board.json`），连上各机器后按 `activity.subscribe` 的快照校正；最近对话从日志尾部读；`state.json` 只存 `summarizedUntil`（摘要推进到的位置），焦点不落盘，未读数只在客户端内存、渠道游标在 `channels.json`（2026-10-07 按上游 v2.3 实现对齐）。

---

## 7. 渠道（聊天工具）

### 7.1 `ChannelHub` / `ChannelAdapter`

每个平台一个适配器（C++，用 core 现有的 `Http`（含 `streamLines`）和 QtWebSockets；不引入 Node）：

```
ChannelAdapter
  start() / stop() / status            连接状态（界面上的小圆点）
  inbound → InboundMessage             {channel, chatKey, sender, text, attachments, replyTo, callbackData, audio?}
  send(OutboundMessage) → MessageRef   {chatKey, text, buttons, replyTo}
  edit(MessageRef, …)                  能编辑的平台
  stream: begin / append / finish      能流式的平台
  capabilities                         markdown、buttons、edit、stream、files、voice、maxLength、rateLimit
```

### 7.2 渲染

助手输出是 Markdown；每个渠道的渲染器转一遍：SChat、微信 → 纯文本（列表用「·」，代码块原样、去掉围栏）；Telegram → HTML 子集；
飞书、钉钉、企业微信 → 卡片 / markdown；Slack → mrkdwn；Discord → markdown。超长按平台上限切条，或改发「给看的」文件 / 链接。
能流式的边出边推（SChat streams、Telegram `sendMessageDraft`、飞书 CardKit、钉钉 AI 卡片、企业微信流式回复），其余发「正在输入」、说完一次发。

### 7.3 平台

主机多半在 NAT 后面，**先接不要公网地址的**：

| 平台 | 收消息 | 要公网 | 流式 | 按钮 | 阶段 |
|---|---|---|---|---|---|
| SChat 机器人 | `getUpdates` 长轮询 | 不要 | `streams` | inline 键盘 | P1 |
| Telegram | `getUpdates` 长轮询 | 不要 | `sendMessageDraft`（Bot API 9.5，私聊） | 有 | P1 |
| 微信（官方 ClawBot / iLink） | 扫码登录 + 长轮询 | 不要 | 只有「正在输入」 | 无 | P1 |
| 企业微信智能机器人 | WebSocket 长连接 | 不要 | 有 | 模板卡片 | P1 |
| 飞书 | 长连接事件订阅 | 不要 | CardKit 流式卡片 | 卡片 | P1 |
| 钉钉 | Stream 模式 | 不要 | AI 卡片流式 | 卡片 | P1 |
| QQ 官方机器人 | WebSocket 网关 | 不要 | — | 有限；群里只收 @ | P3 |
| Slack / Discord | Socket Mode / Gateway | 不要 | 编辑消息 | 有 | P3 |
| Matrix / 邮件 | `/sync` 长轮询 / IMAP IDLE | 不要 | 编辑（Matrix） | 无 | P3 |
| WhatsApp / LINE | Webhook | **要** | — | 有限 | 通用入口 |

各家要点（实现时核对官方文档）：

- **微信 ClawBot**（2026-03 上线，灰度中，要最新版微信）：iLink 协议，`https://ilinkai.weixin.qq.com`，POST JSON（扫码登录是 GET）；
  `getQRCode` / `pollQRCodeStatus` 拿 bot token，`getUpdates` 长轮询，`sendMessage`、`sendTyping`、`getUploadUrl`；请求头
  `AuthorizationType: ilink_bot_token`、`Authorization: Bearer <token>`、`X-WECHAT-UIN`。**协议是从官方 OpenClaw 插件看出来的，没有公开文档**，
  可能会变——全部关在适配器里，出错时状态标红、不影响别的渠道。
- **企业微信智能机器人**：`wss://openws.work.weixin.qq.com`，连上先发认证帧（botId + secret），心跳、指数退避重连；流式回复（markdown，单条上限 20480 字节）、模板卡片。
- **飞书**：长连接收事件；流式用 CardKit 卡片实体（编辑普通消息有隐性次数上限，长回复会静默失败）。
- **钉钉**：Stream 模式；AI 卡片流式要 `Card.Instance.Write`、`Card.Streaming.Write` 权限。
- **Telegram**：`sendMessageDraft` 只是 30 秒的临时预览，说完必须再 `sendMessage` 落成正式消息。

### 7.4 SChat

SChat 的机器人接口几乎是照 LLM 回复设计的，是第一个接的（服务端 `E:\Secret-Chat\chatserver\internal\server\bot_api_http.go`，
开发文档 `docs/bot_api.md`、契约 `docs/cross_end_contract_events.md`「Bot contract」）：

- **鉴权**：`Authorization: Bearer scbt_…`（用户在 SChat「我的机器人」里建，token 只显示一次）。服务端是自签证书 + SPKI 指纹固定
  （`GET /server-info` 给指纹）：适配器按指纹认证书，同 Friday 自己的配对（REMOTE.md §8）。
- **收**：`POST /bot/v1/getUpdates {offset, limit ≤ 100, timeoutSeconds ≤ 25}`（带 offset 即确认之前的）；游标存 `state.json`。
  不用 webhook（要公网；设了 webhook 时 getUpdates 返回 409）。
- **发**：`sendMessage {to: "user:N", type, content, clientMsgId（UUID v4，必填）, replyMarkup?, replyToMsgId?}`；**只有纯文本**（客户端故意不渲染 Markdown）。
- **流式**：`POST /bot/v1/streams` → `/:id/append {index, delta}`（index 连续）→ `/finish {replyMarkup?}` / `/abort`；
  一个会话同时只能一个流、每秒最多 10 次 append、总长 64 KiB、空闲 120 秒自动结束；推送通知只在 finish 时发。
- **按钮**：inline 键盘最多 10 行 × 4 个；点击是 `callback` 更新，10 分钟内 `answerCallback`。**注意**：私聊里点按钮，`callback.chatKey`
  是 `user:<botId>`，回复要发给 `user:<fromUserId>`，否则被当成 `bot_to_bot` 拒掉。
- **限流**：每个会话 5 秒 5 条、全局每秒 30 条——回执合并（§5.1）本来就要做。
- **加好友**：机器人搜不到，只能扫主人生成的加入码（`SCSH:1:bot:<token>`，SChat App「我的机器人」里出）。
  以后做「一键绑定」：Friday 用 SChat 扫码登录拿用户 token，自动建机器人、出加入码。
- **加密**：机器人收不到端到端加密的消息，跟助手的对话在 SChat 服务端是明文（用户自己的服务器）。设置里写明。
- **参考实现**：`docs/examples/claude-chat-bot.js`（长轮询 + 流式）、`bots/schat-mcp/`（整套接口的 Node 封装）。
- **语音终端**：SChat 的 m5core2 设备能绑一个机器人（设备设置 `boundBotId`），可以当助手的一副「耳朵和嘴」（P2，§8）。
- **媒体（v2.3）**：收：`message` 的 `image` / `video` / `audio` / `file`，content 是 JSON 文本 `{url, thumb?, width?, height?, duration?, wave?}` /
  `{url, name}`（旧格式是裸地址）；`/media/…` 不用鉴权（地址就是凭据）。适配器先下载到 `<临时目录>/Friday-SChat/<渠道>/`（最大 200 MB），
  下好了才交出去，同一批里按收到的顺序（后面的文字等着前面的图片）。发：先用机器人 token 传 `/upload/<image|video|audio|file>`
  （multipart：`file` + `thumb` / `width` / `height`），再 `sendMessage` 对应的类型；缩略图口径照手机两端（图片长边 400、视频封面长边 600
  取 0.5 秒处、JPEG 质量 70），语音带整秒 `duration` 和不超过 28 个 0–100 的 `wave`（iOS、安卓按整数读时长，mac 没时长就不显示）。
  各端都能放 mp3 / 16 位 wav，助手合成的语音能编成 m4a 就编（界面进程 Qt Multimedia、没界面时 ffmpeg），编不了发 16 kHz 的 wav。
  **服务端要 2026-10-07 之后的版本**：之前 `/upload/*` 只认用户的 JWT，机器人上传一律 401（`bot_api.md` 一直写着能传）——Secret-Chat
  `aba693c64` 修了（`uploadAuthorized` → `botAuth`）；老服务端上发不出去时在聊天里补一句文字说明。

### 7.5 绑定、路由

- **认主人**：粘好 token（或扫码登录）后 Friday 显示 6 位绑定码；在聊天工具里给机器人发 `/bind 123456`，这个账号成为主人（可绑多个账号）。
  **只听主人私聊**；别人发的忽略（记一条日志）；群里默认只推送、不接指令（设置）。
- **一条主线**（用户定 2026-10-07）：所有渠道、桌面端、语音进同一个助手对话（同一份日志、同一个焦点、同一份记忆），每条标来源；
  **从哪来回哪去**；回执推到设置里的「通知渠道」（可多选）。在微信里说了一半、回到桌面端接着说，助手知道在说什么。
- **回复回执直达会话**：回复带的消息引用（或纯文本里 `#短号` 开头）对上某条回执 → 直接 `runner.submit` 给那个会话（`from` 标渠道），不经过大脑；
  回一句「已转给 #3f2a」。
- 渠道的 token、secret 存 `SecretStore`（`channels.json` 里只写密钥名），同现有密钥一样只给客户端传名字。

### 7.6 「所有聊天工具」：三层

1. **内置**：§7.3 的 P1、P3。
2. **通用入口**：Webhook 收发（给有公网 / 隧道的用户）；本机 HTTP 接口（只绑 127.0.0.1，带令牌）让任何桥接程序接进来。
3. **脚本渠道**：同现有的脚本工具 / 脚本 provider：脚本按 stdin / stdout 的 JSON 行协议（`{"type":"inbound",…}` / `{"type":"send",…}`）跑一个适配器；
   加 `create_channel` 元工具，冷门平台让 Agent 自己写。

---

## 8. 语音（P2；接口 P0 就留好）

**v2.3 先做了渠道里的语音、图片、视频、文件（2026-10-07，用户要「SChat 上传文件、语音、图片、视频助手读取分析」）**：
- 能力层加「听录音」（`transcribe_audio`）、「说话」（`speak`）：百炼（Qwen3-ASR / Qwen3-TTS）、豆包语音（新的提供方类型 `volcengineSpeech`，
  密钥 `VOLC_SPEECH_KEY`：录音文件识别极速版 / 语音合成大模型）、OpenAI 兼容（`audio/transcriptions` / `audio/speech`）三家都接（用户定：做成可选、三家都接）。
  - 百炼 **Token Plan**（2026-10-08，用户第一通电话「能听到、说不了话」）：`sk-sp-` 开头的密钥只认 `token-plan.maas.qianwenaiapi.com`，
    打到 `dashscope.aliyuncs.com` 一律 401 InvalidApiKey（反过来按量付费的密钥也进不了 Token Plan）。`DashScopeBackend::resolvedBase`：
    选项地址去掉 `/compatible-mode/v1` 这类后缀；Token Plan 密钥 + 百炼的公网地址 → 换成 Token Plan 的（自己填的代理不动）。Token Plan
    套餐里没有 Qwen3-ASR / Qwen3-TTS：不填模型时听录音用 `qwen-audio-3.0-asr-flash`、说话用 `qwen-audio-3.0-tts-plus`（音色 `longanhuan_v3.6`）、
    生图用 `qwen-image-2.0`。`qwen-audio-*` / `fun-asr*` / `cosyvoice*` 走原生接口：识别 = `/api/v1/services/aigc/multimodal-generation/generation`
    的 `input.messages[].content[].input_audio`（结果 `output.text`），合成 = `/api/v1/services/audio/tts/SpeechSynthesizer`
    （`input {text, voice, format: wav, sample_rate: 24000}`，返回 `output.audio.url`）。兼容模式 `chat/completions` 带 `input_audio` 在 Token Plan 上回 400。
    在用户的 Pi5 上用真密钥验过：合成 → 下载 WAV → 识别回原文。
- 进：语音消息先听写（「听录音」），听写的文字交给 `say(…, spoken = true)`：上下文里标「语音消息，下面是听写的文字」、大脑回答更短更口语；
  日志里那句带 `spoken`（时间线上标「语音」）；录音本身作为附件留着。没有能听录音的服务时直接回一句去哪接，不交给大脑。
  图片、视频、文件作为附件交给大脑（没说话时上下文写「只发了附件，没说话」）。
- 大脑的新工具：`look_at`（`AssistantMedia::lookAt`：图片 → 看图、视频 → 看视频、录音 → 听写，文本文件直接读最多 3 万字，别的让它交给会话）、
  `send_file`（这台主机上的文件说完这句就发到聊天里，桌面端显示在那条回复下面）。
- 出：回语音消息的那条回复，文字照常流式发，再合成一条语音（「说话」，要 PCM 自己编码、量时长和波形；只念「说的」部分、去掉 Markdown、
  太长的念前 600 字），放在 `<助手目录>/voice/`（一周前的删掉）。设置 › 助手 › 聊天工具「语音消息用语音回」（默认开）。
  没有能说话的服务时每次启动提醒一次。

**通话（v2.3，用户 2026-10-07 要「助手主动给我打语音 / 视频，我也能给助手打」；助手那边的画面 = 头像 + 说话时的波形）**：
- **SChat 那边**（Secret-Chat `docs/cross_end_contract_events.md`「机器人通话」，chatserver + 四端）：机器人用自己的 token 开一条只走通话信令的
  WS（hello 带 `botToken`，子协议 `schat-ws.v1`），收发和普通客户端一样的 `p2p_request` / `p2p_response` / `p2p_signaling` / `p2p_glare`；
  `/bot/v1/setCallSettings {enabled}` 声明能通话（`formatUser` 的 `botCalls`，四端据此给机器人会话显示通话入口）；`/bot/v1/getIceServers`
  给 TURN 临时凭据（机器人在家里的 NAT 后面、手机在移动网络，一端有中继候选就打得通）；机器人打给同一个用户 10 分钟最多 3 次。
- **媒体**：Secret-Chat 桌面端和手机两端真机联调过的那套（`qt/vendor/schat-call`：`DcMediaEngine` = libdatachannel + Opus 48 kHz 单声道 +
  openh264 baseline，`P2PCallManager` 状态机，信令写法转换），编进 friday_core（`cmake/FridayCallDeps.cmake`，`FRIDAY_CALLS`）。
- **SChatChannel**：`options.calls`（默认开）时连上后 `setCallSettings`、开通话连接（同样按 SPKI 指纹认证书，断了 3 秒起翻倍重连、25 秒 ping）。
- **AssistantCalls**（`core/assistant/calls/`，ChannelHub 持有，同一时间一通）：
  - 接：只接主人打来的（设置「接我打来的电话」，默认开），先拿 TURN 再自动接；别人打来的回 `user_rejected`，另一通在打时回 `busy`；
    接通先应一声「我在，你说。」。
  - 打：大脑的 `call_user {reason, video?}`，打给最近跟助手说话的主人；接通先念 `reason`。设置「助手主动打电话」：
    `important`（默认：你让它打，或有要紧的事——会话要批准、出错停了；没人让打的一刻钟最多一次）/ `asked`（只在你让它打时：这次反应是你说话引起的）/ `never`。
    v2.8 设置「免打扰」（`quietStart` / `quietEnd`）的时段里没人让打的不打（§17）。
  - 听：对方的声音（48 kHz）换成 16 kHz → `VoiceActivity`（能量 + 跟着走的噪声底：开口 120 毫秒、说完 550 毫秒、带上开口前 300 毫秒、
    少于 300 毫秒有声的不算）→ 听写 → `say(…, source = "call:<渠道>", spoken)`（上下文里来源是「SChat 通话」，提示大脑只说一两句、长内容 post_detail）；
    视频通话时带上对方摄像头最新的一帧（大脑能看图就看得见）。打断（v2.5，用户 2026-10-09「我说话不打断，等我这话交给 llm 的时候再打断」）：助手在念、在想时用户开口，它照常念；
这句说完、听写出字、要交给 `say()` 的那一刻才停下正在念的（`stopSpeaking`），`say()` 那边取消正在想的这次、带上新的话重来；
噪音、咳嗽听写不出字，不打断（2026-10-08 以前是连着出声 300 毫秒就停，`VoiceActivity` 的 `bargeIn` 现在不用了）。
视频画面从用户说完（在听写）到助手开口转加载（`CallAvatar` 的 Thinking：光环上一段白色亮弧绕环转）。v2.6（用户 2026-10-10「不是我的声音还是能识别」）：开着「只听我的声音」时每句也算声纹、不像你的不交（§16.2）；只听出一个字母的（「O.」「H.」，杂音）不交。回复的头一句在第一个逗号处就先合成；Qwen-Audio-ASR 对没有人声的录音回 400 `{}`，当成没说话。每句话在日志里记一行「听写 / 大脑 / 头一句合成 / 说完到开口」各几秒。
  - 说：这次反应的回复边生成边按句切（。！？；换行，太长在逗号处），逐句「说话」（要 WAV），换成 48 kHz 排进播放队列；每 20 毫秒按真实时间往引擎里送
    一帧（没有就送静音）。post_detail 的长内容发到那个聊天里；通话中来的回执、助手主动说的顺便念一下。没有能说话的服务时说的话发到聊天里。
  - 画面（视频通话）：`CallAvatar`（竖的 9:16，默认 720×1280，同手机摄像头的 720p）。v2.5 起是 Friday 的光环（`visual/HaloRenderer`，用户 2026-10-09 选「流光光环」、光效「二档 + 三档」，
    和 App 图标、App 里的实时语音同一个）：深色底、中间偏上一圈流动的彩色光环（用户 2026-10-09 要比原来小一半：原来 3:4 画面里环在短边 0.36 倍处 → 0.18；
    换成 9:16 后手机铺满屏幕时左右裁得少，按比例放到 0.24，手机上看起来一样大；光晕、光尘跟着缩，`HaloStyle::scale`），光雾散射、中间泛光、光尘往外飘；在听时外沿跟着用户的音量
    （对方传来的声音算的）起波纹，在说时跟着念出来的音量起伏、光尘变多，在想 / 接通中一段白色亮弧绕环转。以前的头像加竖条不用了。
    不画字（没有界面的 friday-server 没有字体库）。
  - 清晰度：手机上是铺满屏幕（aspect fill）放的，原来 480×640（3:4）要放大约四倍、发虚；openh264 又写死按 30 帧分码率，每秒只送 15 帧时
    800 kbps 实际只用到约 400 kbps。用户要过 2K 60 帧（5 Mbps）：超出 SDP 协商的 H.264 级别（Baseline 3.1，`42e01f`），一个关键帧上百个 RTP 包
    一下子全发出去，真机上丢包、解不出来、一直要关键帧——**卡住不动、发糊**（用户 2026-10-09：「手机摄像头 720p 就很清楚」）。现在：
    - 分三档（`AssistantCalls.cpp` 的 `kVideoLevels`）：720×1280 30 帧 1.5 Mbps（正好在 Baseline 3.1 以内）→ 540×960 30 帧 1 Mbps → 360×640 15 帧 0.5 Mbps。
      起画面时从最高一档往下量画一帧要多久（超过这档间隔 75% 的降一档），通话中画或编的滑动平均一直超过间隔的 90% 也降一档（只降不升）；
      日志「[助手] 通话画面：…」「…跟不上…降到…」「通话画面结束：…每秒 N 帧」。
    - 视频轨道（vendor 的「Friday 版」改动）：`RtcpNackResponder` 丢包重传、`PacingHandler` 固定按 4 Mbps 匀着发（5 毫秒一拍，`kVideoPacingBps`）；
      `setVideoEncoding(码率, 帧率, 线程数)` 让编码器按实际送的帧率分码率，CPU 线程数的一半（最多 4）开线程、按同样多的条带编。
    - 两条线程：一条画、一条转 I420 + 编码（并行，编不过来时没编的那帧直接换成新的）。渲染器提速：散射在四分之一大小上直接画、
      几档先叠好只放大一次，半径大的几档再缩小一半 / 四分之一模糊，模糊按行走（不按列跳着读），全尺寸只画环那一块（1440×1920 一帧从约 31 毫秒降到约 9 毫秒）。
    - libdatachannel 没有带宽估计：网络很差时还是会卡，可以再按对方的丢包（RTCP RR）降档。
  - 时间线上记「小王 打来语音电话」「给小王打视频电话」「通话结束 · 3 分 05 秒」「电话没打通：…」。
- 设置 › 助手 › 聊天工具：「接我打来的电话」「助手主动打电话」、每个渠道的「能打电话（通话已连上 / 还没连上）」（Qt + Swift）。
- 测：`TestCalls`（两端引擎在进程里互连、互发 Opus 和 H.264）、`TestAssistantChannels::calls`（假渠道 + 同一套状态机和引擎当手机：
  主人打来 → 应答、听写、大脑、逐句念回去、手机听得到；`call_user` 打视频电话、手机收到助手的画面、挂断带通话秒数；不是主人的拒掉；设置不让打的不打）、
  `callSocket`（假 SChat 服务器上的 `/ws`：hello、self、主人打来先拿 TURN 再接、关掉通话）。真机（手机、SChat 桌面端）要用户测。
  Swift（v2.8）：自检 `Friday/Friday/Agent/AssistantCallSelfTests.swift` 搬了 `calls` / `p2pText` / `voiceActivity` / `p2pTextMessages`（假手机是进程里的
  另一部通话内核；P2P 文字的假手机用 `friday_text_create_offerer` 这个测试接口当主叫）；`callSocket` 用 `Friday/Tools/Tests/fake_schat.py` 的 `/ws` 跑。

**本地语音（Qt 版 v2.3，2026-10-08，用户要「本地 GPU 实时 stt，识别到一句就开始分析和边写边读」）**：按需下载的本地能力
（Friday.Powers 的 friday-voice：SenseVoice 实时听写 + CosyVoice3 边写边读），见 POWERS.md。装了之后：
- 助手页工具栏的麦克风 = 语音模式：边说边出字幕，一句说完马上 `say(…, "voice")`，回复边生成边合成边播；「停」打断。
  v2.5 念的时候也听（默认开）：你开口它照常念，说完一句交给助手时才打断；外放听到的它自己的话丢掉（POWERS.md §4）。
- 通话里听写自动走本机，合成走本地流式（接通开一条连接，打断时取消）。

**P2P 文字（v2.3，2026-10-08；SChat 手机两端 iOS 9.18.14 / 安卓 9.18.7 起对 `botCalls` 的机器人也给「P2P 文字」入口，同一道闸）**：
- **线上字节一个不加**（Secret-Chat `docs/cross_end_contract_events.md`「机器人通话」的 P2P 文字一条），机器人照手机之间的现行口径：
  - 手机（主叫）在通话连接上发 `p2p_request {requestId, callType:"text", …}`；助手照通话回 `p2p_response {targetUserId, requestId, accepted, timestamp}`。
  - 手机收到 accepted 才建 PeerConnection：一条 DataChannel（label `p2p-text`、有序、主叫建，被叫在 `onDataChannel` 拿到；手机重建时是
    `p2p-text-<时间>`），offer 没有音视频 m 行。`p2p_signaling` 的 `offer` / `answer` / `ice_candidate`（payload `{candidate, sdpMid, sdpMLineIndex}`）/
    `hangup`（连上过的带 `durationSec`）/ `request_cancel`，字段同通话。
  - 通道上一帧一条 UTF-8 JSON 文字帧（iOS / 安卓 `P2PDataMessage`，iOS 严格解码，这几个键都要有）：
    `{"id":"<uuid>","type":"text","senderId":<发的人>,"receiverId":<收的人>,"timestamp":<毫秒>,"textContent":"…","isComplete":true}`；
    图片 / 视频 / 语音 / 文件是头信息（`chunkIndex:-1`、`fileName`…）+ 分片（`chunkIndex`≥0、`chunkData` base64），助手不收。
  - 手机要把文字升级成语音 / 视频（`upgrade_request`，payload 是 `{requestId, targetType, fromUserId, toUserId, timestamp}`）：回 `upgrade_reject`（同形 payload）。
- **AssistantTextSession**（`core/assistant/calls/`，AssistantCalls 持有）：
  - 接：`callType:"text"` 的请求过和通话同一道闸（主人、`enabled && answerCalls`，否则 `user_rejected`）；同一时间一个文字会话：
    别人 / 别的渠道再开回 `busy`，**同一个人在同一个渠道又开一个就换掉旧的**（旧的多半已经断了，不让他干等），`requestId` 一样的（重连后服务端补投的）不理。
    和语音 / 视频通话互不相干：各用各的 PeerConnection，`p2p_signaling` 按 `requestId` 分给它，不占通话。
  - 连：先拿 TURN（`getIceServers`，同通话）、直接用 libdatachannel 建 PeerConnection（STUN 列表照抄 `DcMediaEngine`，`disableAutoNegotiation`），
    再回 accepted；offer 到了 answer，ICE 两边 trickle（对端的回环 / 198.18.0.0/15 候选不要，同 `DcMediaEngine`）。`vendor/schat-call` 一行没改。
  - 收：文字 → `say(…, source = "p2ptext:<渠道>")`，**不标 spoken**（是打字：不合成语音、不按口语回；上下文里来源叫「SChat P2P 文字」，
    不带「通话」两个字，人设里「… 通话」那条口语规则管不到它；人设另有一条「… P2P 文字」：照常打字回、会转成纯文本、文件发到普通聊天）。媒体的头信息每条回一句「这里我只看得懂文字…」，分片不理。
    升级请求拒掉后在通道里说一句「要语音或视频，直接给我打电话」。
  - 回：`push` 给这个来源的（回复、`#短号` 直达的「已直接发给…」，回复里带着 post_detail 的长内容）→ `ChannelRender::plainText`、按 4000 字切条，
    同形发回去（`senderId` = 机器人，`receiverId` = 手机那个人）；通道还没开 / 正在重建时先攒着。会话结束之后才到的回复、没发出去的，
    发到和这个人的普通聊天里。`send_file` 的文件也发到普通聊天里（通道上不传文件），通道里说一声。
    通道开着时来的回执、助手主动说的（推到通知渠道的、没有指定去处的）也在通道里发一条（同通话里顺便念一下；通道没开时不攒，免得收尾时又进普通聊天）。
  - 收尾：对方 `hangup` / `request_cancel`；PeerConnection 失败 / 关了；接了 30 秒通道还没开；通道关了 10 秒没重建（手机是主叫，看到通道关了会自己重建）；
    渠道没了。自己这边收尾时发 `hangup`。析构（退出）只关连接、不发信令。
  - 时间线上记「小王 发起 P2P 文字」「P2P 文字结束 · 3 分 05 秒」（没连上：「P2P 文字没连上（30 秒没连上）」）。
  - 通道断了只等手机重建（手机是主叫，看到通道关了自己建 `p2p-text-<时间>`），**不发** `datachannel_request`：手机自己已经在重建时再收到请求会再建一条、
    关掉刚建的那条，又触发一次重建。新通道来了换着发，旧的收回调、关掉（这时它在手机那边已经关了）。
- Swift 版：v2.8 起通话、P2P 文字也在 App 里（`Friday/Friday/Assistant/Calls`，§2.2），同一套内核、同样的字段和规则；
  助手页时间线的来源（`AssistantView.sourceLabel`）同 `ContextBuilder::sourceLabel` 认 `call:` / `p2ptext:`，设置「接我打来的电话」下面注明也管 P2P 文字。
- 测：`TestAssistantChannels::p2pTextMessages`（纯函数：消息的解析 / 拼装、ICE payload、回复切条、接不接）、`p2pText`（假渠道 + 用 libdatachannel
  照手机的做法当主叫：接、offer 只有 DataChannel、answer / 候选的字段、打字 → 大脑（来源「SChat P2P 文字」、不是语音）→ 纯文本发回、
  图片回一句、升级拒掉、聊着时的回执也进通道、别人开的拒掉、补投的不理、挂断后记一笔、之后才到的回复进普通聊天）。真机（iPhone / 安卓）要用户测。

**原来的设计**：
- 内核的输入输出统一是「一句话」的流；语音只是又一个适配器（桌面端麦克风、SChat 语音终端、渠道里的语音消息）。
- **输入**：VAD 检测说话 + 流式识别（边说边出字，界面上实时字幕），断句后交给内核；**用户一开口就算打断**（停播报、取消反应，§3.2）。
- **输出**：大脑的「说的」部分边生成边按句切，交给流式 TTS；「给看的」不念，发到屏幕 / 渠道（「详细结果发你微信了」）。
- **延迟目标**：用户说完约 1 秒出声。靠：上下文固定且小、前缀走缓存、大脑不开思考、先说后做（派活是异步的，先应一声）；新对话的模型首字慢时，给大脑单独换个快模型（§3.3）。
- **两条路**：
  - **级联**（先做）：识别 → 大脑 → TTS。百炼、方舟都有流式识别 / 合成接口，做成新的能力 provider；现有 `SpeechReader`（系统 TTS）当兜底。文字日志天然就有。
  - **端到端实时语音模型**（以后）：它当「耳朵和嘴」，用 function calling 调同一套内核工具；内核不变。
- 语音模式界面：输入框的麦克风；实时字幕、波形，播报时有「停」。录音默认不保存，只存识别出的文字（设置）。

---

## 9. 安全与默认值（都做成设置，默认保守，用户能关）

| 设置 | 默认 |
|---|---|
| 助手 | 关；打开时主机默认本机（可改） |
| 大脑的模型 / 写回的模型 | 跟新对话一样（§3.3） |
| 大脑思考 | 关 |
| 新建会话、给会话发消息、看会话列表 | 允许 |
| 删除会话、添加 / 移除机器、同步配置 | 每次确认（从聊天工具发起的就在聊天工具里确认） |
| 在聊天工具里批准工具调用 | **关**（开了也只在能加按钮的平台出现） |
| 助手（大脑）自己批准工具调用 | **关** |
| 从聊天工具开 Full 权限的会话 | **关** |
| 推送规则 | 只推助手派的、出错的、要确认的 |
| 用小模型给回执写一句话 | 关（用规则挑，§4.2） |
| 免打扰（这段时间不主动打电话） | 不设（v2.8：设置里设，或用户说了大脑用 `set_quiet_hours` 设，§17） |
| 合并间隔 | 30 秒 |
| 自动唤醒上限 | 每小时 30 次 |
| 每次反应的工具轮数 | 2 |
| 群聊 | 只推送，不接指令 |
| 保存语音录音 | 关 |

- **SSH 密码不走聊天工具**（会以明文留在别家的服务器上）：`add_machine` 要密码时转到 Friday 里的安全输入框（`request_secret` 那套），渠道里只回「请到 Friday 里输入」。
- **防注入**：回执、会话内容、渠道里别人转来的文字一律当数据（放在上下文的数据区、标明来源），不当指令；删除、批准这类动作不能只凭模型一句话。
- **防死循环**：助手自己的输出不变成给自己的事件；自动唤醒有上限；同一会话的同一种事件合并。
- **审计**：大脑的每次工具调用都留在助手时间线上（工具卡片）和日志里。

---

## 10. 存储

**助手不进 `conversations.json`**：那是所有会话一个 JSON 数组、每次保存整份重写；`conversation.subscribe` 也是整段推、不分页。无限长的助手会把两边都拖垮。

```
<dataDir>/Assistant/
  journal/2026-10-07.jsonl   按天追加：{id, at, kind: user|assistant|receipt|tool|event, source, text, projects[], refs[], interrupted?}
  journal/index.json         每天的条数和涉及的项目（按项目筛、往上翻时跳过不相关的天）
  topics.json                {项目: {summary, coveredUntil, updatedAt}}
  memory.json                [{id, kind: preference|fact|decision, text, project?, source, createdAt, updatedAt}]
  state.json                 summarizedUntil（摘要推进到的位置；2026-10-07 按上游 v2.3 实现对齐：无 board.json，焦点不落盘，未读数在客户端内存，渠道游标、绑定的主人在 channels.json）
  channels.json              渠道配置（token 只写 SecretStore 里的密钥名）
  media/                     渠道收到的图片 / 文件、（开了的话）语音录音
```

`<dataDir>` 是主机的数据目录；Mac 上是 `~/Library/Application Support/Friday-Assistant/`（§2.2，沿用 2.7 及以前的），不在 Swift 的数据目录里。
格式规则同其他数据文件（ARCHITECTURE.md：大写 UUID、ISO 时间不带毫秒）。日志不删；设置里给「清空助手的日志和记忆」。

---

## 11. 协议改动（实现时写进 REMOTE.md）

都是**可选功能**：hello 里双方报 `features`，没有的退回旧做法，不加 `protocol` 版本号。

### 11.1 主机 ↔ 各机器

- `hello {…, features: ["activity"]}` → 回复里 `server.features`。
- `activity.subscribe {}` → `{sessions: [{id, title, workingDirectory, status: idle|running|approval|question|quota|error, task?: {goal, steps:[{text, status}]},
  lastBrief?, lastFinishedAt?, approvals: [{eventID, tool, summary}]}]}`；之后推事件 `activity {conversationID, seq, kind, at, …}`（§4.1）。`activity.unsubscribe`。
  `activity.subscribe {finalText: true}` 时 `finished` 事件多带这一轮最后一条回答的文字（截 2000 字），给主机上的小模型写回执那一句（§4.2）；默认不带。
- `conversation.tail {id, count, maxChars}` → 当前路径最后几条的压缩版（文字截断、工具只留名字和一行结果）。也是 §10「以后的手机 App」里分页的第一步。
- `runner.submit` 加 `from?: {kind: "assistant", channel?}`（会话里显示「助手发送 · 来自 微信」）和 `whenBusy?: "queue" | "interject"`（默认 queue）。
- `conversation.create` 加 `title?`、`engineID?`、`createdBy?: "assistant"`。

### 11.2 客户端 ↔ 主机（看助手）

- `assistant.subscribe {}` → `{board, entries（最近一段日志）, unread, focus, channels（状态）, settings}`；之后推 `assistant.board`、`assistant.entry`、
  `assistant.delta {entryID, text}`（流式）、`assistant.speaking {entryID | null}`、`assistant.channels`。
- `assistant.history {before, limit, project?}`：往上翻。
- `assistant.say {text, attachments?}`（语音 P2：`assistant.audio`）、`assistant.stop`（打断）。
- `assistant.memory.list / update / remove`、`assistant.settings.update`、`assistant.channels.update`、`assistant.bindCode`。
- `assistant.export.list / .read` → 分块读出 `Assistant/` 的全部文件；`assistant.import.begin / .write / .finish` 写进新主机（换主机，§2.1；新主机原有的先备份）。
  只给本地 socket（SSH / WSL）连接用，同 `server.pair`（REMOTE.md §6「助手」）。
- `assistant.presence {conversationID | null, foreground}`：客户端报告前台正在看哪个会话（切会话、窗口进出前台时发）；
  主机据此不叫醒、不推这个会话的事件（§5.1）。断开即视为没在看。
- 没开助手的机器回错误 `3`（找不到）；客户端据此显示「助手在 mac-mini 上」或「打开助手」。

### 11.3 Swift 受管端（Mac 本机，§2.2）

Swift App 在本机 socket（Unix domain socket，只有同一个用户能连）上实现协议的**子集**，消息格式、握手、错误码与 REMOTE.md 完全一样：

- socket 在 `~/Library/Application Support/Friday/friday-managed.sock`（权限 0600；另一个 Friday 已经在听就不开）。
- `hello` → `server.host: "swift"`、`features: ["activity", "tail", "submitFrom"]`；`state` 只有 `summaries`、`engines`、`runner`。
- 会话：`conversation.create`（带 §11.1 加的字段）、`.remove`、`.rename`、`.setPinned`、`.tail`；事件 `summaries`。
- 运行：`runner.submit`（带 `from` / `whenBusy`）、`.stop`、`.resolveApproval`、`.setEngine`、`.setPermissionMode`、`.setThinking`、`.retry`；事件 `runner`。
- 动静：`activity.subscribe` / `.unsubscribe`、事件 `activity`（Swift 的 `ActivityTap` / `TurnBrief`，规则同 C++，§4）。
- 助手：`assistant.*`（v2.8，`AssistantService`，§2.2）：别的电脑看、用这台 Mac 上的助手。
- 其余方法（终端、文件、`conversation.subscribe` 整段推、`server.*`）回错误 `4`（不允许）：Swift 的界面照旧由 Swift 自己画，受管端只给助手用。
- `friday-server --attach`（macOS，装好的服务端包里的；自己编的加 `--app`）：把 stdin / stdout 桥到 Swift App 的这个 socket；
  App 没开时先 `open -g -j -b <bundle id>` 在后台拉起、等 socket 出现（最多 20 秒），拉不起来（没登录图形界面）、
  连不上都写一条 `bridgeError` 说明。用户自己退出了 App（Swift 退出时在 socket 旁边留 `friday-managed.quit`，关机 / 注销不算，下次启动删掉）就不拉起，
  回「已经退出了」，客户端照常过一会儿重连，用户打开 App 后连上。测试用的环境变量：`FRIDAY_MANAGED_SOCKET`（socket 路径）、`FRIDAY_APP_BUNDLE_ID`。
- 别的机器的客户端：`RemoteDeploy` 探测到 `Darwin` 时先在 `/Applications`、`~/Applications`、Spotlight（按 bundle id）里找带
  `Contents/Helpers/friday-server` 的 Friday.app（2.7 以前的才有；装好的优先，Xcode 编的、更新时下载的最后），有就把 `~/.friday-server/bin/friday-server`
  链接过去；没有（2.8 起）就同 Linux 装 `macos-arm64` 的服务端包（Swift 客户端一直是这样）。连上是 `host: "swift"` 的：不自动升级（桥只转发）；切换机器不切过去
  （「xx 上是 Mac 版的 Friday：会话请在那台 Mac 上看」，列表里写「Mac 版 Friday，给助手管」，没连过的切过去、连上发现是它就切回本机），
  助手回执上的「打开」同样说明。

---

## 12. 代码结构

```
qt/src/core/assistant/
  AssistantRuntime.{h,cpp}   事件循环：收输入和事件、调度、打断、调大脑、排写回
  Triage.{h,cpp}             事件分级（纯函数）
  WorldBoard.{h,cpp}         状态板：各机器各会话的状态；渲染给模型 / 界面
  ContextBuilder.{h,cpp}     拼上下文、按层裁预算（纯函数）
  AssistantTools.{h,cpp}     助手的工具（ToolOrigin::Assistant）
  Journal.{h,cpp}            按天 JSONL、分页读、项目索引
  TopicSummaries.{h,cpp}     按项目的滚动摘要
  AssistantMemory.{h,cpp}    记忆条目、关键词检索
  BriefWriter.{h,cpp}        （可选）小模型写回执那一句：2 秒超时、数字校验，不行就用规则那句
  channels/                  ChannelHub、ChannelAdapter、渲染器；SChat、Telegram、Weixin、WeCom、Feishu、DingTalk、Webhook、Script…
  calls/                     SChat 通话（AssistantCalls、VoiceActivity、CallAvatar）和 P2P 文字（AssistantTextSession），§8
  voice/                     （P2）VAD、流式识别 / 合成的接口
qt/src/core/agent/ActivityTap.{h,cpp}      会话侧：runner / 任务状态 / 待确认 → activity 事件；AgentControl::turnFinished
qt/src/core/agent/TurnBrief.{h,cpp}        简报：代码写状态和数字、按规则挑那一句（纯函数，§4.2）
qt/src/core/remote/MachinePool.{h,cpp}     从 MachineSwitcher 抽出来的连接池
qt/src/core/remote/RemoteServer.cpp        activity.*、conversation.tail、assistant.*
qt/src/cli/ServerCommand.cpp               macOS：--attach 桥到 Swift 受管端（§11.3）；在 App 里 / 装好的包里时插件目录用 Contents/PlugIns / PlugIns
qt/src/core/remote/RemoteDeploy.cpp        认出 macOS：2.7 以前的 App 带 helper 就链接，没有就装 macos-arm64 的服务端包
qt/ci/build_macos_helper.sh                macOS：编 helper、部署 Qt 框架和插件到 staging 目录（§12「macOS 打包」）
qt/ci/package_macos_server.sh              macOS：staging 目录打成服务端包（签名、server.json），v2.8 起
qt/src/app/viewmodels/Assistant*.{h,cpp}、qt/src/app/qml/assistant/   助手页、回执行、状态板卡片、设置页
Friday/Friday/Assistant/ActivityTap.swift、TurnBrief.swift           同 C++，规则一致（§4）；AgentRunner 的 onTurnStarted / onTurnFinished / onApprovalsChanged
Friday/Friday/Assistant/ManagedEndpoint.swift                        Swift 受管端：本机 socket 上的协议子集（§11.3）
Friday/Friday/Assistant/Core/                                        v2.8 Swift 的助手内核（同 C++ 的 core/assistant）：AssistantRuntime、WorldBoard、Triage、
                                                                     Journal、AssistantMemory、TopicSummaries、UsageLog、ContextBuilder、BriefWriter、Fleet、
                                                                     AssistantTools、AssistantMedia；AssistantService（assistant.*）、LocalAssistantLink
Friday/Friday/Assistant/Channels/                                    v2.8 Swift 的聊天工具渠道（同 C++ 的 channels/）：ChannelHub、SChatChannel、TelegramChannel、
                                                                     ScriptChannel、ChannelRender、ChannelMedia
Friday/Friday/Assistant/Calls/                                       v2.8 Swift 的通话（同 C++ 的 calls/）：AssistantCalls、AssistantTextSession、VoiceActivity、
                                                                     CallAvatar、CallVideo、CallSignalWire、CallEngine（NativeCall / NativeTextPeer：C 接口的外壳）
Friday/Vendor/FridayCall/                                            通话内核的 C 接口（FridayCall.h / .cpp）和 CMake：schat-call + 依赖合成 libfridaycall.a
Friday/Tools/CI/build_call_engine.sh                                 编 libfridaycall.a（Xcode 的「通话内核」构建阶段调；按 ARCHS 分架构编、lipo）
Friday/Friday/Assistant/Channels/SChatSetup.swift                    v2.8 SChat 一键接入（扫码登录建机器人、加好友、传头像）
Friday/Friday/Assistant/AssistantMove.swift                          v2.8 换主机搬家：assistant.export.* / import.* 分块读写
Friday/Tools/Tests/fake_schat.py                                     假 SChat 服务端：测渠道（绑定、收发、流式、上传下载）、通话连接（/ws）、扫码登录
Friday/Friday/Assistant/AssistantLoginItem.swift                     v2.8「登录时在后台运行助手」：登录项拉起 Friday --assistant-daemon、藏窗口；请 2.7 的助手进程退出
Friday/Friday/Agent/AssistantSelfTests.swift、AssistantRuntimeSelfTests.swift   v2.8 助手内核的自检（搬 Qt 的 tst_assistant）
Friday/Friday/Agent/AssistantCallSelfTests.swift                     v2.8 通话的自检（纯函数 + 假渠道、假手机的端到端）
Friday/Tools/Tests/mock_assistant.py                                 假模型：测助手一整圈（说话 → 开会话 → 回执 → 汇报）
Friday/Friday/Assistant/AssistantModel.swift                         Swift 显示端的数据：时间线、状态板、渠道、记忆、换主机、presence、推送通知
Friday/Friday/Views/AssistantView.swift、AssistantSettingsView.swift  助手页（侧边栏「助手」一行、时间线、回执、状态板、输入框）、设置 › 助手
Friday/Tools/CI/build.sh                                             放进助手的登录项（v2.8 起不放 helper：拉起 Friday 自己带 --assistant-daemon）
```

- `FridayCore` 持有 `AssistantRuntime`（开了助手才建）；friday-server 和 Qt 桌面端都有。
- **macOS 打包**：Friday.build 的 macos 任务装 Qt（6.11.3 的 macOS 版，同 `qt/ci/deps.env`，只要 qtwebsockets）、跑 `qt/ci/build_macos_helper.sh`：
  编 arm64 的 friday-server，借 macdeployqt（`-no-plugins`）部署它链接的 Qt 框架，插件只拷要用的（TLS 的 Secure Transport 和只认证书、
  网络状态、jpeg / gif / webp / heic），全部瘦成 arm64。v2.8 起（§2.2）签名身份导入后由 `qt/ci/package_macos_server.sh` 打成服务端包：
  `friday-server/bin/friday-server`（装载路径 `@executable_path/../Frameworks`，`bin` 和 `Frameworks` 是兄弟目录）、`Frameworks/Qt*.framework`、
  `PlugIns/<类别>/`、`server.json`，由里到外签名（hardened runtime，正式签名加时间戳），解开到别处跑一次 `--version` 核对；工作流再解开打成 zip
  交给公证（只是登记：单个程序钉不了票据，装到用户机器上也不带隔离属性），写进 latest.json 的 `assets["macos-arm64"]`（条目本身是 App 的 zip，
  Qt 版只收带 url 的条目；`server` 是这个包）。friday-server 在包里时自己 `addLibraryPath(PlugIns)`。
  2.3–2.7：`Friday/Tools/CI/build.sh` 拿到 `FRIDAY_HELPER_STAGE` 后放进 App 的 `Contents/Helpers/`、`Contents/Frameworks/`、`Contents/PlugIns/`
  （Helpers 里只能放代码：不放 qt.conf），重签 App、随 App 公证；工作流按源码里有没有 package_macos_server.sh 选哪种。
  macOS 上 Qt 的 TLS 后端是 Secure Transport（不带 OpenSSL），只认 RSA 私钥：WebSocket 入口自动生成的证书在 macOS 上和 Windows 一样用 RSA 2048。
- 大脑调用复用 `OpenAIChatEngine`（流式、取消）；`think_in_background` 复用 `SubAgentRunner`。
- 测试：`tst_assistant`（Triage：合并、回来时批量、前台在看；ContextBuilder 的预算；Journal 分页；WorldBoard 从事件和快照校正；打断）、
  `tst_turnbrief`（`pickLine` 的各种结尾：问句、代码块、表格、客套话、只有任务状态；小模型的数字校验和超时退回）、
  `tst_remote` 加 activity 与退回旧做法、渠道适配器对假服务端。

---

## 13. 分阶段

**进度（2026-10-07）**：
- 已做（Qt core + Qt 桌面端，Windows 本机验证）：`TurnBrief` + `turnFinished` + 会话跑完的系统通知；`ActivityTap`；协议 `activity.*`、`conversation.tail`、
  `runner.submit` 的 `from` / `whenBusy`、`conversation.create` 的标题和模型、hello 的 `features`（REMOTE.md）；替用户发的消息标 `sentBy`、排到下一轮的不插话；
  `WorldBoard`、`Triage`、`Journal`、`AssistantMemory`、`TopicSummaries`、`ContextBuilder`、`AssistantTools`、`AssistantRuntime`（打断、异步工具、轮数上限、
  自动唤醒上限、合并推送、回来时批量讲、删除的确认码、只有用户明说才批准）、后台写回（话题摘要、提取记忆）；`FleetMachine`：本机、远程（`RemoteFleetMachine`，
  复用桌面端切换机器的连接）；界面：侧边栏「助手」行、助手页（时间线、回执行带打开 / 批准 / 拒绝、状态板、项目筛选、输入框和停）、设置 › 助手（含「助手记住的事」）、
  会话里「助手发送 · 来自 …」、前台在看的会话不打扰（presence）、助手的推送弹系统通知。
- 也已做：远程机器的连接池 `MachinePool`（core；切换机器和助手共用一条连接，没有界面的 friday-server 当主机时也连着所有远程）、
  看助手的协议 `assistant.*`（REMOTE.md §6「助手」）；桌面端退出时助手开着就交给后台的 friday-server（v2.7 要打开「关闭 Friday 后在后台继续运行」）——v2.8 去掉了：关窗口程序本来就在托盘里，退出就全停。
- 也已做：桌面端连远程主机上的助手——设置 › 助手 ›「助手跑在」（本机 / 机器列表里的任一台，记在 `Assistant/host.json`）；界面经
  `AssistantBackend`（本机直接包 `AssistantRuntime`，远程走那台连接上的 `assistant.*`：订阅拿全量、事件增量、往上翻 `assistant.history`、重连 / 清空后重新订阅）；
  全局一个助手：换主机时原来那边开着就关掉、到新主机上打开（主机是远程时本机的不开）；侧边栏、助手页标「助手 · 主机名」，主机没连上 / 太旧时助手页给说明和
  「重新连接」「改回本机」；状态板、回执上的「打开」把主机的机器 id 换成这台的（`local` = 主机那台，其余按名字对，对不上就在时间线上说）；presence 只报主机自己的会话。
- 也已做：换主机时搬家——选了别的主机先问「把日志、记忆和设置也搬过去」（默认勾上）：关掉原来的、整份读出（`assistant.export.*` / 本机直接读）、
  换过去、连上后写进新主机（`assistant.import.*` / 本机直接写；新主机原有的挪进 `Assistant-backups/<时间>/`，原来那边的留着不删）、再打开；进展和结果显示在设置里。
  主机写的回执里主机自己叫「本机」，在别的电脑上看换成主机的名字。
- 也已做：直达会话——以 `#短号` 开头的话（桌面端输入框的 `@` 插的也是它）原样转给那个会话、不经过大脑，会话里标「助手发送 · 来自 桌面端 · 原话」，
  这一轮算助手派的（跑完的回执按「你在等」处理）；日志里记一条「已直接发给 …」，渠道来的推回去；大脑之后的上下文里这句标着「已经原样转达」。
- 也已做：另一台电脑上的提示——问连着的远程开没开助手（`assistant.status`；不为了问去连，连接可能触发自动安装）：这台关着时助手页写「助手在 mac-mini 上跑」，给「去看那边的」「在这台打开」；两边都开着时顶上一条提醒，给「关掉那边的」「去看那边的」。
- 也已做：老服务端（2.3 之前，hello 没有 `activity`）的退回做法——会话列表看 summaries 副本、在不在跑看 runner 副本的 generating，一轮结束时订阅一下拿整段、自己算简报再退订（界面正选着那段时用界面那份）；`conversation.tail` 同样退回；开会话的标题补一次改名；旧服务端不报要确认的工具调用，助手派的按「经这里发过话」认。
- **Mac 部分已做（2026-10-07）**：Swift 的 `TurnBrief`、`ActivityTap`（规则、JSON 同 C++）、`ManagedEndpoint`（§11.3；`AgentRunner` 只加了钩子和
  `submitFrom`：替用户发的标 `sentBy`、`whenBusy: queue` 排到下一轮）、会话里「助手发送 · 来自 …」；C++ 的 macOS `--attach` 桥、`RemoteDeploy` 认出 macOS、
  `host: "swift"` 不升级不切过去；CI：`qt/ci/build_macos_helper.sh` + build.sh 放进 App 签名（Friday.build 的工作流跟着改）。
  验证：Windows 上的 Qt 助手经 SSH 在 Mac 上开会话、Swift 跑完、简报回来、助手转述；Mac 上 helper 经符号链接能跑、TLS 能握手、Release 的 App 带上 helper 签名校验通过。
- **Mac 本机当主机已做（2026-10-07）**：`AssistantHost`（helper 当助手进程：登录项、直接起、连上、推引擎 / 密钥 / 默认引擎）、C++ 的「本机」走受管端
  （`FRIDAY_ASSISTANT_LOCAL`，没开 App 时叫醒）、Swift 助手页（时间线、回执的打开 / 批准 / 拒绝、状态板、项目筛选、`@` 会话直达、停）、
  侧边栏「助手」行（状态、未读）、设置 › 助手（开关、跑在哪、登录项、名字、模型、汇报、权限、聊天工具、记住的事、清空）、推送弹系统通知、presence。
  验证：Mac 上 Swift 测试版打开助手 → 起助手进程 → 说「跑一下单测」→ 助手在 Swift 里开会话、跑完回执、转述；「打开」切到那段会话；关掉助手 → 助手进程退出。
- **v2.8 已做（2026-10-11）**：用户「助手是助手，friday-server 两者分开」——Mac 上助手的内核用 Swift 重写、搬进 Friday.app（§2.2）：`Assistant/Core`
  （运行时、状态板、分诊、日志、记忆、话题摘要、上下文、回执写作、学习惯 / 作息、管机器）、`AssistantService`（`assistant.*`，助手页进程内调、
  受管端给别的电脑）、`AssistantLoginItem`（登录项拉起 Friday 自己、藏窗口）；删掉 AssistantHost、按需装 friday-server、启动器，C++ 的
  `FRIDAY_ASSISTANT_LOCAL` / `APP`、叫醒、只读的本地能力；Mac 上的 friday-server 包只做 `--attach` 的桥。数据目录沿用，2.7 的记录接着读。
  验证：Swift 自检搬了 Qt 的 tst_assistant（879 项全过）；假模型（`Friday/Tools/Tests/mock_assistant.py`）跑通说话 → 开会话 → 回执 → 汇报、
  编短号的核对重来、记住、`#短号` 直达、重启后记录还在；经受管端 socket 订阅、说话、收推送；读一份 2.7 的真实记录（48 条，1 条「不说」的不显示）。
  渠道（同一天）：SChat、Telegram、脚本渠道搬进 `Assistant/Channels`；用假 SChat（`Friday/Tools/Tests/fake_schat.py`）跑通绑定、只听主人、
  `/status`、流式回复和收尾、引用回执直达、图片当附件、游标存盘，假 Telegram 跑通命令、草稿流式、不支持的消息、按钮回应，脚本渠道跑通收发。
  通话（同一天，用户「现在做语音 / 视频通话」）：语音 / 视频通话、P2P 文字搬进 `Assistant/Calls`，内核用同一套 C++（C 接口 + 静态库）；
  自检 994 项全过（新加的 104 项含进程里的端到端：主人打来自动接、听写交给大脑、逐句念、手机听得到、挂断记时长；`call_user` 打视频、
  手机收到光环画面、摄像头画面随话交给大脑；拒陌生人、设置不让打；P2P 文字一整套），假 SChat 的 `/ws` 跑通 hello、拒陌生人、先拿 TURN 再接、关掉通话。
  界面补齐（同一天）：SChat 扫码一键接入、换主机时搬家、别的机器上也开着助手的提示（§2.2）；自检加了两份助手之间搬家（附件大于一块也一字不差、
  新主机原有的备份）、头像、二维码（1003 项全过），假 SChat 跑通扫码接入（出码 → 扫到 → 确认 → 建机器人 → 加好友 → 传头像 → 退出登录 →
  渠道连上、主人预先绑好、打招呼），本机的 C++ friday-server 跑通别处也开着的提示和「关掉那边的」、经 WebSocket 连的远程不让搬时的说明。
- 还没做：
  搬来的旧回执里 `machineID` 还是原来主机的说法（`local` 指的是原来那台，「打开」可能找错机器）；
  （v2.8 已做：助手内核、渠道、通话搬进 Friday.app，受管端直接给 `assistant.*`，别的电脑经 SSH 看 Mac 当主机的助手，换主机能搬家，§2.2。）

- **P1 已做（SChat）**：`ChannelHub`（core，主机上跑，没有界面的 friday-server 也一样；配置 `Assistant/channels.json`，随搬家走，token 只写密钥名）、
  `ChannelAdapter` 接口、`ChannelRender`（Markdown → 纯文本、切条）、`SChatChannel`（`/bot/v1`：`getUpdates` 长轮询、游标存盘、`sendMessage` 按顺序发、
  429 按 `retryAfter` 重试、打字指示、`answerCallback`；默认明文 HTTP，开了 TLS 的自签证书按 SPKI 指纹固定——没填时第一次连上记下，之后对不上不连）；
  绑定码（6 位、30 分钟、用一次，`/bind 绑定码`）、只听主人私聊、群里不接、回复回到说话的那个私聊、回执和助手主动说的推到开了通知的渠道、
  回桌面端的话不推、引用回复带 `#短号` 的回执 = 直达那个会话；设置 › 助手 ›「聊天工具」（加 SChat 机器人、开关、服务器、token、证书指纹、
  绑定的人、出绑定码、推送开关、去掉）。
  主机是远程（包括没有界面的 friday-server）时从桌面端改渠道：`assistant.channels.*`（token 存进主机的密钥库，REMOTE.md §6「助手」）。
  SChat 全部功能（2026-10-07）：
  - 回复流式：回到渠道的回复第一个字出来时开 `streams`，之后只写完整的行（渲染成纯文本后前面的不会再变；适配器攒着、每 250 毫秒最多追加一次），
    说完收尾、补上剩下的；渲染后前面变了就收尾后改成最终的；开不了流（409）就说完整条发；被打断的按已经写的收尾。
  - 命令：`/status`（各台机器、在跑 / 等你的会话）、`/sessions`（带短号）、`/machines`、`/stop #短号`、`/new 要做的事`（交给大脑开会话）、`/help`、`/bind`，
    注册成命令面板。
  - 回执带按钮：「打开详情」（那段会话最后一条回答）、「接着做…」（之后 10 分钟里的下一句直接给那个会话）；要确认的带「批准 / 拒绝」——设置里
    开了「允许在聊天工具里批准」才有，否则写「请到 Friday 里确认」；点了转给会话、把那条消息改成「——已批准（谁）」、时间线上留痕；不是主人点的不理。
  - 助手在想时每 4 秒发一次「正在输入」（最多两分钟）；收到表情、名片这类看不懂的回一句（图片、视频、语音、文件见 §7.4「媒体」、§8）。
  - 群：在群里发 `/bind 绑定码` = 这个群收回执和通知；群里默认不接指令，开了「群里也听」只听绑定的人。
  - 搬家时渠道的 token 跟着走（导出时放进 `channel-secrets.json`，导入时存进新主机的密钥库、不落盘，备份里也没有）。
  - 一键接入：设置里填 SChat 服务器、点「扫码接入」——Friday 用 SChat 的扫码登录（`/login-sessions`，二维码 `SCSH:1:login:<token>`，自己编码成 SVG，
    `util/QrCode`）拿到用户的 token，建机器人（`/me/bots`）、和用户互为好友（`/bots/rejoin`，不用扫添加码）、
    传机器人的头像（v2.5：`/upload/avatar?userId=<机器人>`，`HaloRenderer::avatar`——深色方图上说话时的光环，JPEG；SChat 只让主人用自己的 token
    给自己的机器人传，机器人自己的 token 传不了，所以只在这一步传；传不上不挡接入。之前建的机器人要在 SChat「我的机器人」里自己换，或者重新接一次）、
    退出这次登录（不在账号里留设备）；
    机器人的 token 交给主机、扫码的人预先绑成主人，连上后打招呼。流程在桌面端跑（`SChatSetup`），主机是远程时一样。
  - 回到渠道里的话不在电脑上弹通知（`assistant.push` 的 `targets` 不空的）。
  还没做：Telegram → 微信 → 企业微信 / 飞书 / 钉钉（等用户定）、Telegram → 微信 → 企业微信 / 飞书 / 钉钉、`BriefWriter`。
- **P0 底座（桌面端文字跑通）**：`ActivityTap` + `TurnBrief`（规则挑那一句）+ `turnFinished` + 系统通知；协议 `activity.*`、`conversation.tail`、
  `submit.from` / `whenBusy`、`assistant.presence`、退回旧做法；`MachinePool`；`WorldBoard` + `Triage`（含合并后过一次、回来时批量讲）；`AssistantRuntime`（调度、打断、异步工具、轮数上限、说的 / 给看的分开）；`ContextBuilder` + `Journal` +
  `TopicSummaries` + `AssistantMemory`；界面：侧边栏助手行、时间线、回执行、状态板卡片、来源标签、`@` 会话、项目筛选、设置页、「助手记住的事」；
  换主机（`assistant.export / import`）。
  **Mac**：Friday.app 带 friday-server（CI 构建、签名、公证）、`AssistantHost`（登录项、引擎和密钥推送）、Swift 的 `ActivityTap` / `TurnBrief`、
  `ManagedEndpoint`、`--attach`（macOS 桥到受管端）、`RemoteDeploy` 认出 macOS；Swift 助手页。
- **P1 渠道**：`ChannelHub`；SChat → Telegram → 微信 → 企业微信 / 飞书 / 钉钉；绑定码、推送规则、合并、回复回执直达、在渠道里确认；
  daemon 交接条件加「绑了渠道」；`BriefWriter`（小模型写回执那一句，默认关）。
- **P2 语音**：流式识别 / 合成 provider、语音模式界面、打断；SChat 语音终端。
- **P3**：QQ、Slack、Discord、Matrix、邮件；通用 Webhook / 本机 HTTP；脚本渠道 + `create_channel`；向量检索。

每个阶段 Qt 和 Swift 的界面一起做。

---

## 14. 已定

都是用户 2026-10-07 定的：

- **回执不每条过大脑**：状态和数字由代码写，那一句按规则挑、可选小模型写（默认关）；要说人话时批量过一次大脑（§4.2、§5.1）。
- **Mac 上 Swift 本机的会话归助手管**：2.3–2.7 是 friday-server 跑内核、Swift App 兼当受管端；v2.8 起（用户 2026-10-11「助手是助手，friday-server
  两者分开」）内核用 Swift 重写、在 Friday.app 里（§2.2）。
- **主机默认本机，用户可以选**任一台远程；换主机就把助手的数据搬过去（§2.1）。
- **所有渠道共用一个助手对话**，桌面端、语音也是同一个（§7.5）。
- **大脑和写回的模型都能选，默认跟新对话一样**（§3.3）。

---

## 15. v2.5 优化（2026-10-09 起，v2.5 分支）

用户要求：优化助手的上下文、学用户的习惯、任务完成后的核对和预测；智能体（会话 Agent）不善于用工具、工作流的问题。
依据是用户本机助手两天的日志（188 条）和 Pi5 上 7 段会话（子 Agent 1820 次 run_shell、1237 次 read_file…）里实际出的错。

### 15.1 上下文（§6.1 的补充）

- **每次带日期**：这一次的输入第一行是「[现在] 2026年10月8日 星期四 11:24（Asia/Shanghai）」，人设要求日期、星期以它为准
  （之前只有「几点几分」，10 月 8 日周四两次说成「10 月 7 号星期三」，还把错的日期写进了派给会话的任务）。
- **工具调用不写成助手说的话**（v2.6 起能调工具的大脑按原样放进历史，见 §16.1）：以前写成助手说的「（我调用了 …）」，模型照着学、说给了用户（通话里也念了）。改放在数据区「[你最近调用过的工具]」，
  一行一个、带结果第一行（失败的照样给它看）。回复里漏出来的「（我调用了 …）」去掉（流式时没写完的半句先藏着）；只剩它和「（不说）」的算不说。
- **失败照实说**：人设加一条「结果以『没』开头就是没做成，照实说」（之前换模型失败了还说「已换成 A 天模型」）。换模型、开会话指定模型按名字找
  （`EngineMatch`：id → 名字原样 → 包含 → 字面相近，版本号对得上的优先；「智普5.3gla」→ 智谱 GLM · glm-5.3-flash），对不上或分不清就列出可选的。
- **工具之后不重说**：每轮请求是一段（引擎每轮结束发 Done），工具之后的那段开头 24 个字跟前面说过的差不多（相邻两字一组的 Dice ≥ 0.5）就整段不要；
  post_detail 的结果也说「正文里说过的不要再说」。段与段之间空一行。
- **摄像头画面单独说**：通话里自动附上的「摄像头 …jpg」不当附件，写「[摄像头画面]」：能看图的大脑直接附上，人设要求用户问到画面才看、才提，不评论用户的样子
  （之前每句话都 look_at 一次）。
- **隔一阵回来**：这段时间有会话变过（`WorldBoard::changedSince`，一轮结束总算变过）才先讲攒下的。
- **系统提示词只放不常变的**：机器列表去掉在线 / 离线（看状态板），前缀才能命中提示缓存。

### 15.2 做完的核对（§4 的补充）

- **派完不算完成**：主智能体派了子 Agent 就收尾这一轮（它们交回后自动再开一轮）。以前这一轮就推 finished，助手当真给用户打电话说「跑完了」，
  其实还要一个钟头。现在 `ActivityTap` 看 `AgentControl::hasPendingAgentWork`：还有子 Agent 在后台跑（或交回的结果还没送到主 Agent）就不推 finished，
  会话照旧「在跑，正在等后台的子 Agent 交回结果」，也不算卡住（以前助手三次去催它「别等子 Agent 了」）；交付那一轮才推，数字合并、用时从第一轮算。
  桌面通知同样等到那时。Swift 的 ActivityTap 同一套。协议见 REMOTE.md §5「会话的实时动静」。
- **回执那一句**：明说的结论（「结论：…」「## 结论」）优先；从后往前找时还跳过还没做的打算（「结果回来后我会验收」）、转折带出的旁话（「但…」）、「说一声」。
- **大脑核对**：简报带上最后那条回答的正文（`TurnBrief::answer`，去掉 Markdown，最多 1200 字；v2.8 起从收尾那段往前收）；状态板记下助手交给每段会话的话（`BoardEntry::request`）。
  叫醒大脑的「做完」回执后面附「交给它的：…」「它最后的回答（节选）：…」（只给大脑，回执本身不变），人设要求先对照核对——没做完、跑偏、只是在说打算就照实说，
  需要时让它接着做。

### 15.3 预测下一步

- 大脑的新工具 `suggest_next {session, text, label}`：核对完做完了的，按「关于用户」里记的习惯想到下一步（测试过了 → 提交、推送）就提，一次反应最多一个。
- 默认**等用户点头**：提议挂在那条回复上（`data.suggestion`，界面上「下一步：提交并推送（mac-mini #3f2a 跑单测）」+「好」「不用」），聊天工具里单独一条带按钮
  （纯文字的平台写「回『好』就让它接着做」）；点了「好」把那句交给会话（`assistant.suggestion`），回复上改成「已交给它」。下一次反应的输入里有
  「[等用户点头的下一步]」（30 分钟内的），用户回一句「好」时大脑用 send_to_session 交过去、那条提议算接受了。
- 设置 › 助手 › 权限「助手提议的下一步直接做，不等我点头」（`autoNextStep`，默认关）：开了就不等点头。

### 15.4 学用户的习惯

学到的都放进「助手记住的事」（能看、能改、能删），分组标种类：偏好 / 事实 / 决定（用户说的）、**习惯**、**作息**、**纠正**（学到的）。
学到的三类每次都进系统提示词（「从用户的用法里学到的」，最多约 1500 字，很少变、不影响提示缓存）；人设要求派活时把跟这件事有关的写进任务（会话看不到）。
自动写的 `source` 以 `auto:` 开头，用户改过的（source 清空）不再自动改。

- **流水**（`UsageLog`，`<助手目录>/usage.jsonl`，最多留 3000 条，随搬家走）：用户什么时候跟助手说话（talk）；会话开始的一轮（turn，`activity` 的
  `started` 2.5 起带 `request`：这一轮是哪句话引起的）——哪台、哪个项目、交给它的话、这段会话上一轮的结果和那一句、隔了几分钟接着说。
- **你说的偏好、纠正**：用户的话里有「以后…」「别…」「不对」「我说的是…」这类信号时，等他 8 秒没再说话就让写回模型提一次（`quickLearn`，
  不等攒够一批写回）；写回的记忆种类也加了 correction。
- **你怎么用会话 → 习惯和纠正**：新记了 12 轮（或隔了一天、又有 3 轮以上）就让写回模型按最近 120 轮总结（`learnHabits`）：反复出现的做法
  （「Friday 项目测试通过后，用户通常接着让它提交并推送」）、用户不满意追问的地方（「做完要自己复查验证，用户常追问『你确定吗』」）。整批换掉自动写的。
- **作息**：代码按最近 14 天说话、自己发起会话的钟头算（至少 30 条、3 天以上），找一天里最长一段基本没动静（每个钟头不超过 2%、至少连着 4 个钟头）的时间：
  「最近两周你一般在 9:00–次日 1:00 用 Friday，1:00–9:00 基本不在」。每小时最多算一次。这段时间里没人让打的电话不打（call_user 回「发消息就行」）。
  用户自己写过作息就不按统计改、不按统计拦。

### 15.5 智能体的工作流和工具（同一批，Qt + Swift）

依据是 Pi5 上 7 段会话（主智能体只派活，工具全是子 Agent 用的）。

- **简单的自己做**（设置 › 智能体 › 协作「主智能体的工作方式」新的一档，默认）：只派活时连「现在几点」都派一个子 Agent，查一次新闻用了
  8 个子 Agent、1 小时 5 分。现在主智能体多一组快工具（`ToolRegistry::isQuickTool`：read_file、list_directory、find_files、search_files、
  get_current_time、web_search、fetch_url、run_shell），一两步能查清的自己做；写文件、改文件还是派。它的 run_shell 最多等 60 秒
  （`ToolContext::maxShellSeconds`），要更久的结果开头提示派子 Agent。设置文件里的 `mainQuickTasks`（默认开）；只派活和验收仍可选。
  `teamMode` 的新值 `delegatesHeavy`（REMOTE.md）。
- **大脑歇着就换**：同一个模型已经回了「5 小时额度用完，18:10 恢复」，主智能体还往上面派了 4 次。子 Agent 因为额度用完、余额不足（QuotaError）、
  密钥不对（401 / 403）失败时，记下这个模型歇到什么时候（服务端说了恢复时间就按它，没说按 QuotaPolicy 的间隔，密钥问题 30 分钟）；之后派发选中它时
  换一个能用的（先跟随主智能体的，再按允许列表的顺序；§15.6 起先找同一档的），结果开头说「xx 额度用完，约 18:10 恢复；这次换成了 yy」。工具说明里的模型列表不变（不打断提示缓存）。
- **edit_file**：339 次失败 22 次，多是空白、缩进对不上。一字不差找不到时按行宽松地再找：先忽略行尾空白，再按内容对上、缩进差得一致才算
  （new_text 跟着换缩进）；恰好一处才改，结果里写怎么对上的。还找不到时报错里给文件里最像的那几行（带行号，相似度 ≥ 0.6）。
- **路径不对**：87 次 list_directory 有 10 次路径不存在（派活时写的路径少了一层）。read_file、list_directory、edit_file、find_files、search_files
  的「不存在」后面接上最近一层在的上级目录里有什么、工作目录下同名的在哪（最多看 5 层、5000 项，跳过隐藏和依赖、构建目录）；
  把目录当文件读、把文件当目录列时直接说用哪个工具。报错原文（和 Swift 的 Foundation 文案一致）不变。
- **等命令**：140 次 sleep 干等一共 3.6 小时（后台跑构建再固定睡几分钟看日志）。read_terminal 加 `wait_for`（一段字或正则）/ `wait_seconds`：
  等到输出里出现它、命令结束或到时间再读；工作方式加一条「直接用 run_shell 跑到结束，或 run_in_terminal + read_terminal 等，不要 sleep 干等」；
  run_shell 里 sleep 30 秒以上的结果后面提示这两个办法。
- **只读查看用专用工具**：1820 次 run_shell 约四百次只是 cat / grep / ls / find。这类命令的结果后面提示用 read_file / search_files / find_files
  （每段对话最多提两次）。
- **子 Agent 的 id 对不上**：打回、发消息、停止给错 id 时，报错后面列出现在还在跑的、交回了在等验收的（名字和 id）。

### 15.6 智能体用模型的决策（同一批，Qt + Swift）

依据是 Pi5 上的运行记录：工具说明给每个模型写的都是同一句「在线，快、按量计费」，主智能体点名时没有依据，点的多是强模型，正好撞上额度；
子 Agent 一律跟着对话的深度思考（高），读几个文件也开高；换一下模型就连新对话的默认、助手的大脑一起改了；起标题这类小请求也用对话的大模型；
主智能体自己额度用完就停下等几个小时，子 Agent 却会换；Pi5 接进来 128 个模型，里面有向量、生图、语音、安全审核这些不能对话的。用户定的（2026-10-09）：

- **换模型只改这段对话**：`AgentRunner::setEngine` 不再写 `defaultEngineID`。模型菜单底部多一项「设为新对话默认」（已经是默认时显示「已是新对话的默认」、
  不可点），设置 › 服务与密钥 › 模型里照旧能改。助手换会话的模型（`set_engine`）也不用再把默认改回去。
- **档位和计费**（`ModelTraits`）：每个模型有档位（快 / 均衡 / 强）和计费（套餐内 / 按量 / 免费）。按模型名推断：max、pro、ultra、opus、large、
  reasoner、r1 算强；flash、lite、mini、nano、small、turbo、lightning、haiku、air 这类算快；都没有时看名字里的参数量（≤ 32B 快，≥ 400B 强），
  其余均衡。Token Plan / Coding Plan 的地址（token-plan、coding）算套餐内，`:free`、名字带「免费」、本机地址算免费，其余按量。推错了在模型的「高级」里改
  （EngineConfig 的 `tier` / `billing`，没设不写）。模型菜单的副标题、服务的模型列表先写「快 · 套餐内」。
- **子智能体用的模型**（v2.8，设置 › 服务与密钥 › 子智能体可以用的服务「子智能体用的模型」，设置文件 `subAgentEngineID`）：
  跟对话用的一样（默认，空）/ 按活自动挑（`auto`，下面这条）/ 指定一个（引擎 id；删掉了、所属服务关掉了按跟对话）。不是按活挑时派发工具没有
  `engine` 参数、说明里不列可选的模型（`ModelChooser::fixedBrainNote`），主智能体点名了也不算数，`work` 只定深度思考；跟对话时不看允许列表，
  允许列表只管按活挑和歇着时换哪个。用户定（2026-10-12）：设了小活「跟对话用的一样」以为子智能体也跟，结果主智能体给盘点填了 `work: hard`，
  autodl 的 qwen3.8-27b（均衡）被换成了通义千问的 deepseek-v4-pro（强）。
- **按活选模型**（`ModelChooser`，子智能体用的模型选「按活自动挑」时）：派发工具的说明按档列出可用的模型（带计费、能不能看图）。主智能体可以用 `engine` 点名，
  也可以用 `work` 说是什么活（v2.6，delegate_task 和 run_parallel_tasks 的每一项）：hard（设计、疑难排查、审查）用强的，change（改、写、跑、装）用均衡的，
  look（只读的查看、搜索、盘点、总结）用快的。都没填就跟随主智能体：用它自己的大脑，它歇着、不支持工具时换同一档的。
  同一档里先用主智能体自己的，再按套餐内、按量、免费、允许列表的顺序；歇着的、不支持工具的不挑；这一档没有就用主智能体的，再找相邻的档
  （快 → 均衡 → 强，均衡 → 强 → 快，强 → 均衡 → 快）。点名的大脑在歇着时也先换同一档的。
  v2.5 没点名时由代码按角色说明和任务里的字眼分活，难题的字眼优先：主智能体写的自包含任务很长，几乎都带「原因」「为什么」「定位」「方案」「证明」
  （路径里的 DeliveryOptimization、network_security_config 也算），实测 .51 上 13 次派发 11 次、Pi5 上 74 次 60 次判成难题，
  不管对话选的是什么模型子智能体都用强的；v2.6 改成主智能体自己说。
- **子智能体的深度思考按活定**：每一项还有 `thinking`（off / low / medium / high）；不填时按 work：look 低、change 中、hard 高，没填 work 按中。
  都不超过这段对话设的档位（超级模式按高算，它多出来的是能往下派），按大脑支持的落地（只有开 / 关的：低按关、其余按开）。
- **小活用的模型**（设置 › 服务与密钥 › 模型「小活用的模型」，设置文件 `lightEngineID`）：自动（默认）时从子智能体能用的模型里挑一个快的（套餐内优先，
  歇着的不挑），一个快的都没有就跟对话一样；也可以选「跟对话用的一样」或指定一个。起标题先用它，起不出来再用对话的；助手的写回没单独选时用它
  （助手设置里写回的第一项改叫「跟小活用的模型一样」；Mac 上助手在 App 里，直接用 App 的这一项）。
- **额度用完时**（同一节「额度用完时」，设置文件 `quotaFallback`）：换一个能用的接着做（默认）/ 等恢复再做。换着做时记下这个模型歇到几点（子 Agent 也不派给它），
  从子智能体能用的模型里找同一档（没有找相邻的）、没歇着、支持工具的，这一轮带着图时能看图的先；换上后照「重试」接着做（做到一半的带过去，还没产出就把用户消息重发），
  交给模型的是「上一步因 xx 额度用完中断，已换成 yy 接着做」，消息上写「xx 额度用完（约 HH:mm 恢复），换成 yy 接着做」。对话选的模型不变：
  原来的歇着时 `engineConfig` 给换上的，歇完了自动用回去（到点刷新模型胶囊）；用户自己换模型就作废。找不到能换的、或设置成等，照旧停下等恢复。
- **不能对话的模型**：拉模型列表时滤掉（向量 embed、重排 rerank、检索、guard / safety 审核、reward、生图生视频 image / video / cogview / wan2.x、
  语音 tts / asr / realtime、parse 这些），列表摘要写「另有 N 个不是对话模型，没列出」。以前接进来的：模型菜单、新对话默认、小活用的模型、
  助手的大脑和写回、子智能体都不列不用，服务的模型列表里那一行标「不是对话模型，对话和智能体都不用它」，取消勾选保存就移除。

## 16. v2.6 修正（2026-10-10，用户「我让它开会话没有开成，还有不是我的声音还是能识别」）

依据是用户本机 10-10 上午那通 SChat 视频电话的助手日志：三次说「已派本机 #2103 / #d5e0」，日志里却没有一次开会话的调用（每次调工具都会记一条），
短号是编的；那通电话里「O.」「H.」「Oh.」「Yeahello.」「好，我来过2号2号。」这些也都交给了大脑。大脑是 qwen3.8-flash、不思考。

### 16.1 嘴上说派了：历史里带上工具调用 + 核对编的短号

- **原因**：v2.5 起工具调用不进历史（§15.1），历史里助手的回复读起来是「好，我找个目录开新会话…（空行）已经派本机 #bdb0 去查了」——
  像是没调工具就说了「已派」，快模型照着这个样子一口气写出「已派本机 #2103」，连短号一起编。人设里「派完马上回一句『已派给 …』」、
  通话里「能直接回答的直接说，别先调工具」也在推它。
- **历史里带上工具调用**（`ContextBuilder`，`ContextInputs::tools` = 大脑能调工具）：每次回复连同它调过的工具算一段，工具按原样（助手消息的 `tool_calls`
  + `tool` 结果，结果最多 600 字）插在调用时回复写到的位置（工具日志的 `offset`：前几段连起来的长度）；旧日志没记位置的放在回复前面，
  回复没进日志的（被作废）工具自己一段。按段从新往旧截（不会只剩 tool 结果），截掉的那几段里的工具放进数据区「[更早调用过的工具]」；
  连着两条助手消息时并成一条。调用 id 由工具日志的 id 来（每次一样，命中提示缓存）。不能调工具的大脑照旧只在数据区。
- **人设**：一定要真的调用工具，拿到结果才算派出去，短号照结果写、不要自己编；没调工具就不要说已经派了、开了。通话那条加「要派活、开会话、转达还是得调用工具」。
- **核对**（`AssistantRuntime`）：说派活的一句（派、开、转、交给、让、发给、叫、安排）里有状态板上对不上的「#短号」就是编的。
  - 流式时：还没写完的那句提到「#」先不给看，写完了核对；编了短号的那句起先不给看（不念、不发到聊天里）。
  - 一次请求写完后（主线程，`verifyReply`）：去掉那几句（记进这条回复的 `data.unverified`），大脑能调工具时接着同一次对话再请求一次：
    「（Friday 核对）你刚才说「…」，但 #2103 不在状态板上，这次也没有调用 create_session / send_to_session——其实没派出去，这句没有发给用户。
    现在要派就直接调用工具真的派…派不了就照实说没派成」。只重来一次。
  - 收尾时还有编的（重来了还编、来不及核对）照样去掉，补一句「抱歉，刚才说的 #2103 其实没派出去。」；不能调工具的大脑去掉之后也补这句。
  - 工具之后那段跟前面差不多就不要（§15.1）：提到了前面没说过的 #短号的不算（派出去的结果不能丢）。
- 测试：`TestAssistant::contextToolHistory`、`TestAssistantRuntime::fabricatedDispatch`（脚本大脑先编 #d5e0、重来真派；重来还编时补那句）、`dispatchAndReceipt`。

### 16.2 通话里也只听你的声音

- **原因**：声纹（§8 语音模式，v2.3）只接在电脑麦克风的语音模式（`AssistantVoice`）里，SChat 通话（`AssistantCalls`）断句 → 听写 → 直接 `say()`，
  只扔掉没听出字的。
- **做法**：设置里「只听我的声音」开着（`voiceMode/onlyEnrolled`，默认开）、登记过声纹、装了能算声纹的本地语音时，每句断出来写成 WAV 后，
  听写出字了再用 `Voiceprint::embed`（本地语音的 `POST /v1/audio/speaker`，一句几毫秒；不到 0.5 秒的回 400，当认不出是谁）算声纹、跟登记的比。
  - 门槛：声纹是在电脑麦克风上登记的，通话里是手机麦克风 + 压缩，自己的也会低一些：比设置的严格程度松一档（标准 → 宽松：0.35，不到 1.5 秒的 0.20）。
  - 不交、不打断（`AssistantCalls::skipHeard`）：只听出一个字母（杂音）；助手在说 / 在想时没有两个以上实在的字（同语音模式）；不像你；
    太短算不出声纹、助手又在说 / 在想。声纹拦下的记一条说明「没交给助手（不像你的声音：相似度 0.12，要到 0.35）：…」（带 `voiceprint`、`voiceprintThreshold`、`durationMs`），
    交了的那句用户消息也带上相似度：先看实际数字（手机上自己的有多高），不合适再调门槛或加「在通话里登记」。
- 测试：`TestAssistant::callsSkipHeard`；真包 `TestPowers::voiceprint` 加了整句声纹（本机：登记的人 0.99，另一个人 -0.06）。

### 16.3 声纹热更新（用户「感冒了声音变了声纹会变吗」→「声纹热更新，可行吗？安全吗？」→「按你说的做」）

- **为什么不全自动**：要安全，只能拿「很确定是你」的句子更新；感冒这种几天内的突变，相似度一下掉下来，这些句子恰恰不够格。
  所以自动更新只跟慢变化（换麦克风、换房间、嗓音慢慢变），突变要你点一下「是我说的」。
- **voiceprint.json 多两样**（Qt `Voiceprint`、Swift `Voiceprint` 同一个格式；旧文件照样读）：
  - `vector`：登记的，锚点，永远不自动改；重新登记换掉整个文件。
  - `recent {vector, updates, at}`「最近的你」：交给了助手、够长（≥ 3 秒）、跟登记的或最近的你很像（≥ 0.6）、不是助手在念的时候说的，
    往它挪一小步（10%）再归一化；挪完跟登记的不到 0.5 就不挪（`Voiceprint::adapt`）——别人、电视、助手自己的回声进不来，也不会一步步漂走。
  - `temporary [{vector, at, until, text}]`「是我说的」记下的，14 天作废、最多 5 个（`Voiceprint::remember`）；不拿来更新最近的你。
  - 比对取三者里最像的（`Enrolled::match`）。改之前都重新读文件（语音模式、通话都会写）。
- **「是我说的」**：
  - 语音模式：被拦下的那句在语音条上留 12 秒，旁边「是我说的」（`AssistantVoice::confirmOther` / Swift `VoiceMode.confirmOther`）：记成临时的，这句照样交给助手。
  - 通话：被声纹拦下的说明带上那句的声纹（`data.voiceprintVector`、`heard`），时间线上那条下面「是我说的」（`assistant.voice.confirm`，
    `AssistantRuntime::confirmVoice`）：记成临时的，说明去掉声纹、标 `voiceConfirmed`，显示「记住了：14 天内都认得这个声音」；那句不补交（多半已经过去了）。
- **设置 › 能力 › 声纹**：说明写「用 3 句话登记，10月8日 14:51；自动更新过 12 次（最近 …）；临时记住 1 个（到 10月24日）」，
  有最近的你或临时的时多一个「恢复成登记时的」（`Voiceprint::resetAdapted`）。
- 测试：`TestAssistant::voiceprintAdaptive`（门槛、一步步漂不过 0.5、临时的上限和作废、恢复、旧文件）、`TestAssistantRuntime::confirmVoice`；
  真包 `TestPowers::voiceprint`：登记的人那几句自动更新了 2 次，英文那段（另一个人）被拦下 → 「是我说的」→ 交给助手、再说一遍直接认得。

### 16.4 麦克风进输入框、按住说话（用户「将麦克风按钮放到输入框里左边，点击后隐藏输入框，按住 alt 或者 command 键持续听写，松开后关闭」）

- **麦克风**：从助手页工具栏挪到输入框按钮行的最左边（麦克风、回形针、@）；点了开语音模式，输入框收起，只留语音条和光环，点语音条的 × 结束、输入框回来。
  Qt：ComposerView 多一个 `startTool`；Swift：AssistantComposer 的按钮行。
- **按住说话**：助手页开着时，单独按住 Alt（Windows / Linux；macOS 是 ⌘）0.3 秒开语音模式、一直听；松开时不再听，正在说的这句马上定稿
  （实时听写的 `flush`）交给助手，它回完（念完）就自己关；松开前没说话，0.9 秒后关。按着时又按了别的键（Alt+Tab、⌘C、⌘⇧4 这类快捷键）
  就作废、马上关；窗口 / App 不在前台了算松开；语音模式本来就开着时按住不管（v2.6 起松开后助手还在想 / 在念时再按住是接着听，§16.6）。
  语音条上写「在听，说完松开 Alt 交给助手」「在想…（回完就关；按住 Alt 接着说）」。
  逻辑在 `AssistantVoice::setHeld / release`（Qt core）、Swift `VoiceMode.release`；按键在 Qt `VoiceMode::eventFilter`（只看发给窗口的那一次，
  按住开的那次松开 Alt 收下，免得 Windows 去激活窗口菜单）、Swift `NSEvent.addLocalMonitorForEvents`。兜底：松开 90 秒还没回完也关。
- **顺手修的**：Qt 版语音模式开始时 `AssistantVoice::start` 读声纹先发一次 changed（那时还是关着），`VoiceMode` 收到就把刚打开的麦克风、
  扬声器关了——从 10-08 加声纹（b802ad7）起 Qt 版语音模式听不到声音。现在只在语音模式「开着 → 关了」那一下关设备。
- 调试：Qt 也认 `FRIDAY_VOICE_INPUT=<16 kHz 单声道 wav>`（录音当麦克风）、`FRIDAY_VOICE_OUTPUT=null`（不出声）；截图脚本多了 `keydown:` / `keyup:`。
- 测试：真包 `TestPowers::voiceHold`（说到一半松开也交出去、回完自己关；没说话松开就关；不是按住开的 release 不管）；
  截图脚本按住 Alt → 录音 → 松开 → 假大脑回完 → 自己关、输入框回来；Alt 按着马上按 Tab 不开语音模式。

### 16.5 中英混说听写（用户「我发现他识别不清楚我说的英语」→ 选「本地加更准的模型」）

- **原因**：10-10 语音模式里说的「Secret Chat」被听成「是亏欠的」「谁可以切」「推C欠」「C亏欠」——本地语音的 SenseVoice-Small 整句以中文为主时
  按中文听，夹着的英文词按读音写成汉字（语种是自动的，不是设成了中文）。
- **本地语音 0.3**（Friday.Powers，`docs/voice-protocol.md`「Whisper 定稿」）：一句说完先照旧用 SenseVoice 判断是不是人声、认语种，
  再交给 Whisper large-v3-turbo（whisper.cpp v1.9.5，q5_0，574 MB；Windows、Mac、Jetson Orin 的包带）出定稿，两边按字 / 英文词对齐合起来：
  中文用 SenseVoice 的，英文词、以及 SenseVoice 把英文写成汉字的地方用 Whisper 的（「我说的是C亏欠」→「我说的是Secret Chat」）。
  编码窗口按句子长度算（M1 上一句从 4 秒多降到 0.7 秒），5080 上一句多花约 0.05 秒。实时字幕照旧是 SenseVoice 的。
- **热词**：`AssistantRuntime::vocabulary()`（Swift `AssistantModel.vocabulary`）——机器名、会话的项目目录名、会话标题里的英文词组，最多 30 个，
  用「、」连起来交给 Whisper（语音模式的实时听写 start 帧 `prompt`、通话的听录音请求 OpenAI 的 `prompt` 字段）：「Cloud Code」→「Claude Code」。
- **顺手修的**：本地语音连上之前攒的消息以前是先发全部文本、再发音频——按住说话在连上前就松开（冷启动时更容易，0.3 多加载 Whisper 要多一秒）时，
  「这句马上定稿」的 flush 跑到了音频前面，这句一直等不到结尾、90 秒后才关。现在按原来的顺序发（Qt `RealtimeConnection`、Swift 同名类）。
- 测试：`TestAssistantRuntime::vocabulary`；真包 `TestPowers::realVoice`（本机有 Whisper 模型和合成的中英混说录音时：
  「我说的是 Secret Chat，不是别的。」认出 Secret Chat）、`voiceHold`（0.3 的包冷启动慢，连上前就松开也能交出去）。

### 16.6 说完再交、再按住接着听、记错的会忘（用户 2026-10-10 的五个问题）

- **说完再交**（「一句话我还没说完或者在想的时候它就已经发出去了」）：实时听写按停顿切句（0.45 秒没声音就切），想一想就切开了。
  切出来的一句先攒着（`AssistantVoice::queue`，Swift `VoiceMode.queue`），再等一会儿：这期间又开口了就和下一句拼起来，等够了才交。
  等多久（`TurnEnd::waitMs`，Swift `TurnEnd`）：基准按设置 › 能力里的「说完一句」（很快交 / 标准 / 边想边说：0.3 / 0.7 / 1.6 秒，`voiceMode/patience`）；
  结尾是逗号、顿号、省略号或「然后、就是、那个、因为、所以、还有、但是…」这类后面还有话的词（英文 and / but / the…）、只有「嗯」「呃」的多等 1.5 秒，
  问句减半。按住说话时按着一直攒、松开才交。通话（`AssistantCalls::queueSay`）同样，从说完那一刻算起（听写花的时间算在等里）；

  坑：本地语音一句正常定稿时只发 `final`，不发 `speech_end`（只有判成杂音时发）——`final` 也要当作「这句结束」（`segmentEnded`），
  不然状态停在「在听这句」，攒着的永远不交。
- **再按住接着听**（「在它用大脑的时候我松开 alt，然后又按住没有反应」）：按住开的、松开后助手还在想 / 在念时，再按住 0.3 秒是 `rehold`：
  接着听，「回完就关」作废；按着的时候一直听（关了「说话时也听」也听）。
- **记错的会忘**（「助手记住的东西如果是错的，应该要遗忘」）：用户本机记下过「以后提到『C亏欠』应理解为 C 盘」——其实是语音把 Secret Chat 听错了。
  记忆带短编号（`MemoryItem::shortId`、`AssistantMemory::find`），人设里要求：用户说不对、改主意了、情况变了，或者当初是听错理解错才记下的，
  用 `remember` 填 `replaces` 改掉、没有对的说法就 `forget` 按编号删；快学（`quickLearn`）和写回的提示词也让模型把对不上的放进 `forget`、
  有对的说法用 `replaces`（`AssistantRuntime::applyMemoryChanges`），忘了、改了的在时间线记一条说明；「忘掉」「记错」「听错」「其实是」也触发快学。
- **会话在处理时往上翻会抖**：消息列表 / 助手时间线改成按锚点排版（`AnchoredList` + `AnchoredColumn`，STYLE.md「消息列表按锚点排版」）。
- **能力页下载时抖**：设置页的每一块按 `Powers.powerIds` 建（只在多了、少了时重建），原来绑 `Powers.powers`，每涨 0.5% 进度整页销毁重建；
  下载中的状态只占一行。
- 测试：`TestAssistant::turnEnd`、`memory`（编号）、`TestAssistantRuntime::forgetWrongMemory`、`quickLearnForgets`；真包 `TestPowers::voiceTurns`
  （按住说两句松开才拼成一句交、松开后再按住不关、停一下接着说拼成一句）；`voiceprint` 改成数中文句数（连着说的几句会拼成一条）。

### 16.7 会话页按住 Alt 听写进输入框（用户「我在别的会话按 alt 的时候，助手还是会触发语音」「每个会话都要增加语音按钮，按住 alt 实时将内容显示在输入框里，按住 alt 的时候语音图标换成现在助手显示的大图标放到语音图标位置」）

- **原因**：Qt 助手页用 `Binding { target: VoiceMode; property: "pushToTalk" }` 打开按住说话，离开助手页时页面卸载，Binding 跟着销毁却不会把值改回去，
  `pushToTalk` 一直是真——之后在哪个会话按住 Alt 都开助手的语音模式。现在助手页自己写（`onPushToTalkChanged`）、销毁时清掉；
  它只看 `Assistant.available && enabled`（不读 VoiceMode：写 pushToTalk 会发 VoiceMode 的 changed，读了就是绑定循环），装没装本地语音由 VoiceMode 按键时看。
  Swift 版一直是 `onAppear` / `onDisappear`，没有这个问题。
- **按住 Alt 按页面分派**（Qt `VoiceMode::eventFilter`、Swift `VoiceMode.handleKey`）：助手页开着（`pushToTalk`）是语音模式（§16.4）；
  会话页开着时是听写进那段会话的输入框（`dictationTarget`：Qt 是那页的 ComposerView，弱引用，输入框销毁时自动清掉；Swift 是会话 id，`onAppear` / `onDisappear` 设）。
  同样单独按住 0.3 秒才开、按着又按了别的键作废、窗口不在前台算松开；松开后最后一句还在定稿时又按住是接着听（`Dictation::resume`）。
- **听写**（core `powers/Dictation`、Swift `Powers/Dictation.swift`）：只用本地语音的实时听写（不念、不交给助手、不按声纹过滤），热词同语音模式。
  `text` 是定稿的句子拼起来（`TurnEnd::join`）+ 正在说的这句；说完了（点停、松开）`flush` 这句，定稿到了再等够 0.9 秒就关，最多 6 秒。
  麦克风只有一个：听写、语音模式、登记声纹开一个先关别的（`VoiceMode`）。
- **输入框**（Qt `ComposerView`、Swift `ComposerView`）：会话页按钮行最左边多一个麦克风，点了听写、再点停（助手页的麦克风照旧是语音模式）。
  听到的实时插在光标处（替换选中的；输入框没聚焦时接在末尾），光标跟在写进去的字后面；接在英文、数字后面的英文前空一格。
  听写时麦克风换成助手语音模式的那个光环（HaloView，60 × 60 叠在按钮的位置、压在旁边的按钮上面，按钮本身只是透明、占着位置不让旁边的挪）：
  连本地语音、等最后一句定稿时转圈，在听时跟着麦克风的音量起伏；点光环停。空输入框的占位字写「在听，说完松开 Alt」/「在听，说完点光环停下」。
  听写时不弹悬停提示（用户「说话的时候 tooltip 挡住了输入框」：输入框在窗口底边，提示放不下翻到上面盖住正在写的字）；听写开始时鼠标正停在按钮上的，麦克风的提示等鼠标移开过再恢复。
  听写中自己打字、发送（输入框的字和最后写进去的对不上）就停下听写，免得下一句盖掉；换了会话就停（写进去的照旧在草稿里）。
  Swift 的光标位置经 `TextCaret`（GrowingTextView 的协调器记着 NSTextView；外面换掉整段文字时按它放光标，用一次就清）。
- **顺手修的**：语音模式按住说话「松开后关掉」的检查以前靠一个正好 0.9 秒的定时器，到时再比一次「松开多久了」——Qt 的粗定时器可能早几毫秒到，
  差一点就再没人管，松开前没说话时要等 90 秒兜底才关（Qt `AssistantVoice::checkReleased`；Swift 连上前就松开时同样）。现在没等够就按剩下的时间再约一次；听写的 `checkFinished` 同样。
- 调试：Swift 钩子 `hold[:off]`（当作按下 / 松开 ⌘）、`dictate[:off]`、`dictatewait:<状态>|heard:<字>`、`dictatestate`（连输入框里实际的字和光标）。
- 测试：真包 `TestPowers::dictation`（边说边出字、finish 后一会儿就关、连上前就说完照样定稿、resume 接着听）；
  Qt 截图脚本：助手页 pushToTalk 为真 → 回会话页为假、按住 Alt 听写进输入框（插在光标处）、Alt+Tab 作废、点麦克风 / 光环开停、打字停下听写、助手页按住 Alt 仍是语音模式；
  Mac 上 v26u Debug 版（测试用的本地语音）按住 ⌘ / 点麦克风写进输入框、光标在写进去的字后面。

### 16.8 助手的音色和语气（用户「能力里为什么没有音色选择，还有助手应该用设置的音色，还是根据内容选择合适的音色」→「按这个方案做，Qt 和 Swift 都改」）

- **为什么以前没得选**：本地语音（friday-voice）协议一直支持多个音色（`ready.voices`、`start.voice`）和 CosyVoice3 的指令（`start.instruction`），
  但包里只有 `voices/default.gguf` 一个；语音模式、通话边写边读启动时都不传音色；通话逐句合成、语音消息的回复走能力层，用在线服务设置里的默认音色。
- **定的原则**：助手自己说话用设置里固定的音色（同一个人，换声音像换了个人）；按内容只调语气、不换音色。生成音频的任务（`speak` 工具）照旧由智能体按内容挑音色。
- **音色**（friday-voice 0.4）：新加四个，从 AISHELL-3（Apache-2.0）的录音提取：`young-male` 青年男声、`calm-male` 沉稳男声（41 岁以上那位）、
  `gentle-female` 温柔女声（念得慢、平稳）、`lively-female` 活泼女声（快、起伏大），加上原来的 `default` 默认女声。挑法：每类两三个候选，
  合成同一句测试文字后转写回来核对（字错率基本为 0）、算和参考录音的声纹相似度（0.82–0.92）、彼此的相似度（≤ 0.58），按念同一句的时长看快慢；
  参考录音控制在 5–11 秒（太长每次合成都要多处理，慢设备上第一声更晚）。做法、来源见 Friday.Powers 的 `voice/assets/voices/README.md`、`THIRD_PARTY_NOTICES.md`。
- **设置**（`AssistantConfig::voice` / `voiceTone`，设置 › 助手「声音」：音色、试听、按内容调语气，默认开）：列表是内置目录（`AssistantSpeech::catalog`，Qt / Swift 一份）
  和本地语音包里实际有的（`PowerManager::voices`，`powersVariant` 的 `voices`，远程的经 RemotePowers 原样转过来）的交集；选的音色包里没有（还没更新到 0.4）时说一句、先用默认的念。
  试听：本机的本地语音（`provider: power-voice`）念一句，Qt 用 QML 的 MediaPlayer 放、Swift 用 AVAudioPlayer 放。
- **用在哪**：语音模式（Qt `AssistantVoice`、Swift `VoiceMode`）启动边写边读时带 `voice`；通话边写边读同样；通话逐句合成、语音消息的回复走能力层时
  加 `localVoice`（只有本地语音的 `PowerBackend` 认，智能体没点名音色时换成 `voice`；在线服务不看它、用自己设置里的音色）。
- **语气**：用户在用语音（语音模式、通话、聊天里发语音消息）且开着「按内容调语气」时，每轮提示词里「用户在用语音」那句后面加 `AssistantSpeech::toneHint()`：
  开头可以写「【语气：轻快】」（轻快 / 温和 / 郑重 / 平缓，同义的也认）。runtime 在流式写回复时看开头（`takeTone`）：像是没写完的标记先不给看、不念；
  写完了从原文里去掉、语气记进 `entry.data.tone`（Swift 的 Entry.tone 读它）。念的时候换成 CosyVoice3 的指令：边写边读每条回复开始前
  `RealtimeSpeaker::update` 再发一次 `start`（friday-voice 0.4 起每段字进队列时就记下当时的配置，换语气不影响还没念完的上一条）；能力层的加 `style`（OpenAI 兼容、豆包认，百炼忽略）。
- **实测**（RTX 5080，CosyVoice3 Q8）：带指令念得照样准、耗时不变；声音和不带指令时相比会有点变（同一个人一般 0.7 以上）。四个音色 × 两句比了几种写法，跟不带指令的同一句的声纹相似度平均都在 0.75–0.91，挑最不变声的：轻快「语气轻松一些。」（0.87，「开心、轻快」0.83）、温和「请用温柔、关切的语气说。」（0.85）、郑重「请用认真、稳重的语气说。」（0.82，「严肃、郑重」0.80）、平缓「请用平缓、略带歉意的语气说。」（0.84）。带指令时语速略快。
- 测试：Qt `TestAssistantRuntime::speechVoicesAndTones`（目录、指令、标记解析、能力层选项、设置读写）、`voiceReplyTone`（提示词、流式时不露出半截标记、tone 记进条目、
  「【」开头的别的内容不吞、打字的和关了的不提示）；Swift 自检 models() 里的音色目录和指令；friday-voice 0.4 真服务：一个连接里换语气、五个音色都在 `ready.voices`。

## 17. v2.8 修正（2026-10-11，.51 上 changji 会话的问题，用户「是模型问题还是你设计有问题」→「先修改你自己的问题」）

.51 上助手派给本机会话的 changji 出片任务出了一串事：5 小时的一轮做完后助手打电话说「成片合格」（其实视频没生成，会话的交接报告写得清楚）、
早上 7 点打电话（用户说过 0–10 点别打）、「先停下」转过去要等几小时、会话把「这次优先试 Wan」当成用户拍板删了 26 GB 权重。
查下来四处是设计的问题（模型那边——大脑编造、谎报「已停下」、替用户做决定——之后再说）：

- **回执截头不截尾**（§4.2）：`TurnBrief::answer` 以前取整条回答的前 1200 字，长回合的开头是一路的进度播报；改成从收尾那段往前收。
  「那一句」认标题里含「结论」的节和写得够实的冒号引子；最后一段在等用户拍板 / 确认时状态是「在问你」。
- **主会话一轮跑几百步**：「简单的自己做」的主 Agent 自己只能跑 60 秒内的命令，大脑又让它别派子 Agent（以为子 Agent 的步骤看不见），
  它只好每分钟 `Start-Sleep 57` 轮询一次，一轮 316 步、5 小时、上下文涨到 23.5 万、累计输入 5379 万 token。
  - 主 Agent 一轮里也折叠（`OpenAIChatEngine::folding` + `foldFromHere()`，同子 Agent 的 `ContextFolding::session` 规则和预算）：只折这一轮自己的步骤，
    之前几轮照旧归 `ContextFolding`；折了时消息上提示一次「这一轮较长，已把这一轮较早的工具输出折叠成摘要」。
  - `run_shell` 的干等提示认 PowerShell 的 `Start-Sleep`（`-Seconds` / `-Milliseconds`）、cmd 的 `timeout /t`；「简单的自己做」的主 Agent 提示派子 Agent 去跑和等。
  - 主 Agent 的工作方式、大脑的人设里写明：子 Agent 每一步的工具调用都实时显示在派发它的卡片里，用户看得到，别为了「看得见」不派。
  - 转给正在忙的会话、没插话（`send_to_session`）：工具结果说清它这一轮已经跑了几分钟（状态板 `BoardEntry::runningSince`，收到「开始」时记）、
    这句话要等这一轮做完才看得到、要马上看到带 `interject`、要它停下用 `manage_session stop`；工具说明改成用户在纠正、追问、叫停正在做的事时用 `interject`。
- **用户明说的规矩只是一条记忆**：「每天 0–10 点别打电话」以前只用 `remember` 记下，打电话只按统计的作息拦。新加设置「免打扰」
  （`AssistantConfig::quietStart` / `quietEnd`，每天 [开始, 结束) 点，可跨午夜，相等 = 不设，默认不设；设置 › 助手 › 聊天工具「免打扰」：开关 + 从几点到几点，打开时先给 22–8）
  和大脑工具 `set_quiet_hours {start, end}`（用户说某段时间别打电话时用，相等是取消）。这段时间里没人让打的 `call_user` 回「现在是用户设的免打扰时段」，
  放在「这里打不了电话」前面；用户这次让打的照打。每次的「[现在]」后面写着「（免打扰：每天 0:00–10:00 不主动打电话）」。
  （.51 上那条记忆不会自己变成设置：更新后在设置里设一次，或跟助手再说一次。）
- **派活时分不清谁说的**：会话收到助手转达的话（`sentBy` 有值、不是「· 原话」直达的）时，交给模型的正文前面加 `AgentRunner::relayNote`：
  「」里是用户原话，其余是助手的理解、拆解和建议，不是用户的决定，和原话冲突以原话为准，助手建议的取舍用户没点过头不能当成已经拍板；
  大脑的人设和 `create_session` / `send_to_session` 的说明要求原话用「」引出、自己的补充写明是建议。任务状态的工作方式加一条：
  换了一件新事先换目标、删掉只对上一件事有效的约束（「先暂停」「等用户定方案」），一直有效的要求留着。

没改的：「完全」权限下删除不问（用户选的 full 的含义），要不要加「完全权限下删大文件 / 用户下载的数据仍然先问」等用户定。

**.51 实测**（2026-10-11，隔离的测试实例 `C:\Users\ultra\friday-v28-test`，大脑、会话、子智能体都用 glm-5.3-flash）：
- 说「每天0-10点不要打电话给我」→ 大脑调 `set_quiet_hours {0, 10}`（不是 remember），设置里写进去了。
- 派「写个 3 分钟的后台脚本、等它跑完报行数」→ 大脑把原话放进「」、拆解标成「建议」；会话自己派子 Agent 去等（没有一分钟一分钟轮询）；
  回执正文是会话收尾的报告，大脑照实转述「9 行、间隔正好 20 秒」。
- 会话等子 Agent 时说「让它先停一下」→ 大脑用了 `manage_session stop`；子 Agent 等确认时大脑没替用户批（用户刚说停），用户明说后才批。
- 「列两个整理方案、等我选」→ 会话收尾写「我等你选，选之前不动手」：动词表原来只有「选择 / 选定」没认出来，补上单字「选」，重跑后是「在问你」。
- 「read_file 分 20 次读完 big.txt」→ 一轮 22 步时折叠了一次（消息上有提示），这一轮最后上下文 8.6 万（不折约 14 万），读出的三行内容照样对。
- 实测发现：`manage_session stop`（和界面的停止一样，v1.15 起）只停主会话这一轮，**不停它派出去的后台子 Agent**，
  主会话一轮已经结束、只在等子 Agent 时 `AgentRunner::stop` 什么也不做，工具却回「已让它停下」。.51 上那次「先停下」也是这样。

**停下连子 Agent 一起停**（用户定：「连子 Agent 一起停」，full 权限删除不问不变）：助手的 `manage_session stop` 和聊天工具里的 `/stop`
（`FleetMachine::stop`）改成 `AgentRunner::stopWithAgents`——停这一轮，再把这段对话的子 Agent（含独立的后台子 Agent）全部停掉、
标成 quiet（`AgentTeamState::stopQuietly`）：卡片照样标「已停止」，但结果**不再新开一轮交回主 Agent**（不然主会话会被叫醒、按「已有信息继续」接着干）。
远程是 `runner.stop {agents: true}`（REMOTE.md；旧服务端不认，照旧只停这一轮）。界面的停止键不变，仍只停这一轮（v1.15）。工具结果写明「连它派出去的后台子 Agent 一起停了」。

**聊天工具里的确认卡片**（用户 2026-10-11「发我确认的信息并没有触发 schat 确认卡片」）：会话要确认时大脑会被叫醒，推到聊天工具的是它说的那句
（「…在等你确认，批不批？」），只有叫不醒大脑时才推带「批准 / 拒绝」按钮的模板回执——所以收到的确认消息没有按钮。现在 `ChannelHub::deliver`
对大脑的话（notice / reply）也找按钮：话里说到的会话（#短号）正等着确认、只有这一段在等、开了「在聊天工具里批准」，就在这条上挂「批准 / 拒绝」
（`mentionedApproval`，点了走原来的 `ok:` / `no:`）；流式写的回复挂不了按钮，写完另发一张和回执一样的确认卡片（`deliverApprovalCard`）。
交给大脑的确认事件后面提示它说到时带上短号。测试 `TestAssistantChannels::brainApprovalButtons`、`TestRunner::stopWithAgents`。

测试：Qt `TestTurnBrief::answerFromTheEnd` / `conclusionHeadings` / `waitingForUser`、`TestRunner::foldWithinTurn`（假接口连调 18 次 read_file：
后面的请求里这一轮较早的结果折成摘要、上一轮原样、只提示一次）、`TestRunner::interjectedPromptText`（转达说明）、`TestTools`（Start-Sleep / timeout /t、主 Agent 的提示）、
`TestAssistant::userQuietHours` / `relayToBusySession`；Swift 自检同样的回执、提示用例。

## 18. v2.8 模型评测后的修正（2026-10-12，用户「现在说说模型的问题怎么改」→ 本机所有模型实测 →「清理吧，然后把那几个问题都改了推送」）

评测（新版本上，本机全部在线模型当大脑 6 个场景两轮、当会话 3 个任务，外加本地 Qwen3.5-9B、Qwen3.6-27B、Qwen3.6-35B-A3B）：
qwen3.8-max / qwen3.7-max 两轮全过、回话中位 4 秒（和 qwen3.8-flash 一样快）；qwen3.8-flash 三次都编（空「」、编附件路径、编「#删文件」和工作目录）；
多数会话模型删除被拒后换 Move-Item / Rename-Item / rmdir 绕过去；有子 Agent 没带 --prefix 跑 cmake --install 装进了 C:\Program Files。
本地 9B 不开思考时嘴上说设好了免打扰、开了会话，其实没调工具。照这些改（Qt + Swift）：

- **拒绝提示**：工具被拒的回灌从「换一种不需要这个操作的做法」改成「不要再发起，也不要换别的命令或工具达到同样的效果（挪走、改名、覆盖、清空、
  换一种删法都算）；不需要这一步也能做完就照做，做不了就停下来问」。工作方式里那句同改。
- **子 Agent 不动用户原有的东西**：守则加一条——不是这次做出来的（旧的编译产物、模型和数据、配置）不删、不清空、不覆盖、不挪走，除非任务写明是用户原话要求的；
  要干净重来就换新目录。（Qwen3.6-27B 当会话时主会话让子 Agent「删除 build 下的所有内容」，那是唯一能跑的旧引擎。）
- **往系统目录装软件、写文件、改系统设置要先问**：设置 › Agent「这些事停下来交给我」加一项（默认勾上、能取消），勾着时自动模式下
  `AgentRunner::isSystemChange` 认出来的调用也要确认：没带 --prefix 的 cmake --install、make / ninja install、包管理器、sudo、npm -g、Install-Module，
  往 Program Files / Windows / ProgramData / /usr / /etc / /Applications / /Library 写东西（只读不算），注册表 HKLM、setx /M、服务、计划任务、执行策略、
  systemctl、launchctl；write_file / edit_file 写到系统目录。完全权限照旧不问。
- **大脑回复核对**（v2.6 只认派活句里的十六进制短号）：`unverifiedClaims` 再认——说到会话 / 短号、说创建 / 新建的句子里状态板上没有的短号
  （「会话短号：#a1b2c」单独一句、不是十六进制的「#删文件」）；说开了会话、这次却没调 create_session；说设好了免打扰、这次却没调 set_quiet_hours、
  设置里也不是这个时段；说到的路径这台上没有（上一级也没有）、这次的上下文和工具参数结果里也没出现过（路径里能有空格、中文，后面直接跟中文也认）。
  这些句子不发给用户，告诉大脑哪样对不上、让它重来一次；重来还对不上照实补一句（「抱歉，刚才说设好了免打扰，其实没设」）。
- **派活的附件**：这台上没有的文件以前悄悄丢掉、会话照样开；现在拒绝这次调用、说清楚（qwen3.8-flash 编了 Desktop/年度报告.docx）。
- **回执的「在问你」**：再认「你定了哪个（…），我再动手」「你说哪个，我就做哪个」「你选 1、2 还是 3，我就照做」「你回一句…我就执行」「请告诉我您选择哪个方案」。
- **打字和回执单独选模型**（用户定「打字和处理回执用强模型、语音保持快的」）：设置 › 助手 › 模型「打字和回执」（`textEngineID`，没选同大脑）。
  有语音（语音模式、通话、听写来的）的反应用大脑，打字、回执、到点提醒用它（`reactionEngine`）。
- **Kimi k2.6 / k2.7-code**：「invalid temperature: only 1 is allowed for this model」认成不接受的参数，去掉 temperature 重发。
- **模型下载断了自动续传**：网络错误、5xx / 429 时等 2–30 秒从 .part 接着下，最多再试 5 次（以前 17 GB 下到 94% 报 HTTP/2 protocol error 就停住）。
- **friday-llm 0.1.2**（Friday.Powers）：MoE 模型的上下文下限 128K（专家挪到内存里换上下文），稠密模型仍 32K。
  实测 Qwen3.6-35B-A3B IQ4_XS 从 32K 到 128K，生成 46 tok/s。

测试：Qt `TestAssistantRuntime::fabricatedClaims` / `textEngine` / `attachments`、`TestTurnBrief::waitingForUser`、`TestRunner::isDestructive`（系统改动）、
`TestRunner::toolResultText`、`TestAgentRegistry::subAgentPreamble` / `workingMethod`、`TestEngines::unsupportedParameterDropped`；Swift 自检同样的用例。

