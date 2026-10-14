# Friday（Qt 版）架构

Friday 的 Windows / Linux 版（同一个程序 `friday` 带命令参数时是命令行版、`friday server …` 是远程 Friday 的服务端），用 Qt 6 QML + C++ 重写 macOS 版（SwiftUI）。
**Swift 源码（`Friday/Friday/**.swift`）是唯一的真相来源**：功能、界面结构、文案（中文原文逐字一致）、交互细节、默认值、
提示词、工具定义都严格对齐 Swift；只有「观感」换成各平台的原生样式（原生 Qt Quick Controls 样式、系统窗口与标题栏、
系统配色与字体；Windows 11 上工具栏行并进标题栏）。

v2.0 起 Qt 版不带本地推理（1.x 用 llama.cpp / stable-diffusion.cpp 跑本地模型、本地看图 / 生图 / 生视频，已全部删除）：
大脑只有在线大模型（§6），工具照旧在本机执行。

Swift 源码仍在演进（macOS 版照常提交）：实现时以当时的 Swift 文件为准，头文件与 Swift 不一致时按 Swift 追加 / 调整
（§2.3 第 4 条），并在交付总结里说明。

相关文档：

| 文档 | 内容 |
|---|---|
| 本文 | 目录与模块、线程模型、错误处理、JSON 同构规则、QML ↔ C++ 约定、Swift → Qt 文件对照、构建 |
| [`REMOTE.md`](REMOTE.md) | v2.1 远程 Friday：friday-server、客户端协议（Qt、Swift、以后的手机 App 都按它实现） |
| [`TERMINAL.md`](TERMINAL.md) | v2.2 内置终端与卡片停靠：libvterm / SwiftTerm、伪终端、终端卡片、四边停靠、Agent 的 read_terminal / run_in_terminal |
| [`PACKAGING.md`](PACKAGING.md) | 安装包布局、从 1.x 升级、`latest.json` 的 `assets`、各平台换包更新、Windows 安装程序与 deb |
| `STYLE.md`（设计系统） | `Friday.Style` 组件库：原生样式的选择、Theme、窗口、工具栏、菜单、表单…的用法 |

---

## 1. 目录结构

```
qt/
├── CMakeLists.txt              顶层：选项、find_package、生成 BuildInfo.h、add_subdirectory
├── cmake/BuildInfo.h.in        版本 / 渠道 / 平台 / 架构（== Swift 的 CurrentBuild + Info.plist）
├── cmake/FridayCodePage.cmake  Windows 可执行文件嵌 activeCodePage=UTF-8 清单（utf8.manifest）
├── docs/                       本文、PACKAGING.md、STYLE.md 等；PORT_V15_V19.md、port/ 是移植时的计划（历史）
├── src/
│   ├── core/                   friday_core（静态库）：全部非界面代码，GUI 与 CLI 共用
│   │   ├── util/               基础设施：Errors、CancelToken、MainThread（runOnMain）、Worker（线程池 / Promise）、
│   │   │                       Http（阻塞 HTTP / SSE / 下载）、ProcessRunner、Json、JsonKey（JSON 键名的简写 key("id")）、Paths、Platform、SettingsFile、Format、
│   │   │                       Characters（Swift String 的语义：按字形簇数 / 截取、空白与换行、Int(String)；各处共用，不再各写一份）、
│   │   │                       OrderedSave（后台按顺序写盘、只落最新的快照：会话、密钥库、日志都用它）
│   │   ├── models/             AppSettings、ChatModels、ConversationStore、MediaStore
│   │   ├── secrets/            SecretStore、SecretGuard（v1.16 密钥只以名字流转）
│   │   ├── llm/                ModelListFetcher、OnlineServices
│   │   │   └── engine/         ChatEngine、EngineStore、OpenAIChatEngine、RequestGate、QuotaPolicy、SessionFolding
│   │   ├── agent/              AgentControl（v2.1：界面操作 Agent 的接口）、AgentRunner、SubAgentRunner、AgentTeamState、ToolSink、
│   │   │                       ToolRegistry、ToolTypes、AgentTools、WebSearch、WorkingMethod、TaskState、ContextFolding、TurnCheck、
│   │   │                       MetaTools、ScriptTools、ShellEnvironment、AgentProfiles、AgentJournal、RefreshThrottle、DebugHooks、
│   │   │                       DeliveryCheck
│   │   ├── capabilities/       Capability、CapabilityRegistry、ScriptProviders、HTTPClient、MediaEncoding、AudioEncoding
│   │   │   └── providers/      OpenAIStyleAPI、OpenAICompatibleBackend、ArkBackend、DashScopeBackend、DoubaoSpeechBackend、SpeechSupport
│   │   ├── mcp/                MCPManager、MCPServerSearch、McpClient
│   │   ├── update/             ReleaseVersion、UpdateManager
│   │   ├── remote/             v2.1 远程（REMOTE.md）：RemoteProtocol（协议与编解码）、RemoteServer（friday-server 的服务端）、
│   │   │                       RemoteClient（ssh / wsl.exe / WebSocket 连接）、RemoteRunner（客户端的 AgentControl）、RemoteSession（客户端的组合根）、
│   │   │                       RemoteDevices（WebSocket 设备的配对令牌）、RemoteTerminals（v2.2 远程终端的副本）、
│   │   │                       LocalService（和本机后台服务在本地 socket 上同步一问一答）
│   │   ├── terminal/           v2.2 内置终端（TERMINAL.md）：TerminalEmulator（libvterm 包装）、Pty（ConPTY / forkpty）、
│   │   │                       Terminal / TerminalControl（接口）、TerminalManager（本机）、TerminalTools（Agent 的两个工具）
│   │   └── FridayCore.h/.cpp   组合根（== FridayApp.init）
│   ├── app/                    friday（图形界面）
│   │   ├── main.cpp            入口：选原生控件样式、中文回退字体、标题栏模式、建对象、注册 QML 单例、加载 Main.qml
│   │   ├── FridayApp.h/.cpp    菜单命令、托盘、关于面板、窗口请求（QML 单例 App）
│   │   ├── SpeechReader.h/.cpp 朗读（QTextToSpeech）
│   │   ├── ThumbnailProvider   image://thumb 缩略图
│   │   ├── ClipboardBridge     剪贴板 / 拖放（QML 单例 Clipboard）
│   │   ├── VideoFrameGrabber   视频抽帧（Qt Multimedia，装进 MediaEncoding）
│   │   ├── AudioCodec          录音解码 / m4a 编码（QAudioDecoder、QMediaRecorder + QAudioBufferInput，装进 AudioEncoding）
│   │   ├── LoadScheduler       先占位、后台加载（v1.17，QML 单例 LoadScheduler；QML 一侧是 chat/DeferredLoader.qml）
│   │   ├── MachineSwitcher     切换机器、添加远程、同步配置（v2.1，QML 单例 Machines；REMOTE.md §11）
│   │   ├── DesktopService      桌面端兼当服务端（v2.1；REMOTE.md §12）
│   │   ├── StandbyTakeover     启动时后台服务在跑任务：先连着它用，空闲后接管（v2.1；REMOTE.md §12）；和后台服务的同步一问一答在 core 的 remote/LocalService
│   │   ├── viewmodels/         界面用的模型：SidebarModel、SearchResultsModel、ChatViewModel、MessageListModel、
│   │   │                       ModelMenuBuilder、ToolListModel、ServiceCatalog、MarkdownRenderer、
│   │   │                       AgentTreeModel（后台任务面板）、PanelLayout（v2.2 卡片停靠的布局，每段会话一份）、Enums、Formatters
│   │   ├── terminal/           v2.2 内置终端的界面：TerminalView（画格子、键盘 / 输入法 / 鼠标）、TerminalTabs（一段会话的标签）
│   │   ├── platform/           只在 Windows 上编译的原生代码：WindowsTitleBar（Windows 11 合并标题栏、开窗防白闪；
│   │   │                       见 STYLE.md「标题栏」）
│   │   ├── qml/                QML 主模块 URI "Friday"：Main.qml、ContentView.qml、UpdateBanner.qml、AboutPanel.qml、
│   │   │   ├── sidebar/        侧边栏、搜索
│   │   │   ├── chat/           聊天、输入栏、消息、工具卡、Markdown
│   │   │   ├── agenttree/      后台任务面板（v2.0 前叫 Agent 树）
│   │   │   ├── media/          图片 / 视频 / 附件
│   │   │   └── settings/       设置 sheet 的各页、服务目录
│   │   ├── resources/          应用图标、托盘图标（friday.png、tray.png…）
│   │   └── style/              设计系统 Friday.Style（原生控件的薄包装 + Theme，静态 QML 模块，自带 CMakeLists.txt）
│   ├── cli/                    命令行版（静态库 friday_cli，链进 friday：带命令参数就不开界面，FridayCli；§11）：命令行对话（CliSession、Terminal）
│   │                           + 远程 Friday 的常驻服务（ServerCommand：friday server …，或程序文件叫 friday-server 时；兼 --attach 桥；REMOTE.md）；
│   │                           main.cpp 是没有图形界面的 friday-headless（服务端包里的 bin/friday）
│   └── console/                Windows 的 friday.com：终端里敲 friday 时先跑它，把参数和标准句柄交给 friday.exe（§11）
├── tests/                      friday_tests（QtTest；TestRegistry.h + main.cpp + tst_*.cpp + fixtures/）
├── installer/                  Windows 安装程序（stub、卸载器、安装界面、打包工具 mkpkg；PACKAGING.md §7）
├── ci/                         构建与打包脚本（见 ci/README.md、PACKAGING.md）
└── tools/                      开发工具（图标生成 gen_icons.py）
```

**文件命名**：C++ 类名、方法名与 Swift 类型一一对应（`ConversationStore`、`AgentRunner`…），不用命名空间。
每个 Swift 文件在 Qt 侧有对应的 `.h/.cpp`（或 `.qml`），文件头注释写「对应 Swift：…」。对照表见 §9。

---

## 2. 构建目标

| 目标 | 类型 | 依赖 | 说明 |
|---|---|---|---|
| `friday_core` | 静态库 | Qt6::Core / Network / Concurrent / Gui（Gui 只用 QImage）/ WebSockets（v2.1 远程的 WebSocket 入口）；私有链接 Qt6::CorePrivate（只在 ScriptProviders.cpp 里用 QZipReader / QZipWriter，不进公开头文件） | `src/core/**` 用 GLOB_RECURSE 收集；**不依赖 Qml / Quick / Multimedia** |
| `friday_header_check` | OBJECT（不默认构建） | 同上 | 每个 core 头文件单独编译一次，检查自洽：`cmake --build <b> --target friday_header_check` |
| `friday` | 可执行（WIN32 / MACOSX_BUNDLE） | friday_core、friday_cli、Qml、Quick、QuickControls2、Svg、Multimedia、TextToSpeech、Widgets；Windows 另链系统库 dwmapi、uxtheme | QML 主模块 URI `Friday`；`qml/**` 与 app 层 C++ 都用 glob 收集（`platform/` 只在 Windows 上收）；链接 `friday_style` + `friday_styleplugin` |
| `friday_style` | 静态 QML 模块 | Quick、QuickControls2 | URI `Friday.Style`，在 `src/app/style/`（设计系统维护） |
| `friday_cli` | 静态库 | friday_core | 命令行版（`src/cli/`）：命令行对话 + 远程 Friday 的常驻服务（`friday server …`，REMOTE.md）；链进 `friday`，带命令参数时不开界面（§11）。v2.6–v2.7 是单独的 `friday-cli`，v2.6 之前服务另有 `friday-server` |
| `friday-headless` | 可执行（控制台） | friday_cli | 没有图形界面的 friday（不链 Qt Quick）：远程用的服务端包里叫 `bin/friday`，旁边放别名 `friday-server`（叫这个名字时直接是服务，参数不带 `server`）；`FRIDAY_BUILD_GUI=OFF` 时只编它 |
| `friday-console` | 可执行（控制台，只在 Windows 上） | 不链 Qt | 输出 `friday.com`：终端里敲 `friday …` 时 cmd / PowerShell 先找到它，把参数和标准句柄交给旁边的 friday.exe、等它结束、带回退出码（friday.exe 是 GUI 子系统，终端不等它） |
| `friday_tests` | 可执行 | friday_core、Qt6::Test | `ctest` 或直接运行；`friday_tests TestUtil` 只跑一个类 |
| `FridaySetup`、`uninstall`、`FridaySetupUI`、`mkpkg`、`installer_shortcut_test` | 可执行 | 除 `FridaySetupUI`（Qt Quick）外不链 Qt、静态链 VC++ 运行库 | 只在 Windows 上、构建图形界面且 `FRIDAY_BUILD_INSTALLER` 开着时构建（`installer/`，PACKAGING.md §7） |

新增 `.h/.cpp/.qml` 不用改 CMake（`CONFIGURE_DEPENDS`）。**QML 文件名在整个 `Friday` 模块里必须唯一**（类型名取文件名），
而且**不能和 `Friday.Style` 的组件重名**（两个模块都 import 时会歧义）：比如工具页的徽章叫 `ToolBadge.qml`，不叫 `Badge.qml`。

