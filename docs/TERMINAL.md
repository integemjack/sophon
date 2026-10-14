# 内置终端与卡片停靠（v2.2）

用户要求（2026-10-06）：对话旁边能开终端；终端和「后台任务」一样是嵌在窗口里的卡片，任务列表开着时终端放在它下面；
卡片都能拖，拖到除侧边栏以外的任何地方；布局每段会话各一份，默认都不打开。Agent 能读终端、能在终端里跑一直运行的命令。
Qt（Windows / Linux）和 Swift（macOS）两端一样；看远程机器时终端开在那台机器上（协议见 REMOTE.md §5、§6「终端」）。

## 1. 用户看到的

- 工具栏的「终端」按钮（Ctrl+` / ⌃`）开关这段对话的终端卡片。卡片第一次打开、这段对话还没有终端时自动开一个。
  v2.3：卡片打开时先画骨架（一道高光扫过），选中的终端屏幕上出现字（shell 的提示符、远程终端重放的输出）才露出来，最多等 3 秒（STYLE.md §9）。
- 卡片顶上一排标签：每个是一个 shell（标题依次取：Agent 开的命令 > 程序设的标题 > shell 名；Windows 的「C:\…\pwsh.exe」缩成「pwsh」），
  悬停或选中时出现「×」（关掉，结束 shell），中键也能关；「+」再开一个。右边最大化 / 还原、收起（只收起卡片，shell 照常跑）。
  选中的标签每段对话记一份（没记过的选最新的）；标签放不下时横着滚，选中的那个总露出来。
  **Agent 用 run_in_terminal 开的终端一出现就选中它**：卡片开着时切过去，收着的话自动打开后停在它上面——连着同一台机器的几个客户端
  都这样（用户要求 2026-10-07：一个客户端开着终端卡片时 Agent 开了终端，另一个客户端的卡片自动打开后停在原来那个 shell 上，看不到命令）。
- 终端在这段对话的工作目录里开（没选目录时：本机用设置里的默认目录，远程用那台机器自己的默认目录），用户的默认 shell：
  Windows 是 PowerShell 7（没有就 Windows PowerShell、再没有就 cmd），Linux / macOS 是 `$SHELL` 的登录 shell（读 profile）。
- 切走对话、收起卡片、换机器，shell 都照常跑；关掉标签或退出 Friday 才结束（连同它起的子进程：Windows 用作业对象，Unix 杀进程组）。
  shell 自己退出了标签留着，末尾写一行「[进程已退出，代码 N]」，还能看、Agent 还能读。会话删掉了它的终端跟着结束。
- 终端里：复制粘贴（Ctrl+Shift+C / V；Windows 上 Ctrl+V 也粘贴；有选中时 Ctrl+C 是复制）、拖选 / 双击选词 / 三击选行、右键菜单、
  滚轮和 Shift+PageUp / PageDown 翻滚动缓冲（5000 行）、中文输入法、vim / htop 这类全屏程序（开了鼠标模式时鼠标交给程序，按住 Shift 仍是选择）。
  配色：深色纯黑底（同窗口）、浅色窗口底色，16 色按深浅色各一套（VS Code 终端的配色）。

## 2. 卡片停靠

卡片：后台任务（`tasks`）、终端（`terminal`），以后的面板照同样接。

- **四个停靠边** left / right / top / bottom：左右两边是整列（工具栏下沿到窗口底），上下两边夹在左右两列之间。
  **每边分几组**（用户要求 2026-10-07）：左右两边一组是一列（几列并排），上下两边一组是一行（几行叠着）；组里的卡片沿着这条边排
  （左右两边上下叠，上下两边左右排）。各组、组里的卡片都按比例（weight）分，中间空 5。每张卡四周空 5、1px 分隔线色描边、圆角。
  对话区按四边让位（开关、挪卡片时过渡）。卡片打开时直接出现在停靠的位置（用户要求 2026-10-07：Qt 原来从左上角滑过去），
  已经开着的卡片给它让位、挪动、改大小时才过渡（Qt 的 PanelDock：卡片的 placed 在打开的那一下处理完才变 true）。