### 2.1 CMake 选项

| 变量 | 默认 | 用途 |
|---|---|---|
| `FRIDAY_VERSION` | `0.0.0` | 版本号 X.Y.Z（== CFBundleShortVersionString） |
| `FRIDAY_BUILD_NUMBER` | `0` | 构建号 |
| `FRIDAY_RELEASE_TAG` | 空 | CI 打包时的标签 `vX.Y.Z` / `vX.Y.Z-beta`；空 = 本机开发版（只提示更新、不自动装） |
| `FRIDAY_RELEASE_CHANNEL` | 空 | `stable` / `beta`；空 = 按标签推断 |
| `FRIDAY_BUILD_GUI` / `FRIDAY_BUILD_CLI` / `FRIDAY_BUILD_TESTS` | ON | |
| `FRIDAY_BUILD_INSTALLER` | ON | Windows 安装程序的部件（只在 Windows 上、构建图形界面时有效） |
| `FRIDAY_QT_DEPLOY` | OFF | `cmake --install` 时顺带跑 Qt 的部署脚本。它只按界面源码里的 import 部署 QML 模块，带不上 main.cpp 运行时才选的控件样式，装出来的 friday 加载界面会失败（打开时配置阶段打警告）；正式打包用 `ci/package_windows.ps1` / `ci/package_linux.sh` |

这些值（外加 CMake 按目标机定的平台 `windows|linux|macos`、架构 `x64|arm64`）生成到 `<构建目录>/generated/BuildInfo.h`
（`struct BuildInfo { static constexpr … }`），代码里通过 `CurrentBuild`（update/ReleaseVersion.h）使用，不要直接散落引用。
v2.0 起没有推理后端（1.x 的 `FRIDAY_GPU_BACKEND`、`FRIDAY_INFERENCE_SDK` 等选项已删除），`CurrentBuild::assetKey()` = `<平台>-<架构>`。

### 2.2 本机构建

**Qt 版本**：本机开发与 CI 都用 Qt 6.11.3（`ci/deps.env` 的 `FRIDAY_QT_VERSION`，升级时一起换）。`find_package(Qt6 6.8)` /
`qt_standard_project_setup(REQUIRES 6.8)` 是声明的最低版本，但只在 6.11.3 上验证过；新代码要用 6.8 以后才有的 API 时，
把 CMake（顶层与 `src/app/style/`）和文档里的最低版本一起改，不要声称没测过的兼容性。

Windows：VS 2022 + Qt 官方 `msvc2022_64`，`$env:QT_ROOT_DIR = 'C:\Qt\6.11.3\msvc2022_64'; pwsh qt/ci/build_app.ps1`
（构建目录 `qt/build-app/build`，可执行文件在它的根下），见 `qt/README.md`。

macOS 开发机：

```bash
cd qt
cmake -S . -B build-arch -G Ninja -DCMAKE_PREFIX_PATH=/opt/homebrew
cmake --build build-arch
./build-arch/tests/friday_tests
```

> 本机的 Command Line Tools 带的 MacOSX27 SDK 与链接器不兼容（`tapi error: unknown architecture arm64e.x1`）。
> 遇到时改用 Xcode 的工具链：
> `-DCMAKE_CXX_COMPILER=/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/clang++`
> `-DCMAKE_OSX_SYSROOT=/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX26.5.sdk`

`cmake --install build-arch --prefix <目录>`：`friday`、`friday-headless`（Windows 另有 `friday.com`）装到 `<prefix>/bin`（Windows 装在 `<prefix>/` 根）；
打包脚本再放 Qt 运行库（Windows 还有 VC++ 运行库）与 `package.json`（PACKAGING.md）。

### 2.3 并行开发时怎么验证自己的代码

实现包之间互相依赖对方的 `.cpp`，全部完成之前 `friday` 链接不过。各包自查：

1. `cmake --build build-arch --target friday_header_check`：头文件都能独立编译；
2. 只编自己的目标文件：`ninja -C build-arch src/core/CMakeFiles/friday_core.dir/agent/AgentRunner.cpp.o`
   （app 层：`src/app/CMakeFiles/friday.dir/viewmodels/ChatViewModel.cpp.o`；QML 文件由 qmlcachegen 编译，
   目标形如 `src/app/CMakeFiles/friday.dir/.rcc/qmlcache/friday_qml/chat/ChatView_qml.cpp.o`）；
3. 单元测试：`friday_tests` 链接的是静态库，只会拉进被引用到的目标文件——只测自己模块的测试在别的包没完成时也能链接、运行；
4. 不要改别人的 `.cpp` / `.qml`。头文件归实现它的包：可以加私有成员、补 include、**追加**公开方法；
   不要改动或删除已有的公开签名（别的包正在按它写代码），确实要改时在交付总结里写清楚。

---

## 3. 线程模型

目标：C++ 代码能逐行对齐 Swift 的 `async/await`，同时不卡界面、不死锁。

### 3.1 规则

1. **被界面观察的状态只在主线程修改**。会话、设置、模型状态、工具注册表…都是 `QObject` + `Q_PROPERTY` + `NOTIFY`，
   相当于 Swift 的 `@Observable` / `@MainActor`。
2. **长任务在工作线程里「阻塞式、顺序书写」**：Agent 的一个回合、子 Agent、下载、生成、MCP 连接。
   用 `Worker::run` / `Worker::spawn` 起，代码里直接调用阻塞的辅助函数（`Http::send`、`Http::streamLines`、
   `ProcessRunner::run`、`Worker::sleep`、`Promise::wait`），写法和 Swift 的 `try await` 一一对应。
3. **工作线程改状态用 `runOnMain(...)`（排队、不等），读状态用 `runOnMainSync(...)`（阻塞等结果）**。
   排队是有序的：先 `runOnMain` 改、后 `runOnMainSync` 读，一定读到改过的值。
4. **主线程永远不阻塞等待工作线程**（不 `wait`、不 `join`、不 `QFuture::waitForFinished`），否则工作线程里的
   `runOnMainSync` 会死锁。主线程要结果就等信号 / 回调。
5. **取消用 `CancelToken`**：`Worker::run` 把令牌设为线程的「当前令牌」，阻塞辅助函数默认读它，取消时抛
   `CancellationError`；子任务用 `makeChild()`。新令牌一律 `Worker::newToken()`（退出时会被一起取消）。
6. **退出**：`FridayCore::shutdown()` → `MainThread::beginShutdown()`（之后 `runOnMainSync` 不再等主线程，直接抛
   `CancellationError`）→ 取消全部任务 → 主线程边处理事件边等工作线程最多 3 秒。后台写文件的地方在
   `MainThread::isShuttingDown()` 时改为同步写（`ConversationStore::saveNow` 等），保证落盘。

### 3.2 基础设施一览（`src/core/util/`，已实现并有单元测试）

| 头文件 | 对应 Swift | 用法 |
|---|---|---|
| `Errors.h` | `throws`、`CancellationError`、`AgentToolError` | `throw AgentToolError::failed("…")`；`catch (...) { describeCurrentException(); }` |
| `CancelToken.h` | `Task` 的取消 | `CancelToken::checkCurrent()` == `try Task.checkCancellation()`；`currentIsCancelled()` == `Task.isCancelled` |
| `MainThread.h` | `@MainActor`、`MainActor.run` | `runOnMain(fn)`、`runOnMainSync(fn)`、`isMainThread()` |
| `Worker.h` | `Task {}`、`Task.sleep`、`CheckedContinuation` | `Worker::run(token, fn)`、`Worker::spawn(token, fn)`、`Worker::sleep(ms)`、`Promise<T>` |
| `Http.h` | `URLSession`（data / bytes.lines / download） | `Http::send`、`Http::streamLines`（SSE）、`Http::download`（.part 续传） |
| `ProcessRunner.h` | `Process` + `Pipe` + 超时 | `ProcessRunner::run(options)` |
| `Json.h` | `JSONEncoder` / `JSONDecoder` 的规则 | UUID、日期、可选字段、带关联值的枚举、Swift 同款 pretty 文本 |
| `Paths.h` | `URL.applicationSupportDirectory` 等 | 数据 / 缓存目录、更新包暂存目录 |
| `Platform.h` | `ProcessInfo`、`NSWorkspace`、`SystemCapacity` | 内存、shell、文件管理器、回收站、平台文案 |
| `SettingsFile.h` | `UserDefaults.standard` | INI：`数据目录/settings.ini` |
| `Format.h` | `ByteCountFormatter`、`.relative(presentation: .named)` 等 | 字节数、相对时间、千分位、万 / 亿 |

### 3.3 写法示例

**起一个回合（== `currentTask = Task { … }`）**

```cpp
void AgentRunner::send(const QString& text, const QStringList& attachments, const QUuid& conversationID,
                       const std::optional<QUuid>& replacing)
{
    // …主线程里的同步部分：导入附件、写入用户消息和空助手消息、resetStats()、beginPass()…
    m_currentTask = Worker::newToken();
    Worker::run(m_currentTask, [this, conversationID, history, userMessage] {
        runTurn(conversationID, history, userMessage);     // 顺序书写 Swift Task 里的全部代码
    });
}

void AgentRunner::runTurn(…)
{
    if (runOnMainSync([&] { return toolsEnabled; }))     // v2.0：设置里没有工具总开关，toolsEnabled 只给测试关
        tools->mcp->waitUntilSettled();                    // == await tools.mcp.waitUntilSettled()
    try {
        auto engine = this->engine(conversationID, history);
        …
        try {
            consume([&](const EngineEventHandler& onEvent) { engine->stream(prompt, images, videos, onEvent); });
        } catch (const CancellationError&) {
        } catch (...) {
            recover(describeCurrentException(), conversationID, prompt, images, videos);
        }
    } catch (const CancellationError&) {
    } catch (...) {
        tools->journal->record(AgentJournal::Entry::Kind::Framework, "无法创建对话引擎", describeCurrentException());
        const QString message = describeCurrentException();
        updateMessage([message](ChatMessage& m) { m.error = message; });
    }
    finish();
}
```

**流式更新界面（== `updateMessage { … }`）**：排队到主线程，按顺序执行；读回时用 `runOnMainSync`。

```cpp
void AgentRunner::updateMessage(std::function<void(ChatMessage&)> body)
{
    const auto conversationID = m_streamConversationID;
    const auto messageID = m_streamMessageID;
    if (!conversationID || !messageID)
        return;
    runOnMain([this, conversationID, messageID, body = std::move(body)] {
        store->updateMessage(*messageID, *conversationID, body);   // 发 messageChanged，界面只刷这一行
    });
}

int AgentRunner::currentBlockCount() const
{
    return runOnMainSync([&] { /* 从 store 读正在写入的助手消息的块数 */ return 0; });
}
```

**审批（== `await withCheckedContinuation { approvals[event.id] = $0 }`）**

```cpp
// 工作线程（dispatch 里）
auto approval = Promise<bool>::make();
{
    QMutexLocker lock(&m_approvalsMutex);
    m_approvals.insert(event.id, approval);
}
sink.append(event);                          // 审批卡片出现在界面上
bool approved = false;
try {
    approved = approval->wait();             // 阻塞到用户点「允许 / 拒绝」；停止生成时抛 CancellationError
} catch (const CancellationError&) {
    approved = false;
}
if (!approved || CancelToken::currentIsCancelled()) { /* denied */ }

// 主线程（界面按钮 → AgentRunner.resolveApproval(eventID, allow)）
void AgentRunner::resolveApproval(const QUuid& eventID, bool approved)
{
    PromisePtr<bool> approval;
    {
        QMutexLocker lock(&m_approvalsMutex);
        approval = m_approvals.take(eventID);
    }
    if (approval)
        approval->resolve(approved);
}
// stop()：取消回合令牌，并把所有待审批的 resolve(false)
```
等确认期间（== Swift 231ab34）：`notePendingApproval` 把「事件 → 会话」登记到主线程的表里，侧边栏据 `hasPendingApproval(会话)`
显示橙色手势、`userAttentionRequested` 让 app 层在 Friday 不在前台时闪任务栏图标（`QWindow::alert`）；确认 / 拒绝 / 停止后撤下。
界面上 `ToolEvent::hasPendingApproval()`（递归进子 Agent 的运行记录）为真的卡片不折叠、标橙色「待确认」，消息底栏显示
「等待你确认一个工具调用（在上面的卡片里）」。启动时 `ConversationStore` 用 `MessageBlock::markInterrupted` 把上次退出时
还在运行 / 待确认的调用标成失败（「等待确认时应用退出，已中断」/「应用退出时仍在运行，已中断」）；派子 Agent 的卡片（delegate_task、
run_parallel_tasks、rework_agent）写成 `MessageBlock::interruptedAgentText()`（同样的开头，接着说明子 Agent 不会再交回结果、要接着做就重新派发），
主 Agent 下一轮看到才不会以为它们还在后台跑（v1.20）。退出途中（`MainThread::isShuttingDown()`）被取消的后台子 Agent 不改卡片、不发通知
（照常收尾会被写成「被用户停止了…不要重新派发」），也不再新开一轮送结果——和 Swift 一样留给下次启动标中断。
主 Agent 调 `wait_agents` 时按 `backgroundChildren` 实际在跑的回答，一个都没有时照实说（v1.20，以前一律回「在后台跑，不用等」）。
v1.20 实际跑下来补的几处（Swift 与 Qt 同）：`run_shell` 执行前用 `AgentTools::selfKillRefusal` 拦下会结束 Friday 自己的命令
（按名字结束 friday / friday-cli / friday-server / friday-headless，或结束本进程 PID；子 Agent 编译时以为是 friday-cli 占着文件，把运行自己的进程结束了）；
在线请求遇到 400 / 422 说「不接受某个可选参数」（`OpenAIChatEngine::unsupportedParameters`，只认 temperature、top_p、stream_options、
思考参数这类 Friday 自己加的）就去掉重发，并按引擎 id 记住，之后的请求不再带；HTTP/2 连接出错（`ProtocolFailure`）算网络临时故障
（== Swift 的 networkConnectionLost）；大脑或网络出的错（`AgentRunner::isEngineFailure`）不记作没做成的派发，记下的那次带上用的大脑，
重派时明确换了别的大脑就不算原样重派。
再补四处：`read_agent_log` 加 `offset`（记录太长时只给开头和结尾，中间的分段读，`AgentLog::text` + `AgentTools::page`）；子 Agent 工作方式的
「善后」写明按启动时拿到的 PID 停掉临时进程、不要按名字结束；能力提供方报余额不足、欠费、密钥不对、没有权限（`CapabilityRegistry::unusableReason`，
按 `QuotaPolicy` 分类，限流和周期额度不算）时 `run` 直接停用它、记诊断日志，并告诉模型别再试、告诉用户怎么恢复；
哪些事要停下来交给用户由 设置 › 智能体「这些事停下来交给我」定（`agent/UserStops`：花钱、注册登录账号、要用户提供密钥、还没给过的授权、
要用户拍板的取舍、只有用户亲手才能验证的；默认全部勾上，存在 settings.ini 的 `userStopsOff`），写进验收提示、验收说明、工作方式和交付检查，
没勾的写明不用停下、自己想办法办成（用户要 Agent 自动注册账号这类事时取消）。

**并行子 Agent（== `tasks.map { Task { … } }` + `withTaskCancellationHandler`）**

```cpp
QList<PromisePtr<ToolOutput>> tasks;
for (const auto& spec : specs) {
    auto child = CancelToken::current()->makeChild();       // 父回合取消 → 子 Agent 一起取消
    tasks << Worker::spawn(child, [=, this] {
        return runSubAgent(spec.profile, spec.task, spec.engine, depth, spec.eventID, conversationID);
    });
}
for (int i = 0; i < tasks.size(); ++i) {
    try {
        outputs << tasks[i]->wait();                         // 当前令牌取消时抛 CancellationError
    } catch (const CancellationError&) {
        throw;
    } catch (...) {
        failures[i] = describeCurrentException();
    }
}
```
实际代码（SubAgentRunner.cpp）：每个子任务一结束就在自己的线程里把状态写进卡片（`updateAgentEvent`，== Swift 231ab34，不等兄弟）——
改存档，父级是子 Agent 时同时投进它本地块副本的信箱（`AgentEditMailbox::postEdit`，它自己的线程下次发布 / 读块前应用）；
失败（不含停止）记进诊断日志；最后 `CancelToken::checkCurrent()`——停止生成时父卡片按停止处理（失败「已停止」），不显示成功。

**父子 Agent 协作（v1.7–v1.8，docs/PORT_V15_V19.md、docs/port/subagents.md）**：团队状态 `AgentTeamState`（QML 单例 `AgentTeam`）
内部一把锁、任意线程可调，记着运行中的子 Agent（可单独停止 `stopAgent`、发消息 `sendToAgent`）、发给它们的消息、父级的收件箱、
活动 / 步数 / 速度。三种派法：同步 `delegate_task` 就在调用方线程里跑、套一个子令牌（单独停掉它不影响父级）；`run_parallel_tasks`
每个任务一个 `Worker::spawn`（子令牌）；`background: true` 起一个独立令牌的工作线程，立刻返回，结束时卡片定稿、通知进父级收件箱
（`finishRun`：注销与投递一把锁）。父级做完当前一步时引擎的 `interjection` 把收件箱和消息交给它；主 Agent 想结束一轮时还有后台子 Agent
就等它们（`drainBackgroundAgents`）。v1.15 起主 Agent 派的第一层（派发、并行、打回）一律在后台（`dispatchesInBackground`）、是独立的
（`markDetached`，下级跟着算，`isIndependent`；线程上带 `TaskLocal<IndependentRun>`，`isStopping()` 恒为假）：drain 不等它们，这一轮
收尾（`reset`）、点停止（`cancelTeam`）都不停，卡片不标停止；结果进主 Agent 收件箱时 `onPost` → `noticeArrived` → `scheduleQueueDelivery`，
对话空着就新开一轮（`send(…, notices)`，送回来的汇报 `fromAgent` 在前；对话里不显示，MessageListModel 读路径时跳过）。删除对话 `stopAllAgents` 连独立的一起停。开关 `firstLevelAgentsInBackground`
（设置文件同名键 / `FRIDAY_FIRST_LEVEL_BACKGROUND=0`；测试里 `setFirstLevelInBackground(false)`）。看门狗（主线程 QTimer，5 秒）停掉卡住的、到期限的；等下级 / 确认时把活动标成等待（`waitingWithoutSlot`）。
阻塞等待（`Promise::waitImpl`）让出 Worker 线程池名额，层数、数量不限也不会占满线程池；同时登记的子 Agent 安全上限 1000。
子 Agent 看不到对话（== Swift 7f3a79f，v1.19 改）：派发时把用户在这段对话里说的原话（`TaskState::userWords`，太多时最新的原样约 4000 字、
更早的只附第一条和像要求的）和主 Agent 记下的要求和决定（约束、决定、关键信息里的规矩 `looksLikeRule`，标明是它记的）接在任务后面
（`TaskState::subAgentNote`；目标和别的关键信息不附）；第 2 层起的上级链（`ancestryNote`）不再重复用户的请求。
v1.19 子 Agent 一次运行是同一个在线会话，`OpenAIChatEngine::folding`（`llm/engine/SessionFolding`，由 `ContextFolding::session` 生成）
在每次请求前折叠较早的步骤：没折叠的部分超过 `ContextFolding::sessionBudget()`（主会话在线预算的两倍，96000——折得早子 Agent 会回头重读前面
读过的内容，实测抵掉省下的）就把最近约半个预算之外的工具结果、长参数、长正文换成
`ContextFolding` 的摘要、去掉 reasoning_content（`EngineConfig::echoesReasoning` 的 DeepSeek 留着），用户消息和子 Agent 的结果不动，
最新的一步总是原样；`AgentTools::truncate` 超长输出留开头 3/4、结尾 1/4。v2.8 主 Agent 也挂同样的 `folding`（`AgentRunner::configure`），
每轮开头（和自愈重建引擎后）`foldFromHere()`：只折这一轮自己的步骤，之前几轮照旧归下面的上下文折叠；折了时消息上提示一次（`AgentRunner::turnFoldedNotice`）。子 Agent 的系统提示词是 `AgentTeamTools::subAgentPreamble` + 角色说明 +
`reportGuidance`（v1.20 回报格式接在角色说明后面，盖过角色说明里对输出格式、长度的要求）+ 工具说明，要求按
「结论 / 依据 / 改动 / 未完成与风险」回报、两千字以内；交给上级时超过 `AgentTeamTools::resultLimit`（4000）字的由 `AgentResults::clip`
存全文、只交开头 3/4 和结尾 1/4。v1.20 起子 Agent 只有在开着 Ultra（`ThinkingLevel::allowsNesting`；v2.9 起是这段对话的任务模式「极限性能」——任务模式每段对话各自记
（`Conversation.teamMode`，没选过跟随 `AppSettings::defaultTeamMode`），`AgentRunner::teamPolicy` 按它算出 `TeamPolicy`（能不能派、只派活、快工具、ultra），
主 Agent 的工具列表、系统提示、preflight、run_shell 时限都按它；`ToolRegistry` 的全局开关只剩「派发工具关掉 = 一律不派」的总开关，
这时 `AgentRunner::thinkingLevel` 一律返回 Ultra，会话里存过的 Ultra 按高）时才能再往下派：
否则 `subAgentSpecs` 不给多 Agent 一组的工具（`report_to_parent` 除外）、`ToolRegistry::guidance(Role::Sub, false)` 没有「再往下派」，
`runTeamTool` 也退回照着名字硬调的 delegate_task / run_parallel_tasks。交给上级的结果末尾附运行记录 id（`AgentLog::shortID`，事件 UUID 的前 8 位），主 Agent 要核对时
用 `read_agent_log` 按 id 翻完整过程（`AgentRunner::readAgentLog` → `AgentLog::render`，在 runTeamTool 里分派）。
v1.18（== Swift 同版本）交给上级的结果是 `SubRun::reply`：最后一次调用工具之后写的那段（`DeliveryCheck::finalReply`；为空时交全部正文），
过程话不交。后台子 Agent 结束时卡片的 result 改成结论（给界面看），改之前把模型当时拿到的写进 `ToolEvent::modelResult`，`toolResultText`
优先用它——结论只由汇报送一次。`historyMessages` 对旧存档：汇报按 id 找到运行记录、去掉最终结果之前的过程话（`noticeForModel`）；
派发卡片的结果在后面它自己的汇报里（标题行带同一个 id、开头或结尾 300 字对上）就换成一句指向汇报的话，「后台子 Agent 全部结束」的并行卡片
换成派了谁；汇报和子 Agent 的结果折叠了也不截。发送新消息时用同样的函数（`engineMessage` 带上 `lastTaskNote` / `agentRuns`），和重建逐字一致。
主 Agent 一轮结束时的验证检查点（`AgentRunner::verifyIfNeeded`，规则在 `TurnCheck`）：写了代码或配置、之后没有只读 / 执行类调用，
就在消息上接一句提示、记诊断日志，带着提醒再生成一段；每轮最多一次，计划模式和用户说了不用运行 / 测试时不查。
每段在线请求过引擎的并发闸门 `RequestGate`（按引擎共用，没被限流就慢慢加、429 减半；只在请求真正在途时占名额）。
v2.0 起没有进程内的本地模型，也就没有按内存给本地子 Agent 排队的 `SubAgentScheduler`；v2.7 的本地大模型跑在 friday-llm 里（Friday.Powers），排队、同时算几个、共用前缀都由它自己调度（POWERS.md §8）。

**SSE（== `for try await line in bytes.lines`）**

```cpp
HttpRequest request(OpenAIStyleAPI::endpoint(config.baseURL, "chat/completions"), "POST");
request.timeoutMs = 300000;
request.setJsonBody(body);
request.setHeader("Accept", "text/event-stream");
request.setBearer(apiKey.value_or(QString()));
const HttpResponse response = Http::streamLines(request, [&](const QByteArray& line) {
    if (!line.startsWith("data:"))
        return true;
    const QByteArray payload = line.mid(5).trimmed();
    if (payload == "[DONE]")
        return false;                                      // == break
    const QJsonObject object = QJsonDocument::fromJson(payload).object();
    …
    return onEvent(EngineEvent::chunk(content));           // 调用方要求停止时返回 false
});
if (!response.isSuccess())
    throw CapabilityError::http(response.status, response.bodyText());
```

**主线程上的异步操作（界面发起、结果回界面）**：主线程方法里 `Worker::run`，结束时 `runOnMain` 回写属性 / 发信号。
例如 `CapabilityRegistry::refreshAvailability()`、`ToolRegistry::testScriptAsync()`、
`ServiceCatalog::fetchModels()`。

### 3.4 AgentRunner 的线程约定

- 主线程：`send / stop / resolveApproval / set* / select* / resetSession` 与
  `generatingConversationID` 属性。
- 回合线程：`send()` 做完同步部分后起的工作线程；流式状态（`m_passBuffer`、`m_passStart`、统计字段）只在这个线程读写。
  `finish()` 最后把 `generatingConversationID = nil`、`store->saveNow()` 排队到主线程。
- `dispatch()` 在调用它的工作线程执行（主回合线程或某个子 Agent 线程），`ToolSink` 的方法也在这个线程上调用。
- `m_sessions` 加锁（主线程会清条目，回合线程会读写）；引擎对象同一时间只被一个线程用。
- 查询类方法（`engineConfig`、`permissionMode`、`thinkingLevel`、`workingDirectory`…）是主线程方法；工作线程里用
  `runOnMainSync([&] { return permissionMode(id); })`。`ToolRegistry` 的查询方法（`descriptor`、`preflight`、`guidance`、
  `activeSpecs`…）在工作线程调用时自己转主线程（头文件里标「任意线程」）。

### 3.5 其他对象的线程归属