- **拖动改大小**：边的内侧（贴着对话区那条边框）改这一边的宽（左右，最窄 260，默认就是最窄；Swift 最宽 720、Qt 900）或高
  （上下，最矮 140，默认 220；Swift 最高 640、Qt 700）；相邻两组之间、同一组两张卡片之间改比例（两边都至少 80）。拖动中跟手，松手才存。
  对话区至少留 360 × 200：放不下时左右（上下）两边按比例收窄，只影响画出来的，存着的大小不变（窗口放大了就回来）；
  Swift 的对话区也不向窗口要最小尺寸（不然卡片占得多时窗口会被撑大）。
- **拖着挪**：按住卡片标题栏的空白处拖。落点：
  - 另一张卡片上：看离它哪条边最近——沿着这一边的方向（左右两边是上 / 下边，上下两边是左 / 右边）就和它排进同一组（它前 / 后）；
    横着的方向（左右两边是左 / 右边，上下两边是上 / 下边）就在它旁边**新开一组**（成为两列 / 两行；这一边加宽 / 加高一组的默认大小，
    新组和别的组一样宽）。
  - 对话区靠左 / 右（整个停靠区的 20%）/ 上 / 下（22%）的那一块：停到那一边（接在最后一组末尾；那一边没卡片时新开一组）。
  - 对话区正中、停靠区外（侧边栏、工具栏）不算。
  - **拖动中**（用户要求 2026-10-07）：被拖的卡片**保持拖动开始时的大小、跟着鼠标走**（按住的那一点不变；最上层、不透明度 0.85、
    强调色 2px 边框），松手才挪，挪的时候从鼠标那儿过渡到落点。
    - 排进某一组（和另一张卡片排在一起、停到有卡片的一边）：**只把那个位置腾出来**——按落点预演挪完的样子（`PanelLayout.preview`，不存），
      别的卡片让开，空位画强调色框（8% 底、70% 描边）。预演时各边宽度冻结在挪之前的（原来那一组空了也不收窄），鼠标底下的布局不跟着动。
    - 新开一组（成为两列 / 两行、停到没卡片的一边）：**布局不动、不撑开**（撑开的话鼠标底下的卡片跟着移位，影响拖），只在要插进去的
      地方亮一根悬浮的强调色竖条（左右两边，4 宽、跨整列的高；上下两边是横条），在两组之间的空隙正中，没卡片的边贴着停靠区那条边。
    - 拖到不算的地方：空框标在原来的位置，松手回去。落点按画出来的空位和别的卡片判断（跟着鼠标的卡片不算）：停在被拖卡片的空位上——
      预演里那是落点，不变；否则那是原来的位置，算「不算」。
    落点的下标按挪之前的布局给，`move` 拿掉卡片以后自己修正（同一组往后挪减一，前面的组空了去掉减一）。
  - 卡片挪走后原来那一组空了就去掉，这一边按那一组占的比例收窄（只剩它一组时宽度留着）。
- **最大化**：撑满整个停靠区（盖住对话区和别的卡片），不存；收起卡片、换会话时还原。
- **布局每段会话一份**（`PanelLayout`），默认都不打开。新打开的卡片回到上次关它时的那一边，没有记录时放右边——后台任务排在最上，
  终端接在下面（后台任务进第一组最上，其余接在最后一组末尾）。存在设置里的 `panelLayouts`
  （JSON：会话 id → `{docks: {left|right|top|bottom: {size, groups: [{weight, cards: [{id, weight}]}]}}, last: {卡片: 边}, t}`；
  2.2.0 的 `{size, cards}` 读进来当一组），只留最近用过的 300 段；Qt 是 settings.ini，Swift 是 UserDefaults，格式一样。
- **自动打开**：后台任务——最新一轮出现第 3 层子 Agent 还在跑时（这个视图里一次）、「在后台任务里看」某个子 Agent 时；
  终端——这段对话多了终端（Agent 用 run_in_terminal 开的）而终端卡片关着时。