| 对象 | 状态修改 | 可在工作线程直接调用的方法 |
|---|---|---|
| ConversationStore、AppSettings、UpdateManager、MCPManager（servers） | 主线程 | —（用 runOnMain / runOnMainSync） |
| SecretStore | 主线程（set / remove / markNotSecret） | `contains / get / substitute / missingReferences / resolve / environment / protect`（加锁；protect 收进新密钥后排到主线程发 namesChanged / autoChanged） |
| SecretGuard | — | 全部（库里已知的值是加锁的快照）：`detect / mask / maskJson / maskStored / scrubStoredFiles` |
| EngineStore | 主线程（add / update / remove） | `engines() / engine(id)`（加锁，返回拷贝） |
| AgentJournal | 主线程 | `record()`（自己排队到主线程） |
| ToolRegistry | 主线程 | 查询方法自己转主线程；`execute / saveScript / testScript / rollback` 在工作线程执行 |
| CapabilityRegistry | 主线程 | `run / checkJob / executeTool / availability`（工作线程，内部转主线程读写状态） |
| MCPManager | 主线程 | `call / addServer / waitUntilSettled / syncBlocking`（阻塞） |
| RequestGate | 内部加锁 | 全部（`acquire` 在请求所在的工作线程里阻塞等待） |
| McpClient | 自带线程 | 全部阻塞方法 |
| 能力后端（CapabilityBackend） | 无状态 | `availability / perform / poll` |
| MediaStore、Paths | — | 全部 |

---

## 4. 错误处理

- 工作线程代码用 **C++ 异常** 对齐 Swift 的 `throws`：`throw AgentToolError::invalid("…")`、`throw CapabilityError::http(…)`。
- 所有异常继承 `FridayError`，`message()` = Swift 的 `localizedDescription`，**文案逐字同 Swift**（各错误类的工厂函数
  已经把 Swift 的文案写好了：`AgentError`、`CapabilityError`、`UpdateError`）。
- `catch (...)` 之后用 `describeCurrentException()` 取文字，`currentExceptionIsCancellation()` 判断是不是取消。
- **异常绝不能穿过 Qt 事件循环**：槽函数、`Q_INVOKABLE`、`runOnMain` 排队的 lambda 里自己 catch。
  （`runOnMain` 排队的任务如果漏抛了，`MainThread` 会兜底打印警告；`runOnMainSync` 会把异常带回调用方线程。）
- 网络层错误：`Http::send` 不抛网络错误（写在 `HttpResponse::errorString`），由调用方按 Swift 的逻辑转成对应错误；
  取消总是抛 `CancellationError`。

---

## 5. 持久化：与 Swift 版同构

数据可以在 macOS 版与 Qt 版之间直接拷贝（与本地模型有关的内容 Qt 版读的时候忽略，见 §5.3 末尾）。

### 5.1 位置

| 平台 | 数据目录（== Application Support/Friday） | 缓存目录（== Caches/Friday） |
|---|---|---|
| Windows | `%APPDATA%\Friday` | `%LOCALAPPDATA%\Friday\Cache` |
| Linux | `$XDG_DATA_HOME/Friday`（`~/.local/share/Friday`） | `$XDG_CACHE_HOME/Friday`（`~/.cache/Friday`） |
| macOS（开发） | `~/Library/Application Support/Friday-Qt` | `~/Library/Caches/Friday-Qt` |

环境变量 `FRIDAY_DATA_DIR` / `FRIDAY_CACHE_DIR` 可覆盖（测试、CLI 用）。全部经 `Paths` 取。
v2.8 `FRIDAY_HOME`：换掉**默认**位置（`<它>/data`、`<它>/cache`、`<它>/powers`；测试版的启动脚本 `启动 Friday 测试版.cmd` 用它），
和上面两个不同，它不算「环境变量指定」：设置 › 通用 里照样能换存放位置（`storage.json` 在 `<它>/data` 里），本地能力也不和装好的 Friday 共用。

**换存放位置（Qt 版 v2.3，`util/StorageLocation`）**：用户要「整个系统的文件存放位置需要可以换」——设置 › 通用 › 文件存放位置，
数据、缓存、本地能力（Powers）整体换到选的文件夹：`<根>/Data`、`<根>/Cache`、`<根>/Powers`（选的文件夹不空就在里面建 `Friday`）。
- 记在**默认数据目录**的 `storage.json`（`{root, pending, models}`；它不跟着搬），桌面端、命令行、服务端（都是 friday）都按它取路径；环境变量优先（这时不让换）。
- 不在运行中搬：设置里检查（能写、不在现在的目录里面、目标是空的、空间够）后只登记 `pending`，「现在重启」起一个 `--takeover` 的新进程、自己退出。
- 新起的桌面端拿到**旧**数据目录的锁之后、建核心对象之前搬（`main.cpp` 的 `relocateStorage`，进度窗口）：复制（权限一起带过去）→ 核对大小 →
  改 `storage.json` → 删掉旧的（默认数据目录里的 `storage.json`、锁留着）→ 锁换到新的数据目录（`InstanceLock::relock`）→ 旧位置空了的文件夹删掉。
  搬失败：删掉复制了一半的、取消待办、说明原因，照旧用原来的位置。接管后台服务（锁不在桌面端手里）时不搬，下次再搬。
- v2.8 **模型**（用户「将模型工作目录修改放到通用这里」；2026-10-12 在 AutoDL 上换了位置、模型却还往 30 GB 的系统盘下、盘满了停住，
  问「模型文件下载放到了哪里，为什么改不了」→ 选了「跟着位置走 + 换时搬过去」）：
  - 实际的模型文件夹（`StorageLocation::effectiveModelsDir`）：选了的（`storage.json` 的 `models`）；没选、换了位置时是 `<根>/Models`；都没有是魔搭 / HF 的默认缓存
    （`MODELSCOPE_CACHE`、`HF_HUB_CACHE`、`HF_HOME` 只换默认缓存的位置，不锁设置）。新下的模型放 `<它>/modelscope/models`、`<它>/huggingface/hub`
    （`Paths::modelScopeCacheRoot` / `huggingFaceCacheRoot`）；默认缓存里的照样认（模型库扫两处，`Paths::modelScopeCacheRoots`；friday-llm 的 `--models` 也给两处），
    已经在默认缓存里有一部分的仓库接着下在那里（同一个仓库的文件不拆开）——所以换文件夹一定要把没下完的一起搬过去。
  - 搬（`llm/ModelMover`）：以仓库目录为单位，同一个盘改名、不同盘逐个文件复制（HF 快照里的符号链接照原样建）、全部复制好再删原来的，
    目标里已有的逐个文件并进去；中途失败退回原处。从默认缓存搬只搬 Friday 的（有 GGUF 或没下完的 .part / .incomplete，加生图 / 生视频配套文件的仓库）。
  - 换模型文件夹（`LocalModels::moveModels`，设置里「更改…」「恢复默认 / 跟着位置」）：正在下的先停（Task.paused，界面上照旧是在下、等着它的照样等）、
    friday-llm / friday-diffusion 先停（Windows 上开着的文件挪不动），搬完记下新文件夹、接着下（从 .part 续）；搬的时候新开始的下载先挂着。
    「搬过来」（`adoptDefaultModels`）把默认缓存里 Friday 的模型搬到现在的文件夹（换位置以前下的）。
  - 换存放位置时，没选模型文件夹的话模型跟着搬（`performPending` 里，数据搬好、记下新位置之后；没搬成的留在原处、记成选了的模型文件夹）。
- v2.8 看着远程机器时这一节看、改的是那台的（`storage.*`，REMOTE.md §6「文件存放位置」）；常驻服务起来时也按 `pending` 搬（`cli/ServerCommand.cpp`
  的 `relocateStorage`），「现在重启」是 `storage.restart`（新服务 `--takeover` 等旧的放开锁）。
- 测：`TestStorage`（临时目录当系统默认位置：搬过去、搬回默认、各种不让搬的情况、`FRIDAY_HOME`、模型存放位置、模型跟着位置搬、ModelMover 合并），
  `TestPowers::moveModelsWhileDownloading`（假 HF 慢慢给，下到一半换文件夹：停、搬、续传）、`TestRemote::storageOfRemote`。
  **Windows 上主目录不看环境变量**（Qt 取用户配置目录）：测试一定要用 `MODELSCOPE_CACHE` / `HF_HUB_CACHE` 把默认缓存指到临时目录，不然会搬到真的模型。
- **Swift 版（v2.9，用户「swift 端设置页面通用没有显示文件保存路径和修改文件路径」）**：同样的 `storage.json` 格式和规则，放在 Swift 自己的默认数据目录
  （`~/Library/Application Support/Friday/storage.json`）。`App/StorageLocation.swift`（`FridayPaths` 取路径——各存储一律经它，不再直接拼
  `URL.applicationSupportDirectory`；`StorageLocation` 登记 / 搬）、`LLM/ModelMover.swift`、`LocalModels.moveModels / adoptDefaultModels`
  （正在下的那一段停掉、外面等着的 download / use 不受影响，搬完从 `.part` 续）、`Views/StorageSettingsView.swift`（本机和看着远程时的 `storage.*`）。
  和 Qt 不同的地方：多一个「助手」目录（默认沿用 `Friday-Assistant/Assistant`，换了位置是 `<根>/Data/Assistant`）；没有单实例锁，
  每个 Friday 起来后拿着 `<默认数据目录>/friday.lock` 的共享锁，搬之前要独占它（同一个数据目录还有别的构建开着时不搬、提示下次再搬）；
  在 `FridayMain.main` 里、建任何存储之前搬（`App/StorageRelocation.swift`，进度窗口）；「现在重启」是 `UpdateManager.relaunch`（等这个进程退出再 `open`）；
  受管端的套接字和退出标记留在默认数据目录（`friday-server --attach` 的桥按固定位置找）。测：自检 `SelfTests.storageLocation`，
  界面脚本 `storagemove` / `storagemodels` / `storagestate` / `modeldownload` / `modelwait`（隔离实例 `CFFIXED_USER_HOME`）。

### 5.2 Swift Codable 规则（`util/Json.h` 提供全部辅助）

- UUID：**大写**带连字符（`Json::uuidString`；`QUuid::toString` 是小写，不要直接用）。
- 日期：`.iso8601` 写成 `2026-10-01T02:15:00Z`（UTC、**无毫秒**——Swift 读带毫秒的会失败）→ `Json::isoDate`；
  Swift 默认策略写距 2001-01-01 的秒数（Double）→ `Json::referenceSeconds`。读的时候 `Json::date` 两种都接受。
- 枚举：原始字符串（`rawValue()`）。
- 可选字段为 nil 时不写键（`Json::setIfPresent`）；读时键缺失 = nil（`Json::optString` 等）。
- 非可选、带默认值的字段：Swift 读的时候是**必填**的 → 写文件时一定写上；我们读的时候宽松，缺了用默认值。
- 带关联值的枚举：`{"text":{"_0":"…"}}`、`{"tool":{"_0":{…}}}` → `Json::enumCase / readEnumCase`。
- 字典 `[UUID: UUID]` 编成 `{"<UUID>": "<UUID>"}`（Swift 代码手动转的 `[String: String]`）。
- 键顺序无关紧要（QJsonObject 天然排序，相当于 `.sortedKeys`）。Qt 的缩进格式和 Swift 不同，不影响互读。
- 展示给用户 / 模型的 JSON 文本（工具参数 `ToolEvent.arguments`、脚本 schema）用 `Json::stringify(value, true)`，
  与 Swift `JSONValue.jsonString()` 逐字一致（两空格缩进、`"键" : 值`、键排序、不转义 `/`）。

### 5.3 逐个文件

| 文件（数据目录下） | 内容 | 日期策略 | 其他 |
|---|---|---|---|
| `conversations.json` | `[Conversation]`（空白会话不保存） | `.iso8601`（createdAt、updatedAt、ChatMessage.createdAt、GenerationStats.startedAt） | 见下 |
| `journal.json` | `[AgentJournal.Entry]`：id、date、kind（tool/framework/mcp/recovered）、title、detail | `.iso8601` | 最多 200 条 |
| `learning.json`（v1.10–v1.21） | v2.0 起没有学习，不再读写；旧文件留在磁盘上不删（装回 1.x 还能用），启动时的密钥扫描（`SecretStore::scanStoredHistoryOnce`、`SecretGuard::scrubStoredFiles`）照扫它 | — | 更早的 `lessons.json`（及迁移后的 `lessons.migrated.json`）同样不再读 |
| `capabilities.json` | `{entries: [ProviderEntry], disabledKinds: [], scriptEntries: [ProviderEntry]}` | `.iso8601`（实际无日期） | prettyPrinted + sortedKeys；ProviderEntry：id、type（dashscope / openAICompatible / volcengineArk / script）、title、options、enabled（缺省 true）、disabledKinds?（这个提供方关掉的能力 id，空时不写）；顶层 disabledKinds 一直写空数组（旧版本要求有这个键），旧版按能力整体关掉的会迁移到每个能做这项能力的提供方上；脚本提供方单独放在 scriptEntries（旧版本解不出 script 类型）；旧版的 assignments 键忽略 |
| `jobs.json` | `[CapabilityJob]`：id、remoteID、providerID、kind、prompt、createdAt、conversationID?、status、message?、media | `.iso8601` | prettyPrinted + sortedKeys；只留 50 个 |
| `engines.json` | `[EngineConfig]`：id、title、baseURL、apiKeySecret、model、supportsVision、supportsTools、sendsEnableThinking、extraBody、maxConcurrency?、thinking? | 无 | prettyPrinted + sortedKeys；非可选字段全写 |
| `secrets.json` | `{名字: 值}` | 无 | prettyPrinted + sortedKeys，权限 0600（Windows 上依赖用户目录 ACL） |
| `mcp.json` | `{mcpServers: {名字: {command?, args?, env?, url?, headers?, disabled?, autoApprove?}}}` | 无 | prettyPrinted + sortedKeys + withoutEscapingSlashes；用户手动编辑的原文原样保存；headers 是远程 server 的请求头（Cursor / VS Code 的写法），值里的 ${secret:…} 连接时替换；配置变了只重连变了的服务（MCPManager::sync） |
| `Tools/<名称>/tool.json` | ScriptToolManifest：name、title、description、symbol、permission、language、parameters、testArguments?、version、enabled、createdBy、createdAt、updatedAt、lastTest?{passed, output, date}、secrets? | `.iso8601` | prettyPrinted + sortedKeys + withoutEscapingSlashes；`.versions/vN/` 历史版本 |
| `Providers/<id>/provider.json` | ProviderManifest：id、title、detail、symbol、language、permission、online、capabilities（内置能力 id 或 definitions 里新能力的 id）、definitions?[CapabilityDefinition：id、title、symbol、detail、tool、description、parameters、paid]、options?[key、title、default、hint?、capability?、control?、choices?、minimum?、maximum?、step?、placeholder?、advanced?]、secrets?、check、timeout?、test?{capability, arguments}、version、createdBy（agent / user）、source?、createdAt、updatedAt、lastTest? | `.iso8601` | prettyPrinted + sortedKeys + withoutEscapingSlashes；入口 `main.py|sh|js`（0755）、`work/`（每次运行的产出在 `work/out-*/`，一天后清掉）、`.versions/vN/` 历史版本；`.fridayprovider` = 顶层是 `<id>/` 的 zip（不带 work、.versions、lastTest）；目录有变化（Agent 创建、导入、手动放进去）时自动重新载入 |
| `Media/<会话 UUID>/…` | 附件与生成结果 | — | 消息里只存相对路径（`/` 分隔） |
| `settings.ini` | UserDefaults 的替代 | — | 见 §5.4 |

`Conversation` 的键：id、title、createdAt、updatedAt、nodes（`[ChatMessage]`）、selectedChildren、engineID?、
workingDirectory?、permissionMode、thinking?、pinned；旧存档没有 nodes 只有 messages（线性数组）时按顺序补 parentID。
`ChatMessage`：id、role、blocks、createdAt、stats?、error?、notice?、parentID?、taskNote?（用户消息发出时的整份任务状态；交给模型时
按 `TaskState::Visible` 沿历史算出模型在没折叠部分里已看到的状态，`TaskState::renderedNote` 只附改动、没改不附，重建和发送逐字一致）、
folded?（上下文折叠标成 true 的较早消息，只写 true）。
`MessageBlock`：id、kind（带关联值的枚举：text / thinking / tool / media）。
`ToolEvent`：id、name、arguments、status、result、title?、symbol?、preview?、media?、transcript?、agentName?、queued?、
modelResult?（v1.18：后台子 Agent 结束、result 改成结论之前，模型当时拿到的结果）。
`MediaAttachment`：id、kind、relativePath、fileName、mimeType、byteCount、caption?、width?、height?。
`GenerationStats`：tokensPerSecond、tokenCount、promptTokenCount?、promptTokensPerSecond?、contextTokenCount?、live?、prefilling?、
startedAt?、engineName?。读入时把还标着 live 的统计清掉、空回复标「（生成被中断）」（== ConversationStore.init）。

**v2.0 之前的本地模型数据**（读的时候忽略，不报错）：会话里的 `localModelID`、设置里的 `defaultEngineID = "local"`
（当成没选过）、子 Agent 角色指定的 `"local"` 大脑（当成没指定，跟随主 Agent）、`capabilities.json` 里本地的三种提供方
（localVLM / localDiffusion / localVideo，读的时候丢掉）。v2.0 不再有 KV 缓存落盘：1.x 留在 `<缓存目录>/KVCache` 的文件
不读不写，也不自动删。

### 5.4 设置键（`settings.ini`，与 UserDefaults 同名同默认值）

| 键 | 默认 | 键 | 默认 |
|---|---|---|---|
| systemPrompt | `AppSettings::defaultSystemPrompt()`（存着的还是旧版默认 `previousSystemPrompts()` 时，启动时删掉这个键 = 换成新版默认） | showMenuBarIcon | true |
| enableThinking | false | defaultEngineID | （无；v2.0 之前的 `"local"` 当成没选过） |
| thinkingLevel | （无：上次在输入框选的深度思考档位，2.3 起；enableThinking 跟着写，两者不一致时以开关为准，开着按 medium） | | |
| requireApproval | true | defaultPermissionMode | auto |
| confirmPaidCalls | true | autoUpdate | true |
| workingDirectory | 用户主目录 | updateChannel | 当前构建的渠道 |
| collapsedConversationGroups | ""（侧边栏，换行分隔） | disabledTools | [] （ToolRegistry） |
| recentDirectories | []（最近选择的工作目录，最新的在前，最多 10 条） | disallowedSubAgentEngines | [] （ToolRegistry） |

v2.0 去掉的设置项（temperature、enableTools、searxngURL、subAgentMaxSteps、maxToolRounds）不再读写，设置文件里旧的键不删：
温度固定 `ChatEngine::defaultTemperature`（0.6，模型不认就自动去掉）、工具一直开着、只用内置搜索、主 Agent 和子 Agent 都不限工具轮数 / 步数
（引擎和子 Agent 限轮数 / 步数的机制还在，只是不从设置读）。

v2.0 之前的本地模型设置（modelID、downloadSource、maxTokens、autoDownloadModel、kvCacheEnabled、kvCacheMaxGB、kvCacheMaxDays）不再读写，
留在 settings.ini 里也没有影响。

---

## 6. 大脑：在线大模型 + 本地能力里的本地大模型（v2.0 / v2.7）

v2.7 起本地大模型、本地生图 / 生视频回来了，但不在 Friday 进程里：它们是 Friday.Powers 的 `llm`、`diffusion`（按需下载的本机服务），Friday 这边只有模型库（下载、选用）和 `power://llm` 引擎、能力层的 `PowerBackend`，见 POWERS.md §8。下面说的 v2.0 的取舍照旧：

v2.0 起不带本地推理。1.x 的本地模型（llama.cpp / stable-diffusion.cpp：模型下载与模型库、KV 缓存落盘、本地看图 / 生图 / 生视频、
按内存给本地子 Agent 排队）连同它们在 Swift 里的对应（MLX 的 `ModelManager`、`ModelLibrary`、`ModelCatalog`、`KVCacheStore`、
`LocalMLXEngine`、`Local*Backend`、`AppHeavyWorkCoordinator`、`SubAgentScheduler`）都没有了。与代码结构的对应：

| 事情 | Qt |
|---|---|
| 大脑 | `EngineStore` 里的在线引擎（`EngineConfig`：OpenAI 兼容接口的地址、密钥名、模型、能力开关…，存 `engines.json`），对话用 `OpenAIChatEngine`；本机自己跑的 OpenAI 兼容服务（Ollama、LM Studio…）也按在线引擎接入 |
| 选哪个 | 对话自己选的（`Conversation.engineID`）→ 新对话默认（`AppSettings::defaultEngineID`）→ 第一个接入的（`EngineStore::resolved`）；选的已删掉同样往后退 |
| 没有大脑 | 一个在线引擎都没接入：输入框发不出（`AgentRunner::canSend` 为假），占位文字、托盘状态行说「还没有接入在线大模型」，模型菜单只有「去设置里添加在线服务…」，在线服务在 设置 › 服务与密钥 里添加（`AgentRunner::noEngineMessage`）；Agent 请求单（`AgentRequestSheet`）提交的消息排队，接入后发出。接入了、但服务都关掉了（v2.3）同样发不出，占位文字和模型菜单说「在线服务都关掉了」、指到设置里打开 |
| 关掉的服务 | v2.3 设置 › 服务与密钥 每行的开关（`ServiceInstaller::setEnabled`）：这家的模型（`EngineConfig::disabled`）和能力提供方（`ProviderEntry::enabled`）一起关；配置、密钥留着。拿来用的地方只看开着的（`EngineStore::enabledEngines / enabledEngine / resolved`）：模型菜单、新对话默认、会话选的大脑（关掉了退到默认 / 第一个开着的）、子 Agent、助手、命令行；这家新加的模型跟着关着 |
| 子 Agent 的大脑 | 只在开着的在线引擎里选（设置 › 服务与密钥 的「子 Agent 可以用的服务」按服务开关，存的仍是引擎 id 的列表 `disallowedSubAgentEngines`；一家已有的模型都不给用时，这家新加的模型也不给用，`ToolRegistry::inheritSubAgentPermission`；v2.4 点开一家可以逐个模型开关）；角色没指定（或是以前的 `"local"`）就跟随主 Agent。v2.8 设置「子智能体用的模型」（`subAgentEngineID`）默认跟对话用的一样，不按活换、不认点名；选「按活自动挑」才按上面挑 |
| 看图 / 看视频 / 生图 / 生视频 / 听录音 / 说话 | 能力提供方：在线的 `Dashscope`（阿里云百炼）、`VolcengineArk`（火山方舟）、`VolcengineSpeech`（豆包语音，v2.3：只做听录音、说话）、`OpenAICompatible`，以及在本机运行的 `Script`（脚本提供方，`create_provider` 写或导入 `.fridayprovider`）。听录音（`transcribe_audio`）、说话（`speak`）是 v2.3 加的：百炼走 Qwen3-ASR / Qwen3-TTS（Token Plan 的 `sk-sp-` 密钥自动换到 `token-plan.maas.qianwenaiapi.com`，用 Qwen-Audio-3.0 的原生接口，见 ASSISTANT.md），豆包语音走录音文件识别极速版 / 语音合成大模型（新版控制台的 API Key 走 `X-Api-Key`，填了 APP ID 按旧版的 Access Token），OpenAI 兼容走 `audio/transcriptions` / `audio/speech`；长录音解成 16 kHz 切段识别、长文按句分段合成再拼起来（`AudioEncoding`）。解码 m4a / mp3、编 m4a 在界面进程用 `AudioCodec`（Qt Multimedia），没有界面的 friday-server 找 ffmpeg，都没有时只认 WAV（豆包识别只认 wav / mp3 / ogg，m4a 要先解） |
| 并发 | 每段在线请求过引擎的 `RequestGate`（§3.3） |

**上下文折叠**（== Swift f0a9b4b / 1c4401c，`agent/ContextFolding`）：发送前还没折叠的历史估算超过预算（48000；v2.0 之前本地模型
另有 12000 一档）就把最近一轮之前的消息标成 `folded`，`historyMessages` 把它们重建成摘要。折叠了的那一轮，在线引擎的会话作废
（`m_sessions` 删掉，== Swift `sessions[id] = nil`），按存档重建。两次折叠之间历史逐字不变，在线服务的前缀缓存一直有效。
上下文超长的兜底（`AgentRunner::compactHistory`，Qt 版新增）只在请求仍被拒绝（`ContextOverflowError`）时临时压缩这一次请求的历史，不改存档。

---

## 7. 文案与平台差异

界面文字、提示词、工具定义、错误信息**逐字照抄 Swift**。只有写着 macOS 特有事物的地方替换成当前平台的等价物，
替换词一律从 `Platform` 取：

| Swift 原文 | 替换 | 例子 |
|---|---|---|
| 「Mac」（指这台电脑） | `Platform::deviceWord()`（Windows / Linux：「电脑」） | 默认系统提示词「运行在用户电脑本地」、空状态建议「查一下这台电脑的芯片型号和内存」 |
| 「访达」 | `Platform::fileManagerName()`（资源管理器 / 文件管理器） | 「在访达中显示」→「在资源管理器中显示」 |
| 「macOS 的 zsh」 | `Platform::shell().title` | run_shell 的工具说明 |
| 「本地 MLX 模型」这类说本地模型的字样 | Qt 版没有本地模型（§6），改成在线大模型的说法 | 空状态副标题（v2.0 起空状态不再有副标题） |
| 「⌘N」等快捷键提示 | macOS 保留 ⌘，其余「Ctrl+N」（`App.shortcutModifier`） | 「新对话 (⌘N)」→「新对话 (Ctrl+N)」 |
| 「菜单栏」（MenuBarExtra） | Windows / Linux：「系统托盘」 | 设置 › 通用 的分组标题「系统托盘」、开关「在系统托盘显示 Friday 图标」 |
| 「下载 zip」「和 Developer ID 签名」（自动更新说明） | 包格式取 `App.updatePackageFormat`（zip / AppImage / tar.gz）；Qt 包没有代码签名，去掉签名那半句（只核对大小和 SHA-256） | 设置 › 通用「关于 Friday」的说明 |
| 「废纸篓」 | `Platform::trashName()` / `App.trashName()`（Windows / Linux：「回收站」） | 删除脚本工具的确认「工具目录（含历史版本）会移到回收站。」、delete_tool 的结果 |
| 新建 Bash 脚本工具模板里的 `plutil` | 非 macOS 没有 plutil：模板改用系统自带的 sed 取简单的字符串字段，注释同步改成「系统自带的 sed 可以取出简单的字符串字段」 | 设置 › 工具 › 新建脚本工具（Bash） |