代码：Qt `app/viewmodels/PanelLayout`、`qml/dock/PanelDock.qml`（位置按停靠区尺寸算，卡片内容由 ChatView 给组件）、
`qml/dock/CardDragArea.qml`（标题栏的拖动区）、`style/SplitDivider.qml`（`vertical` 横着的拖动区）；
Swift `Views/PanelDock.swift`（`DockState`：四边的值，`moving` 算挪完的样子；`PanelLayout`、`PanelDock`：四边是对话区的安全区，
卡片画在 overlay 里；标题栏用 `.dockDraggable()`）。Swift 的消息列表忽略左右安全区铺到卡片下面，内容按 `panelInsets` 让开——
左边要连侧边栏（macOS 26 浮在对话区左边，占的是外层的安全区，PanelDock 量出来加上）一起让，不然内容偏左。
卡片内容要有：最大化开关、收起，标题栏接上拖动（Qt：`maximized` 属性，`maximizeToggled` / `closeRequested` / `moveStarted` / `moveDragged` / `moveEnded` 信号）。

## 3. 终端核心

**Qt**（`core/terminal/`，桌面端和 friday-server 共用）：

- `TerminalEmulator`：libvterm（`vendor/libvterm`，0.3.3，MIT）的薄包装。喂 shell 的输出，维护屏幕格子（`TerminalCell`：字符、宽度、
  属性、前景 / 背景——0 默认、调色板序号、真彩色）、光标、标题（OSC 0 / 2）、备用屏幕、鼠标模式、滚动缓冲（自己存，5000 行，
  宽字符右半边宽 0）；按键、粘贴（括号粘贴模式自动加标记）、鼠标编码成要写给 shell 的字节（`onOutput`）。`plainText(n)` 给 Agent 读。
- `Pty`：Windows 用 ConPTY（按名字取 `CreatePseudoConsole` 等，老系统报错不崩；标准句柄一律用伪终端；作业对象 KILL_ON_JOB_CLOSE；
  读线程 + 等进程线程，退出时先关伪终端、读完输出再发 `finished`），Linux / macOS 用 `forkpty`（子进程里只调异步信号安全的函数，
  QSocketNotifier 读、`waitpid` 收尸；结束时整个进程组先 SIGHUP、一秒后 SIGKILL）。Linux 链 libutil。
- `Terminal`（接口：id、会话、命令、标题、在不在跑、模拟器；`write` / `resize` / `close`）和 `TerminalControl`（一台机器的全部终端，按会话分）。
  本机是 `LocalTerminal` / `TerminalManager`（FridayCore 持有，`terminals`），远程是 `RemoteTerminal` / `RemoteTerminals`（RemoteSession 持有）；
  界面从 `AppBackend::terminals()` 取当前机器的那个。`LocalTerminal` 留最近 512 KB 原始输出，远程客户端接上时重放。
- 环境：`TERM=xterm-256color`、`COLORTERM=truecolor`、`TERM_PROGRAM=Friday`，Unix 没有区域设置时补 `LANG=C.UTF-8`。

**Swift**（`Terminal/`）：SwiftTerm 1.19.0（MIT）。本机每个标签一个 `LocalProcessTerminalView`（伪终端、解析、画都是它的），
远程每个标签一个 `TerminalView`（喂服务端转来的输出，打的字经 `term.write` 交过去）；视图一直留着，卡片显示时嵌进去（`TerminalHost`）。
`TerminalStore`（本机一个、每台远程一个）、`TerminalTab`。深浅色切换时重新套配色（SwiftTerm 设颜色时按当时外观换算成固定色）。

## 4. 界面（Qt）

- `app/terminal/TerminalView`（QQuickPaintedItem，QML 类型 TerminalView）：按等宽字体量格子，ASCII 连成一段画、其余字符逐个画在
  自己的格子里（中文占两格、居中）；键盘、输入法（预编辑的字画在光标处）、鼠标、滚轮见 §1。大小变了按格子算出行列数交给
  `Terminal::resize`（120 ms 内合成一次；不到 10 列 3 行的过渡中间态不交——挪位置时宽高过渡中间的样子，shell 在两列一行的
  窗口里会把屏幕刷乱）。往上翻着看时来了新输出，视图停在原来那段内容上。
- `app/terminal/TerminalTabs`（QML 类型）：一段会话的标签、选中哪个（每段会话记住上次选的）、新开 / 关掉、能不能开（远程的原因）。
- `qml/terminal/TerminalCard.qml`、`TerminalButton.qml`。

## 5. Agent

两个内置工具（`AgentTools::all()` 末尾；执行在 Qt `core/terminal/TerminalTools`、Swift `Terminal/TerminalTools.swift`，主线程上读写终端）：