其余一个字都不改。拿不准的在交付总结里列出来。

---

## 8. QML ↔ C++ 约定

### 8.1 总则

- QML 里 `import Friday`（核心单例、app 层类型、本模块的其他 QML 文件）和 `import Friday.Style`（设计系统）。
  要直接用 Qt 自带控件时写 `import QtQuick.Controls as QQC` 再 `QQC.Xxx`（不加限定会和 Friday.Style 的 `Button` / `Menu` /
  `TextField`… 重名）；原生控件不要覆盖 `background` / `contentItem` 等委托（STYLE.md §2）。
- **跨 QML/C++ 边界一律用 QML 友好的类型**：`QString`、`bool`、`int`、`double`、`QStringList`、`QVariantMap`、`QVariantList`、
  `QDateTime`。UUID 用大写字符串，枚举用原始字符串，可选字符串用空字符串表示 nil。
- C++ 方法本身就是这些类型时直接标 `Q_INVOKABLE`；否则另写同名重载（参数 `QString`，返回原始字符串 / `QVariantMap`），
  放在头文件的「// MARK: QML」一节。例：`AgentRunner::permissionMode(const QUuid&) -> PermissionMode`（C++）与
  `Q_INVOKABLE QString permissionMode(const QString&)`（QML）。极少数会歧义的另起名字（`ConversationStore.createConversation`）。
- 结构体给 QML 用 `toVariant()`：键与 JSON 键相同，另加展示字段（`title`、`symbol`、`summary`、`kindTitle`…）。
- 小集合（设置页的列表）用 `QVariantList` 属性 + NOTIFY，变化时整体替换；大列表 / 高频变化的列表用 `QAbstractListModel`（§8.4）。
- QML 绑定不会追踪函数调用里的依赖：`AgentRunner.permissionMode(id)` 在权限变了之后不会自动重算。
  需要随状态更新的值用属性（ChatViewModel 把会话相关的状态整理成属性），或在绑定里带上会变的 `revision` 属性。
  **revision 必须参与求值**：写成 `{ ToolRegistry.revision; return ToolRegistry.descriptorInfo(n) }` 时，qmlcachegen 把绑定
  编译成 C++ 后会删掉那句结果没被用到的读取，依赖随之丢失（实测：输入框占位文字停在「模型准备中…」不再更新）。
  写成 `ToolRegistry.revision < 0 ? ({}) : ToolRegistry.descriptorInfo(n)` 这类让它参与判断的形式。
- 图标：数据里存的是 SF Symbol 名（`ToolEvent.symbol`、`PermissionMode.symbol()`…），QML 写 `Icon { name: "photo" }`，
  设计系统负责映射到 Lucide SVG（`style/icons/mapping.json`）。缺映射的名字告诉设计系统补。
- 相对时间、字节数等用 `Format` 单例（与 core 的 `util/Format` 同一份实现）。

### 8.2 核心单例（main.cpp 用 `qmlRegisterSingletonType` 的回调注册，名字 = Swift 类型名）

| QML 名 | C++ 类 | 属性（均有 NOTIFY） | 主要 Q_INVOKABLE |
|---|---|---|---|
| `AppSettings` | AppSettings | systemPrompt、enableThinking、requireApproval、confirmPaidCalls、workingDirectory、showMenuBarIcon、defaultEngineID、defaultPermissionMode、autoUpdate、updateChannel、recentDirectories（全部可写）、defaultSystemPrompt | resetSystemPrompt() |
| `ConversationStore` | ConversationStore | selectedID（可写）、revision | createConversation(dir)、fork(convID, msgID)、togglePin(id)、rename(id, title)、conversationInfo(id) |
| `AgentRunner` | AgentControl（本机是 AgentRunner，`friday --remote` 时是 RemoteRunner；属性和 QML 方法都在 AgentControl） | isGenerating、generatingCount、stoppingCount、queueRevision、liveAgents、liveSpeed | send(text, attachments, convID, replacing)、stopGenerating()、resolveApproval(eventID, allow)、canSend(convID)、engineConfig(convID)、setEngine(engineID, convID)、permissionMode / setPermissionMode、thinkingLevel / thinkingLevels / setThinking、workingDirectory / setWorkingDirectory、selectBranch(child, parentKey, convID)、resetSession(convID) |
| `SpeechReader` | SpeechReader（app） | speakingMessageID | toggle(text, messageID)、stop() |
| `MCPManager` | MCPManager | servers（另带 isRemote、tools[].readOnly）、revision、configPath、homeDirectory | configText()、saveConfigText(text)（返回错误文字）、saveServerConfig(name, config, oldName)（返回错误文字）、parseServersText(text) → {ok, names, replaced, error}、importServersText(text) → {ok, names, error}、otherAppConfigs() → [{title, path}]、readTextFile(path)、missingSecrets(name)、setDisabled(name, bool)、setAutoApprove(name, bool)、remove(name)、reloadAll()、reconnect(name)、exampleConfigText() |
| `ToolRegistry` | ToolRegistry | scriptTools、disabledNames、disallowedSubAgentEngines、isSelfExtensionEnabled、isTeamEnabled、mainDelegatesOnly（可写，v1.10）、revision、toolsDirectory | setEnabled(name, bool)、descriptorInfo(name)、subAgentEngineAllowed(id)、allowSubAgentEngine(id, bool)、scriptTool(name)、scriptVersions(name)、saveScriptAsync(manifest, code)、testScriptAsync(name, argsJSON)、rollbackAsync(name, version)、deleteScriptTool(name)、reloadScripts()；信号 scriptSaved / scriptTested |
| `AgentJournal` | AgentJournal | entries | clear() |
| `SecretStore` | SecretStore | names、filePath、autoRevision | contains(name)、set(name, value)、remove(name)、masked(name)、validName(name)、normalize(raw)、autoInfo(name)、markNotSecret(name) |
| `CapabilityRegistry` | CapabilityRegistry | entries（另带 kinds、kindInfos、secretNames、active、isLocal、isScript、createdByAgent、symbol）、jobs、packages、packageProblems、revision、availabilityMap、providersDirectory | isEntryActive(id)、setEntryEnabled(id, bool)、setKindAllowed(id, kind, bool)、setEntryOption(id, key, value)、setEntryTitle(id, title)、removeProvider(id)、deleteJob(id)、kindsOf(id)、optionSpecsFor(id)、secretNamesFor(id)、packageFor(id)、importProviderFile(path) → {ok, id, title, version, replaced, permissionTitle, permissionDetail, error}、exportProviderFile(id, path) → 错误、deleteProviderPackage(id) → 错误、retestProvider(id)、refreshAvailability()；信号 providerTestFinished(id, error) |
| `EngineStore` | EngineStore | engines、revision、defaultID（可写：本机写 AppSettings.defaultEngineID，远程发 `settings.update`）、disallowedSubAgentEngines（本机是 ToolRegistry 的，远程是那台的） | updateEngine(map)、subAgentEngineAllowed(id)、allowSubAgentEngine(id, bool)（本机写 ToolRegistry，远程发 `engines.setSubAgentAllowed`；设置页「子 Agent 可以用的大脑」用它） |
| `UpdateManager` | UpdateManager | state、bannerText、statusText、actionTitle、isBusy、downloadProgress（-1 = 不在下载）、hasDraft（可写）、lastChecked、currentBuildTitle、isDevelopment | check(manual)、userTapped() |
| `Machines` | MachineSwitcher（app，v2.1） | currentID、isRemote、currentName、machines（[{id, name, subtitle, kind, icon, current, connected, working, attention}]）、settingUpKey、keySetupStatus、syncBusy、syncPreview、syncSummary | switchTo(id)、addRemote(text, name, identityFile) → 错误、addRemoteWithPassword(text, name, password) → 错误、cancelKeySetup()、disconnectRemote(id)、removeRemote(id)、prepareSync(engines, settings, mcp)、applySync()；信号 switched、keySetupFinished(ok, error)、syncPrepared(error)、syncApplied(error)（docs/REMOTE.md §11） |
| `App` | FridayApp（app） | aboutVersion、aboutBuild、platform、shortcutModifier、updatePackageFormat；v2.1 远程：isRemote、remoteName、remoteConnected、remoteStatus、remoteServer、remoteNotice；recentDirectories（当前机器的，看着远程时是远程自己的，REMOTE.md §5） | reconnectRemote()、wslDistributions()、pushRecentDirectory(path)、removeRecentDirectory(path)、recentDirectoryExists(path)、checkRecentDirectories()、showMain()、newConversation()、stopGenerating()、openSettings(page)、showAbout()、checkForUpdates()、quit()、revealInFileManager(path)、openFile(path)、openUrl(url)、fileManagerName()；信号 showMainRequested / settingsRequested(page) / aboutRequested |

各属性 / 方法的语义见对应头文件的注释（与 Swift 同名成员对应）。

`ConversationStore`、`AgentRunner`、`AgentTeam`、`SecretStore`、`EngineStore` 是「当前机器」的（`AppBackend::store()` 等）：
看着远程机器时回调给的是那台的 RemoteSession 里的副本。切换机器（docs/REMOTE.md §11）时新建一个 QQmlApplicationEngine
重新加载界面，单例回调重新取一次；用 `qmlRegisterSingletonInstance` 注册的实例只能给第一个引擎用，所以一律用回调。

### 8.3 app 层的 QML 类型（`QML_ELEMENT` / `QML_SINGLETON` 自动注册）

| QML 名 | 类型 | 用途 |
|---|---|---|
| `Enums` | 单例 | 枚举原始值 → 标题 / 图标 / 说明、全部取值 |
| `Format` | 单例 | 字节数、相对时间（绑定 `Format.tick` 定时刷新）、千分位、万 / 亿、shortName |
| `Clipboard` | 单例 | 复制文字 / 图片，粘贴文件 / 图片成附件，拖放 URL 转路径 |
| `Markdown` | 单例 | Markdown 分块 + 行内富文本（带缓存）；`estimateSize` 估占位的尺寸 |
| `LoadScheduler` | 单例 | 先占位、后台加载：DeferredLoader 排队，按离视口远近一批批建（每批 ≤ 8 ms，批间让出事件循环），`preloadScreens` 限定预载范围；`busy` 给测试用；`hasVisiblePending(视口)` + 每批之后的 `ticked` 给切页的骨架屏判断视口里的建好没有（STYLE.md §9） |
| `ModelMenu` | 单例 | 模型选择菜单树（各家在线服务 › 模型）与动作 |
| `ServiceCatalog` | 单例 | 在线服务目录、接入 / 移除 / 改模型、拉模型列表 |
| `SidebarModel` | 列表模型 | 侧边栏（分组 + 会话行） |
| `SearchResultsModel` | 列表模型 | 会话搜索结果 |
| `ChatViewModel` | 对象 | 一个聊天视图的状态（含 `messages: MessageListModel`） |
| `AgentTreeModel` | 列表模型 | 一段对话的 Agent 树（轮次 + 子 Agent 节点、面板汇总、工具栏按钮；`active` 时才算，160 ms 合并重建） |
| `PanelLayout` | 对象 | v2.2 一段会话的卡片布局（四边停靠、比例、大小；存进 settings 的 `panelLayouts`，TERMINAL.md §2） |
| `TerminalTabs` | 对象 | v2.2 一段会话的终端标签（当前机器的 TerminalControl） |
| `TerminalView` | 控件（QQuickPaintedItem） | v2.2 画一个终端、接键盘鼠标（TERMINAL.md §4） |
| `MessageListModel` | 列表模型 | 当前路径上的消息（v2.0 起倒序：第 0 行是最新的、排在最下面；v2.6 起 MessageList 按锚点排版，见 STYLE.md） |
| `ToolListModel` | 列表模型 | 设置 › 工具的分组列表 |
| 图片提供者 `image://thumb/<路径>?max=N` | — | 降采样缩略图（ThumbnailProvider） |

这些类型从 `FridayCore::instance()` 取核心对象，QML 里直接 `SidebarModel {}` 即可，不需要传依赖。

### 8.4 列表模型与流式更新

- ConversationStore 另有 `summaries()`（ConversationSummary：标题、时间、目录、模式……不含消息内容，== Swift 的 summaries），
  `conversationsChanged` 只在它真的变了时才发；侧边栏、输入栏、会话级设置只读这份，流式输出改消息内容时它们不重算。
- **SidebarModel**：roles `rowType`（header / conversation）、`groupKey`、`groupTitle`、`groupDirectory`、`expanded`、
  `conversationID`、`title`、`updatedAt`、`pinned`、`isGenerating`、`isSelected`。
- **SearchResultsModel**：`hitID`、`conversationID`、`messageID`、`title`、`snippet`。
- **MessageListModel**：`messageID`、`role`、`blockCount`、`revision`、`plainText`、`attachments`、`stats`、`error`、`notice`、
  `createdAt`、`isStreaming`、`dimmed`、`isWaitingForTool`、`lastIsTool`；方法 `block(row, i)`、`blockAt(row, path)`、`message(row)`、
  `rowOf(messageID)`、`showEarlier()`；信号 `messagesAppended(containsUser)`、`messageUpdated(row)`。
- **ToolListModel**：`rowType`（header / empty / tool）、`groupTitle`、`emptyText`、`name`、`title`、`symbol`、`description`、
  `permission`、`permissionTitle`、`originTitle`、`enabled`、`problem`、`isScript`。

**流式输出只刷新变化的那一行**（MessageListModel.h 文件头有完整示例）：

1. `AgentRunner` 每个 token → `store->updateMessage()`（主线程）→ 信号 `messageChanged(conv, msg)`；
2. `MessageListModel` 找到行，`revision += 1`，`dataChanged(row, {blockCount, revision, plainText, stats, …})`；
3. QML 消息行里块用**整数模型**：`Repeater { model: blockCount }`，每个块委托
   `property var block: revision < 0 ? ({}) : messages.block(row, index)`——新块出现时只多建一个委托，
   已有的块只有内容变了的 `Text` 会重新排版（设同样的字符串不触发）；
4. 子 Agent 的运行记录同理：`block.tool.transcriptCount` 当整数模型，`messages.blockAt(row, [i, j])` 按路径取；
5. 路径结构变化（发新消息、分叉、切分支）→ `pathChanged` → 末尾追加用 `beginInsertRows`，否则 reset。

`AgentRunner::renderPass` 复用原块 id（与 Swift 一样），界面据此保持委托不重建。

### 8.5 窗口、弹窗与菜单

- 两个窗口（主窗口、设置）都在 `Main.qml` 里用设计系统的 `FridayWindow` 声明，响应 `App` 的信号显示；
  关于面板 `AboutPanel.qml` 也是 `FridayWindow`（`resizable: false` 固定尺寸；Swift 是系统关于面板：图标、名称、
  版本 `App.aboutVersion`、构建 `App.aboutBuild`）。窗口都是**系统原生窗口**：最小化 / 最大化 / 关闭按钮、拉伸边框、
  Windows Snap、深色模式的深色标题栏都由系统给，界面里不画标题栏按钮、红绿灯或拖动区。Windows 11 上窗口顶部的工具栏行
  与系统标题栏合成一行（微软的 Custom Window Frame Using DWM：按钮、Snap Layouts、系统提示仍由 DWM 画；原生代码
  `src/app/platform/WindowsTitleBar` 做命中测试与 DWM 设置，并把 Qt 不画的 GDI 重定向表面涂成窗口底色，开窗不闪白），
  Windows 10、Linux、macOS 开发机和 `FRIDAY_TITLEBAR=system` 时是系统标题栏 + 下面的工具栏行（Windows 上同样防开窗白闪，图片预览窗口也是）；
  窗口菜单（右键标题栏、Alt+Space）跟随深浅色，Alt+Space 在合并时弹在标题行下面；见 `docs/STYLE.md`「标题栏」。
- Swift 的 `.sheet` / `.alert` / `.confirmationDialog` → 设计系统的 `Sheet` / `Alert`（原生 `QQC.Popup` / `QQC.Dialog` 的包装）；
  `Menu` / 右键菜单 → `Menu`（含级联子菜单）/ `ContextMenuArea`；`.help(…)` → `Tooltip` / `HelpArea`（原生 `QQC.ToolTip`）。
- 文件 / 目录选择用 `QtQuick.Dialogs` 的 `FileDialog` / `FolderDialog`（原生对话框）。
- 快捷键：`Shortcut { sequences: ["Ctrl+N"] }`（macOS 上 Ctrl 自动对应 ⌘）：新对话 Ctrl+N、停止生成 Ctrl+.、设置 Ctrl+,、搜索 Ctrl+F。
- 菜单命令：macOS（开发机）是原生菜单栏；Windows / Linux 没有菜单栏，主窗口顶部一行最左边是应用菜单（☰，`qml/AppMenuButton.qml`，
  放在 NavigationSplitView 的 `leadingToolbar` 里；点按钮或按 F10 打开）：新对话、停止生成、—、设置…、—、检查更新…、关于 Friday、—、
  退出 Friday，调用的与托盘菜单 / 快捷键是同一套 `App` / `ConversationStore` 方法，快捷键只显示提示。
- 托盘菜单是原生 `QMenu`（FridayApp）；应用内的菜单一律用设计系统的 `Menu`（v2.0 起自绘，见 `docs/STYLE.md`）。
- 设置：v2.0 起是主窗口里的 sheet（`qml/settings/SettingsSheet.qml`，== Swift 的 SettingsPresenter + ContentView 的 `.sheet`）。
  `App.openSettings(page)` 先把主窗口带到前面再弹出（托盘 / 应用菜单的「设置…」、Ctrl+,、检查更新…、模型菜单的「添加在线服务…」），
  `App.dismissSettings()` 关掉（「让 Agent 做…」发出请求后）。5 页：general / services / agent / tools / capabilities，旧页名
  secrets → services、mcp → tools、subAgents → agent。
- v1.9 菜单栏计量（Swift 在菜单栏图标右边画「N AI / 速度」两行）：托盘图标放不下字，图标不变，tooltip 是三行「Friday / N AI / 速度」，
  托盘菜单状态行下面一行禁用项「N AI 在工作 · 速度」（有 Agent 在工作时）；跟随 `AgentRunner::liveMeterChanged`（每秒最多一次）。
- 关窗口：托盘图标开着时关闭主窗口不退出（== macOS 的菜单栏应用）；托盘关掉时关闭最后一个窗口即退出。

---

## 9. Swift → Qt 文件对照

| Swift | Qt |
|---|---|
| App/FridayApp.swift | core/FridayCore（init）、app/main.cpp、app/FridayApp（菜单 / 托盘 / 关于 / 窗口）、qml/Main.qml、qml/AboutPanel.qml |
| Models/AppSettings.swift | core/models/AppSettings |
| Models/ChatModels.swift | core/models/ChatModels |
| Models/ConversationStore.swift | core/models/ConversationStore |
| Models/MediaStore.swift | core/models/MediaStore |
| Models/SpeechReader.swift | app/SpeechReader |
| Secrets/SecretStore.swift | core/secrets/SecretStore |
| Secrets/SecretGuard.swift | core/secrets/SecretGuard（v1.16：入口 AgentRunner::submit / send / dispatch / sendToAgent、看图描述；执行 ToolRegistry::execute；出口 OpenAIChatEngine 调 transport 前；启动 FridayCore 先载入密钥库再清理存档） |
| LLM/Engine/ChatEngine.swift | core/llm/engine/ChatEngine |
| LLM/Engine/EngineStore.swift | core/llm/engine/EngineStore |
| LLM/Engine/OpenAIChatEngine.swift | core/llm/engine/OpenAIChatEngine |
| LLM/ModelListFetcher.swift | core/llm/ModelListFetcher |
| LLM/OnlineServices.swift | core/llm/OnlineServices（OnlineService、OnlineServices、ServiceSettings、ServiceInstaller；显示名 / 接口地址 / 密钥名是服务一级的设置） |
| Agent/AgentJournal.swift | core/agent/AgentJournal（v1.10 起只有 AgentJournal；经验库 LessonStore 换成了学习记录，v2.0 学习整个去掉） |
| Agent/DeliveryCheck.swift | core/agent/DeliveryCheck（框架的交付检查：只交了方案、写完没验证、没交结果；主 Agent 与子 Agent 自查的提示词） |
| Agent/AgentProfiles.swift | core/agent/AgentProfiles（AgentProfile、AgentTeamTools：delegate_task、run_parallel_tasks、read_agent_log、message_agent、stop_agent、wait_agents、report_to_parent 的定义、拆小规则与子 Agent 提示词开头） |
| Agent/AgentTeamControl.swift | core/agent/AgentTeamState（团队状态）、core/agent/AgentTeamControl.cpp（AgentRunner 的停止 / 发消息 / 让出名额 / 派发检查 / 上级链 / 通知文字） |
| Agent/AgentLiveness.swift | core/agent/AgentLiveness（SubAgentEnded；AgentRunner 的状态文字、看门狗、期限、原地打转、半成品） |
| LLM/Engine/RequestGate.swift | core/llm/engine/RequestGate |
| Agent/AgentRunner.swift | core/agent/AgentRunner、core/agent/ToolSink |
| Agent/AgentTools.swift | core/agent/AgentTools |
| Agent/WebSearch.swift | core/agent/WebSearch（请求地址 / 结果整理拆成 requestURL、format 便于测试；Docker 部署命令按平台，见 §7） |
| Agent/WorkingMethod.swift | core/agent/WorkingMethod |
| Agent/TaskState.swift | core/agent/TaskState（update_task_state 的执行是 AgentRunner::updateTaskState，在 ContextFolding.cpp；附给子 Agent 的主任务背景 subAgentNote 在 SubAgentRunner.cpp 里用） |
| Agent/ContextFolding.swift | core/agent/ContextFolding（含 extension AgentRunner 的 foldHistoryIfNeeded / currentPath / updateTaskState / readAgentLog；见 §6「上下文折叠」） |
| Agent/TurnCheck.swift | core/agent/TurnCheck（TurnCheck：验证检查点的规则，执行在 AgentRunner::verifyIfNeeded；AgentLog：按 id 找子 Agent 的运行记录并整理成文字。Windows 上多认 PowerShell / cmd 脚本，路径里的「\」也当分隔符） |
| Agent/SelfTests.swift | tests/tst_context.cpp（TestContextAssist：任务状态、折叠摘要、子 Agent 背景、TurnCheck、AgentLog）与 tests/tst_runner.cpp 的折叠 / 任务状态 / read_agent_log / 验证检查点用例（Swift 是调试构建启动时 FRIDAY_SELFTEST 跑的自检，Qt 版就是单元测试） |
| Agent/MetaTools.swift | core/agent/MetaTools |
| Agent/ScriptTools.swift | core/agent/ScriptTools |
| Agent/ShellEnvironment.swift | core/agent/ShellEnvironment |
| Agent/SubAgentRunner.swift | core/agent/SubAgentRunner |
| Agent/ToolRegistry.swift | core/agent/ToolRegistry |
| Agent/ToolTypes.swift | core/agent/ToolTypes（AgentToolError 在 util/Errors） |
| Agent/RefreshThrottle.swift | core/agent/RefreshThrottle（工作线程请求、主线程补刷，见头文件） |
| Agent/DebugHooks.swift | core/agent/DebugHooks（调试构建：FRIDAY_AUTOSEND、FRIDAY_AUTOSEND_FILE（多轮自动发送）、FRIDAY_PERF_STRESS、FRIDAY_REVIEW_AFTER、FRIDAY_MAINTAIN_AFTER（v1.10）、FRIDAY_DUMP_CONTEXT（v1.18，导出下一轮的上下文，和 Swift 同格式可逐条比对）、FRIDAY_DUMP_SUBAGENTS（v1.19，按引擎的写法重放存档里每个子 Agent，算不折叠 / 折叠时每次请求的大小）；对照开关 FRIDAY_NO_CONTEXT_ASSIST 在 AgentRunner::contextAssistEnabled；FRIDAY_FOLD_BUDGET=<token 数>（把折叠预算调低，几轮就触发折叠，每次发送时读）在 ContextFolding::budget；FRIDAY_SELFTEST 见 SelfTests 一行） |
| Capabilities/Capability.swift | core/capabilities/Capability |
| Capabilities/CapabilityRegistry.swift | core/capabilities/CapabilityRegistry |
| Capabilities/ScriptProviders.swift | core/capabilities/ScriptProviders（.fridayprovider 用 QZipReader / QZipWriter 读写，解压时拒绝越出目标目录的路径） |
| Capabilities/HTTPClient.swift | core/capabilities/HTTPClient（建在 util/Http 上） |
| Capabilities/MediaEncoding.swift | core/capabilities/MediaEncoding、app/VideoFrameGrabber（录音的 core/capabilities/AudioEncoding、app/AudioCodec 是 v2.3 新加的） |
| Capabilities/Providers/OpenAIStyleAPI.swift | core/capabilities/providers/OpenAIStyleAPI |
| Capabilities/Providers/OpenAICompatibleBackend.swift | core/capabilities/providers/OpenAICompatibleBackend |
| Capabilities/Providers/ArkBackend.swift | core/capabilities/providers/ArkBackend |
| Capabilities/Providers/DashScopeBackend.swift | core/capabilities/providers/DashScopeBackend |
| MCP/MCPManager.swift | core/mcp/MCPManager、core/mcp/McpClient（替代 MCP Swift SDK） |
| MCP/MCPServerSearch.swift | core/mcp/MCPServerSearch |
| Update/ReleaseVersion.swift | core/update/ReleaseVersion |
| Update/UpdateManager.swift | core/update/UpdateManager |
| Views/ContentView.swift | qml/ContentView.qml、qml/sidebar/SidebarView.qml、ConversationRow.qml、ConversationSearchSheet.qml、viewmodels/SidebarModel、SearchResultsModel |
| Views/ChatView.swift | qml/chat/ChatView.qml、MessageList.qml、EmptyStateView.qml、viewmodels/ChatViewModel（v2.0 分叉切换在 MessageRow 里） |
| Views/ComposerView.swift | qml/chat/ComposerView.qml（及拆出的胶囊组件） |
| Views/GrowingTextView.swift | qml/chat/GrowingTextView.qml、app/ClipboardBridge |
| Views/ModelStatusView.swift | qml/chat/ModelStatusView.qml、viewmodels/ModelMenuBuilder |
| Views/MessageRow.swift | qml/chat/MessageRow.qml、ThinkingCard.qml、ToolCard.qml、SubAgentTranscript.qml、SecretRequestPanel.qml、StatsLabel.qml、TypingIndicator.qml、viewmodels/MessageListModel |
| Views/AgentTreePanel.swift | qml/agenttree/（AgentTreePanel、AgentPanelSummary、AgentTreeRow、AgentNodeCard、AgentDetailView、AgentTreeButton）、qml/chat/ModelPill.qml、AgentStatusIcon.qml、AgentControls.qml、ChildAgentChips.qml、core/agent/AgentTree（纯函数）、viewmodels/AgentTreeModel |
| Views/PanelDock.swift | qml/dock/PanelDock.qml、CardDragArea.qml、viewmodels/PanelLayout |
| Views/TerminalCard.swift、Terminal/TerminalStore.swift、Terminal/TerminalTools.swift | qml/terminal/TerminalCard.qml、TerminalButton.qml、app/terminal/、core/terminal/、core/remote/RemoteTerminals |
| Views/MediaViews.swift | qml/media/MediaGallery.qml、MediaView.qml、VideoBox.qml、ImageThumbnail.qml、AttachmentChip.qml、ImagePreview.qml、app/ThumbnailProvider |
| Views/MarkdownText.swift | qml/chat/MarkdownText.qml、CodeBlock.qml、viewmodels/MarkdownRenderer |
| Views/SettingsView.swift | qml/settings/SettingsSheet.qml、GeneralSettings.qml、AgentSettings.qml、style/SheetHost.qml |
| Views/UpdateBanner.swift | qml/UpdateBanner.qml、qml/settings/AboutSection.qml |
| Views/ToolsSettingsView.swift | qml/settings/ToolsSettingsView.qml、ToolRow.qml、ToolDetailView.qml、ScriptToolEditor.qml、NewScriptToolSheet.qml、ToolBadge.qml、viewmodels/ToolListModel |
| Views/MCPServerViews.swift | qml/settings/MCPServerRow.qml、MCPStatusIcon.qml、MCPServerDetailView.qml、MCPServerForm.qml、MCPAddServerSheet.qml、MCPPasteConfigSheet.qml、MCPConfigEditorSheet.qml |
| Views/AgentRequestSheet.swift | qml/settings/AgentRequestSheet.qml |
| Views/CapabilitySettingsView.swift | qml/settings/CapabilitySettingsView.qml、ProviderRow.qml、ProviderIcon.qml、ProviderDetailView.qml、ProviderOptionField.qml、ProviderSourceSection.qml |
| Views/ServicesSettingsView.swift | qml/settings/ServicesSettingsView.qml |
| Views/ServiceGalleryView.swift | qml/settings/ServiceGallerySheet.qml、BrandMark.qml、ServiceCard.qml、CustomServiceCard.qml、AddServiceForm.qml、CustomServiceForm.qml、ServiceFieldRow.qml、viewmodels/ServiceCatalog |
| Views/ServiceModelPicker.swift | qml/settings/ServiceModelPicker.qml、ServiceModelsSheet.qml |
| Views/EngineEditor.swift | qml/settings/EngineEditor.qml |
| Views/GlassBackdrop.swift | 设计系统：窗口的纯色底（FridayWindow 的 `Theme.windowBackground`，= 系统窗口色） |