| 工具 | 权限 | 做什么 |
|---|---|---|
| `read_terminal` | 只读 | 读这段对话的终端最近的输出（`lines` 默认 200、最多 2000；`terminal` 是编号——id 前 8 位——或标题里的字，不填读最近有输出的那个）；有几个终端时结果开头列出来；结果只留最后 16000 字 |
| `run_in_terminal` | 执行 | 在这段对话的终端里新开一个标签运行 `command`（起好 shell 后打进去回车），马上返回开头 3 秒的输出，不等命令结束；终端卡片自动打开 |

- 主 Agent 只派活时也能用 `read_terminal`（`isOrchestratorTool`）：用户说「看下终端里的报错」不用为此派子 Agent。`run_in_terminal` 跟别的工具一样归子 Agent。
- 确认：`run_in_terminal` 同 `run_shell`——计划模式问，自动模式按命令判断删除类（`isDestructive`），完全模式不问；也拒绝结束 Friday 自己的命令。
- `read_terminal` 算轮询类工具（`pollingTools`）：等开发服务器起来时连着读几次不算原地打转。
- 远程：工具在服务端执行，终端也在服务端，客户端看到的是同一批（Agent 开的终端，客户端收到 `terminals` 事件后接上）。

## 6. 远程

协议见 REMOTE.md §5（`terminals` 事件、TerminalsState、`term.output`）和 §6「终端」（`term.open` / `attach` / `detach` / `write` / `resize` / `close`）。
要点：客户端自己编 id 开终端（服务端开好后把它接上）；连上（hello）时按 `state.terminals` 建副本、逐个 `term.attach`（服务端回最近的原始输出，
客户端清屏后重放）；之后 `term.output` 实时转（不合并，打字的回显不等）；大小以最后改的那边为准。经 WebSocket 连进来的不能用终端
（`available` 为假，同配对：只有登录了这台机器的连接可以）；hello 里没有 `terminals` 的是 2.1.x 的服务端，自动升级后才有。

几个客户端连着同一台机器时（用户定的 2026-10-07）：
- **终端是共用的**：shell 在那台机器上只有一份。任一客户端关掉标签（×）就结束 shell，大家的这个标签一起消失；
  Agent 开的终端大家都选中它（§1）。
- **卡片是各自的**：收起 / 打开终端卡片、卡片停在哪边、多宽多高，只改本机，不同步——每台客户端的窗口和屏幕不一样，
  别的客户端也可能正用着这个终端。卡片布局照旧按对话存在本机设置里（§2）。

## 7. 测试

- Qt `tests/tst_terminal.cpp`：模拟器（文字、颜色、中文宽度、滚动缓冲和变高回填、标题、按键 / 括号粘贴、备用屏幕）、伪终端（真起进程、退出码）、
  TerminalManager（开、按会话列、打命令看到输出、连着改大小不丢屏幕、会话删掉跟着关）。
- `tests/tst_remote.cpp` `terminals()`：真的服务端 + 两个客户端经本地 socket——客户端开、打字、输出转回；另一个客户端连上时接上已有的；
  服务端那边开的（Agent 的 run_in_terminal）两个客户端都接上、都看得到命令的输出；客户端关服务端也关。
- `tests/tst_tools.cpp`：两个新工具在内置工具列表末尾、权限。
- `tests/tst_panellayout.cpp`：卡片停靠的布局——打开的位置、新开一组成为两列、挪回去收窄、同一组里换顺序、下边成行、关了再开回原来那边、
  预演不存、比例、2.2.0 旧格式。
- 界面：Qt 调试脚本（`#terminalButton`、`#panelDock`（`rectFor`、`dropTarget`、`floatingRect`、`slotPlaceholder`、`insertBar(dropTarget)`、`layout.docks`）、`#terminalCardGrip` / `#agentTreeGrip` 拖动
  （`drag:…,hold` 不松手截预演、`drop` 松开）、`#dockEdgeHandle_<边>`、`dockGroupHandle_<边>_<i>`）；
  Swift 调试钩子 `card:<id>[:off]`、`card:<id>:move:<边>:<组>:<下标>[:new]`、`carddrag:<id>:<x>,<y>` / `carddrop`（打印落点、预演、挪完的布局）、
  `new`、`send:<文字>`、`idle`、`terminals[:行数]`、`termtype:<文字>`（⏎ 是回车）。