1.x 的本地模型（MLX 的 LocalMLXEngine、ModelManager、ModelLibrary、ModelCatalog、KVCacheStore、HubAdapters、ModelScopeDownloader、
SubAgentScheduler、Local*Backend、ModelBrowserView，Qt 侧对应的 llama.cpp / stable-diffusion.cpp 实现）v2.0 起删除，不在表里（§6）。

---

## 10. 自动更新

与 Swift 一致（每小时读 `latest.json`、渠道规则、空闲时自动换包、工作时横幅提示、开发版只提示），差别：

- 选包：`ReleaseEntry.assets[CurrentBuild::assetKey()]`（`windows-x64`、`linux-x64`；1.x 带后端的老键由 Friday.build 指向同一套包），
  按安装方式取条目本身 / `installer` / `deb` / `tarball`（`CurrentBuild::packageFormat()`）；
  没有本机的键 = 这个版本没有你的平台的包，保持现状不报错（PACKAGING.md §4）。
- 校验：大小 + SHA-256（没有代码签名）。
- 换包：Windows 便携版 PowerShell 脚本换目录、安装版以管理员身份静默跑安装程序；Linux AppImage 原子替换 / tar.gz 换目录 /
  deb 经 pkexec 安装（PACKAGING.md §5）。安装版和 deb 要管理员确认，只在用户点了时装。换包脚本启动成功后保存会话、结束 MCP
  （`beforeRelaunch`）再退出。

---

## 11. 命令行版（friday 带命令参数）

v2.8 起只有一个程序 `friday`（用户定 2026-10-11：「friday-cli 只要是带命令参数的就不启动界面」）：桌面端 `src/app/main.cpp` 一开头问
`FridayCli::requested(argc, argv)`——带了命令参数就交给 `FridayCli::run`（`src/cli/FridayCli.cpp`，静态库 friday_cli），不建 QGuiApplication、不开窗口；
不带参数、或者第一个参数是窗口自己用的（`--remote`、`--takeover`、`--geometry=`、`--select=`，以及 `-platform`、`-psn_…` 这类单横线的 Qt / 系统参数，
`FridayCli::isWindowArgument`）才开图形界面。`friday-headless`（`src/cli/main.cpp`）是同一套命令行、不链 Qt Quick，进服务端包。

- `friday server <选项>`（或者程序文件叫 `friday-server`）是远程 Friday 的常驻服务（§11a、`ServerCommand.h`），不碰终端；
  其余是命令行里的对话（下面这些）。v2.6–v2.7 是单独的 `friday-cli`（v2.6 之前服务另有 friday-server）。
- `friday <目录> [提示词]`：第一个是已有目录的参数当新会话的工作目录（同 `--cwd`），终端里 `friday .` 就在当前目录干活；`friday chat …` 同 `friday …`、`friday help` 同 `--help`。
- Windows：friday.exe 是 GUI 子系统，命令行模式下 `AttachConsole(ATTACH_PARENT_PROCESS)` 接上终端；终端里敲 `friday` 先找到的是同目录的
  `friday.com`（`src/console/`，控制台程序）：把参数和标准句柄原样交给 friday.exe、等它结束、带回退出码，这样 cmd / PowerShell 会等它、管道和重定向照常。

同一套核心（`FridayCore`，不开自动更新），终端里交互：

- 主线程跑 `QCoreApplication` 事件循环（核心对象都在主线程）；**终端读写放在一个工作线程里**（读 stdin 是阻塞的，
  放主线程会卡住 `runOnMainSync`）。
- 用法（实现包定稿）：`friday [目录] [提示词]`（一次性问答，回复流式打印到 stdout）、只给选项或目录不给提示词进入交互模式（不带任何参数是开图形界面）；
  `--conversation <id>` 继续某段会话、`--new`、`--list`；`--engine <id>`（在线引擎的 id 或名称）；
  `--cwd <目录>`（新会话的工作目录）、`--permission auto|plan|full`、`--thinking off|low|medium|high|ultra`、`--attach <文件>`、
  `--yes`（审批一律允许）、`--json`（事件流输出 JSON 行）。还没有接入在线大模型时说明原因（在图形界面的 设置 › 服务与密钥 里添加在线服务）。
- Ctrl+C 只取消生成。
- 后台子 Agent（v1.15 起主 Agent 派的第一层都在后台，主 Agent 交代完这一轮就结束）：一轮说完后还有子 Agent 在跑、或交回的结果
  还没送到主 Agent，就接着等——照样打印它们的工具调用、问审批，送结果新开的那几轮也接着打印（`--json` 每轮一个 `done`），
  都完了才回到提示符 / 退出；不等的话一次性问答进程一退，后台的子 Agent 跟着没了。等的时候 Ctrl+C 连它们一起停。
- 审批在终端里问「允许执行？[y/N]」；`request_secret` 用不回显的输入；思考内容灰色显示；工具卡简化成一行状态。
- 数据目录与 GUI 相同（可以用 `FRIDAY_DATA_DIR` 指到别处）。同一个数据目录同时只开一个 Friday（`util/InstanceLock`：
  `<数据目录>/friday.lock`）：两边各自在内存里持有会话、整份写回 conversations.json，同时开会互相覆盖。
  CLI 拿不到锁就说明原因退出（`--list` 只读，不拿锁）；再次启动 GUI 时请已经在运行的那个把窗口拿到前台（本地服务，名字按数据目录区分）。

---

## 11a. 远程（v2.1）

协议、部署、生命周期见 [`REMOTE.md`](REMOTE.md)。代码上的要点：

- **服务端** `friday server --daemon`（远程装的叫 `friday-server --daemon`）= `FridayCore` + `RemoteServer`（`src/core/remote/`）：拿数据目录的锁，本地 socket 只对同一个用户开放；
  请求在主线程直接调核心对象，核心对象的信号先记脏，每 80 毫秒合并一次推给客户端（发之前和上次发给这个客户端的比，没变不发）。
- **客户端** `friday --remote <目标>`：main.cpp 先把 `FRIDAY_DATA_DIR` 换成 `<本机数据目录>/Remotes/<目标>`（设置、媒体缓存、单实例锁分开），
  照常建 `FridayCore` 但不 `start`，再建 `RemoteSession`：`ConversationStore` 的副本（`makeReplica`）、`RemoteRunner`（实现 `AgentControl`）、
  团队快照（`AgentTeamState::applyReplica`），本机的 `EngineStore` / `SecretStore` 改成转发（`remoteUpdate` / `remoteSet`）。
  界面层（ViewModel、FridayApp）一律从 `AppBackend` 取会话和 Agent，不直接用 `FridayCore::runner / store`。
- 业务规则（能不能发、能不能重试、生效的大脑…）由服务端算好放在 Runtime 里，客户端只显示；Swift 客户端、手机 App 照这个做。
- **桌面端兼当服务端**（REMOTE.md §12）：`DesktopService` 在桌面端的 FridayCore 上开 RemoteServer（`setDesktopHost(true)`）；
  退出由 `FridayApp::quit()` 交给它（有任务就收起窗口跑完再退；v2.8 起退出就全停，不再起后台服务——关掉窗口程序本来就在托盘里）。
  启动时锁被后台服务拿着：main.cpp 用 `remote/LocalService`（friday server 的 --stop、--status 等管理命令也用它）请它退出，它在忙就进 `StandbyTakeover`（「本机」换成 `RemoteTarget::Kind::Local` 的 RemoteSession）。
  起常驻进程统一用 `util/DetachedLaunch`（只继承指向 NUL 的标准句柄、脱离作业）。

---

## 12. 测试

`tests/`：`TestRegistry.h` + `main.cpp` + 每个包一个 `tst_<包>.cpp`（`FRIDAY_TEST(类名)` 登记）。main.cpp 把数据 / 缓存目录
指到临时目录。重点测：JSON 与 Swift 同构（`tests/fixtures/` 里手写的 Swift 格式样例：UUID 大写、日期无毫秒、带关联值的枚举、
缺字段的旧存档）、ReasoningParser、版本比较与标签解析、工具参数预检、历史重建、SSE 解析（本机临时 HTTP 服务，
参考 `tst_util.cpp` 的 TinyServer）、任务状态与上下文折叠、子 Agent 背景、验证检查点与运行记录（`tst_context.cpp`，== Swift 的 SelfTests.swift）。**不要把真实用户数据拷进 fixtures。**
只在调试构建里生效的用例（调试钩子、FRIDAY_SIMULATE_FAILURE）在发布构建里跳过：要跑它们就另配一个不带 `/DNDEBUG` 的构建目录
（如 `-DCMAKE_BUILD_TYPE=Release "-DCMAKE_CXX_FLAGS_RELEASE=/O2 /Ob2"`，仍链接发布版 Qt）。
