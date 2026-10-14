# 本地能力（Powers，v2.3）

用户要求（2026-10-08）：「开发一个能力使用本地 GPU 模型做实时 stt，识别到一句就开始分析和边写边读」；
这类能力**按需下载、可以热加载，不集成到平台里**，代码放在独立仓库 [Friday.Powers](https://github.com/integemjack/Friday.Powers)
（本机克隆在仓库根目录的 `powers/`，被 `.gitignore` 忽略）。Power 的规范、协议见那边的 `docs/power-spec.md`、`docs/voice-protocol.md`。

Swift 版同样做了（Apple 芯片的 Mac 用 `macos-arm64-metal` 包），见 §7。

## 1. 用户看到的

- **设置 › 能力**：列出 index.json 里这台机器能用的 Power（第一个是「本地语音」）。每个有：
  - 状态和进度条（下载包、下模型，显示「已下 / 共要下」）、「安装（1.1 GB）」「更新到 x」「取消」「启动 / 停止」。
  - 版本、平台（Windows 有 Vulkan 和 CUDA 两种包可选，换平台就是重新下包，模型共用）、能做什么、启用开关、卸载。
  - v2.7 每块可以收起（标题旁的箭头）：收起时只留「状态」一行（装了但关掉的这里有「启用」）；没装、关掉的默认收起，展开了里面的选项
    （模型、语音模式、声纹）也是灰的，只能装、开、选平台、卸载；用户点过的按点的（这次打开设置期间），在块里关掉、卸载时不跟着收起。
  - 本地语音多一个「语音模式里助手说话时也听你说：你说完一句交给它时才打断它」（v2.5，默认开）；voice 0.2 起还有「声纹」：登记 / 重新登记 / 删除、
    「语音模式只听你的声音」（默认开）、严格程度（宽松 / 标准 / 严格），见 §4.1。
- **装在哪台就在哪台管**（2026-10-08 用户：「本地能力是不是应该在哪个服务器上管理哪个服务器的本地能力」）：本地能力用的是装它那台的显卡，
  那台的 Agent 听录音 / 说话、它当助手主机时的通话都用它。所以侧边栏底部切到一台远程机器时，这一页管的是那台的（标题「xx 上的本地能力」）：
  在那台上下载、安装、更新、开关、卸载，经协议 `powers.*`（REMOTE.md §6「本地能力」；页面开着时订阅那台的进展，`PowersWatch` 建出来时订阅、
  析构时退订）。QML 的 `Powers` 单例跟着当前机器换：本机是 `PowerManager`，远程是 `RemotePowers`（同一个接口 `PowersControl`；Swift 是
  `PowersSource`：`PowerManager` / `RemotePowers`）。助手页的语音模式在这台电脑上听，用的是这台的本地能力，所以插话、声纹这几项只在看本机时列出。
  那台的 Friday 太旧（`server.features` 里没有 `powers`）或没连上时说明原因，按钮都不能点。
- **能力层**：装好的 Power 是「本机」提供方（`power-<id>`，类型 `power`），「听录音」「说话」自动优先用它；用到时才拉起。
- **助手页的麦克风**（工具栏，装了本地语音才出现）：语音模式，见 §4。
- **助手通话**：装了本地语音时，听写走本机（能力层自动选本地），合成走本地流式（§5）。

## 2. PowerManager（`core/powers/`）

- **索引**：`https://github.com/integemjack/Friday.Powers/releases/download/index/index-v2.json`（v2.7；取不到再取老的 `index.json`，
  那里只有 schema 1 的 Power，2.6 及以前的 Friday 读它）。`FRIDAY_POWERS_INDEX` 可以指别的地址或本地文件，包和模型的地址可以是 `file://`，
  本机测试用。启动 20 秒后取一次，之后每 6 小时；取到的存一份当缓存，离线时用。
- **平台**（`PowerPlatform::candidates`）：Windows x64 → `windows-x64-vulkan`（默认），有 `nvcuda.dll` 时再加 `windows-x64-cuda13`
  （v2.7.1 起；之前自带 cuBLAS 的 `windows-x64-cuda` 也认，装着的有新版本时换成 `windows-x64-cuda13`，见下面「共用运行库」）；
  Apple 芯片 → `macos-arm64-metal`；Linux x64 → `linux-x64-vulkan`，有 libcuda 时再加 `linux-x64-cuda`（v2.7）；Linux arm64 看
  `/etc/nv_tegra_release`：R36 → `linux-arm64-jetson-orin`，否则 `linux-arm64-vulkan`（v2.7 起不再支持 Jetson Nano）。别的平台不显示本地能力。
- **目录**（`Paths::powersRoot()`）：Windows `%LOCALAPPDATA%\Friday\Powers`（模型上 GB，不放会漫游的 `%APPDATA%`），其他 `dataDir()/Powers`，
  `FRIDAY_POWERS_DIR` 可以覆盖。`state.json`（装了哪些、版本、平台、开关）、`index.json`、`<id>/<版本>-<平台>/`（解开的包）、
  `<id>/models/`（模型，各版本共用；`<文件>.sha256` 记已校验过的值，免得每次都算 1 GB 的哈希）、
  `runtimes/<id>/<版本>/`（共用运行库，`.sha256` 记包的校验值，有它才算装好）。
- **共用运行库**（v2.7.1，`index-v2.json` 的 `runtimes`，见 Friday.Powers 的 docs/power-spec.md §1.1）：平台包写着 `"runtime": "cuda13"` 时，
  本机还没有这个版本就先下它（`PowerPlatform::runtimeKey()` 挑本机的包，几个 Power 一起装时一次只装一个、拿到锁再看一眼），
  `state.json` 记 `runtimeDir`；拉起这个 Power 时把目录加到 `PATH`（Linux 是 `LD_LIBRARY_PATH`）最前面。索引里没有它要的运行库（或没有本机的包）
  的平台包不列、不装。装好、卸载、进程停了都清一遍：没有装着的 Power、也没有在跑的进程用的版本删掉（有在装的先不动）。
  现在只有 `cuda13`（Windows 上 CUDA 版共用的 cuBLAS，约 380 MB；以前 llm / diffusion / voice 的 CUDA 包各带一份）。
- **安装**（Worker 线程）：下包（`Http::download` 断点续传）→ 比大小和 SHA-256 → 解开（Windows 用系统自带的 `tar.exe`，它也认 zip）→
  改名到 `<版本>-<平台>/` → 下缺的模型（同样校验）→ 主线程记进 `state.json`、同步能力层的条目。新版本装好时：没人在用就马上停掉旧进程换新的，
  有人在用（语音模式、通话 `retain` 着）就等用完。旧版本目录在进程停了之后删。索引里出了新版本自动在后台更新。
- **进程**：`<exe> serve --models <models> --host 127.0.0.1 --port 0 --token <每次随机> --parent-stdin`（参数模板来自清单），
  从 stdout 读 `{"event":"listening","url":…}`；Windows 用作业对象（Friday 没了它跟着没），各平台关 stdin 它自己退出，3 秒没退就 kill。
  两分钟没就绪算失败（stderr 最后几行里的错误给人看）。闲置 15 分钟（没请求、没人 retain）停掉，把显存还回去。
- `ensureRunning(id)`（工作线程）：没在跑就拉起，等到就绪，返回地址和令牌。

## 3. 能力层（`PowerBackend`）

- 新的提供方类型 `power`（`ProviderType::Power`，`isLocal`、`isManaged`：不能手动建，create_provider 也拒绝）。条目由 PowerManager 同步
  （`CapabilityRegistry::syncPowerProviders`）：装了就有、卸载就删；能做什么写在条目的 `capabilities` 选项里（工作线程里读也安全）；
  用户在 设置 › 工具 里的开关保留。**单独存在 `capabilities.json` 的 `powerEntries`**（同 `scriptEntries`：旧版本、Swift 版不认识这个类型）。
- 执行：`ensureRunning` 拿到地址和令牌，交给 `OpenAICompatibleBackend`（Power 提供 OpenAI 兼容的 `audio/transcriptions`、`audio/speech`）；
  说话一律要 wav（Power 不出 mp3），要别的格式再转 m4a。

## 4. 语音模式（`AssistantVoice` + app 的 `VoiceMode`）

- `RealtimeTranscriber` / `RealtimeSpeaker`（`core/powers/RealtimeVoice`）：本地语音的两条 WebSocket；连上之前喂的音频 / 文字先攒着
  （Power 冷启动一两秒，开口的那几个字不会丢），用着时 retain。
- `AssistantVoice`（core）：麦克风的 PCM → 实时听写（字幕）→ 一句的定稿交给 `AssistantRuntime::say(text, "voice")`（来源 voice 大脑就按口语回）；
  来源是 `voice` 的助手回复（`entryAdded` / `entryChanged`）按增量交给边写边读，写完 `flush`；音频经 `audio` 信号交给界面。
  状态 off / starting / listening / hearing / thinking / speaking。
  - 打断：「停」（`interrupt`：取消合成、丢掉没播的、助手正在想或写这句就 `interrupt()`）。
  - **念的时候也听**（v2.5，`listenWhileSpeaking`，设置里的开关，默认开；用户 2026-10-09「我说话不打断，等我这话交给 llm 的时候再打断」）：
    助手在念、在想时你开口，它照常念；这句说完、听写出来、要交给助手的那一刻才打断——不念了（`stopSpeech`），正在想的由 `say` 取消、
    带上新话重来。没有实在的字（两个字以上；噪音、咳嗽、「嗯」）不交、不打断。外放时麦克风会听到助手自己：念的时候（或播完半秒内）
    开口的那句（`speechStarted` 时记下），听写出来和它最近念的 600 字对得上（`isEcho`：相邻两字一半以上出现过）就丢掉，字幕上也不显示。
    关掉时同以前的外放：念的时候不要麦克风的音频（`wantsMicrophone`），播完 450 毫秒再听。以前的「允许插话」（`bargeIn`，开口就打断）去掉了。
- `VoiceMode`（app，QML 单例）：QAudioSource / QAudioSink；设备支持就要 16 kHz / 24 kHz 单声道 16 位，不支持用设备的格式（采样率照实告诉
  Power，它重采样；这里只换声道和样本格式）。电平给界面的麦克风图标。macOS 先要麦克风权限。
- 界面：助手页工具栏的麦克风按钮，输入框上方的字幕条（在听 / 这句听到的 / 在想… / 在说…，「停」、结束）。
  v2.5 字幕条上方是 Friday 的光环（app 的 `HaloView`，`visual/HaloRenderer`；Swift 是 `Views/HaloView.swift`，同一套参数）：在听时跟着麦克风音量、
  在说时跟着念出来的声音（`VoiceMode.outputLevel`：交给扬声器时按 50 毫秒算好、跟着播放一格一格取）起伏，在想时一段亮弧绕环转；透明底、四周渐隐。

### 4.1 声纹：只听你的声音（2026-10-08）

用户：「我跟它说话时正好别人跟我说话，我第一句一直被打断」（一句说完就交给助手，助手在想时再交一句会停下重来，旁边的人一直在说，
你的那句就一直等不到回答）→「要用声纹」。

- Power（voice 0.2）：CAM++（CosyVoice 带的 `campplus.onnx`）在 ggml 上，实时听写 `start.speaker: true` 时每句 final（和说到 1 秒以上的
  partial）带 192 维声纹；`RealtimeTranscriber::voiceprint` 信号在同一句的 partial / final 之前发。
- `core/assistant/Voiceprint`：登记的声纹存在数据目录的 `voiceprint.json`（只在这台电脑上，不上传、不同步）；相似度（余弦）、几句取平均、
  阈值（宽松 0.35 / 标准 0.45 / 严格 0.55，不到 1.5 秒的短句放宽 0.15：实测同一个人 3 秒一句 0.77–0.98、1 秒 0.4–0.7，不同人 < 0.3）。
- 登记（`VoiceprintEnrollment`，界面 `VoiceprintEnrollSheet`）：只开麦克风，实时听写听你念三句（每句至少 1.5 秒），每句定稿自带声纹，
  取平均存起来；三句之间不像同一个人（最低相似度 < 0.5，念的时候旁边有人在说）就不存、提示重来。
- 语音模式（`AssistantVoice`）：登记了、开着「只听你的声音」、Power 能算声纹时（`filtering()`）每句都比：
  - 不是你：不交给助手、不打断，字幕上说「不是你的声音，没交给助手：…」；
  - 太短算不出声纹（不到 0.5 秒）：助手在想 / 在说时不算（多半是旁边的人随口一句），空着时照常交；
  - 念的时候说的也要认出是你才交（才打断）；太短认不出的不算。

## 5. 助手通话

- 听写：通话里每句话走「听录音」能力，装了本地语音时自动选本机（一句约 40 毫秒，百炼要一秒左右）。
- 合成：装了本地语音时接通就开一条 `RealtimeSpeaker`（48 kHz，就是通话的采样率），句子交给它，音频边合成边进播放队列，句间留 120 毫秒；
  打断时 `cancel`，`cancelled` 之前到的音频丢掉；连接出错就退回逐句走「说话」能力。
- Mac 上 v2.8 起助手在 Friday.app 里（ASSISTANT.md §2.2），通话还没搬过来；2.3–2.7 通话在 App 自带的助手进程里，用 Swift App 装的本地语音。

## 6. 测

`TestPowers`：清单解析（拼进路径的名字、跳出 models 目录的模型不收）、能力层条目、用假包从本地索引装一遍（状态、文件、条目、卸载）、
校验不对时报错。设了 `FRIDAY_TEST_VOICE_ZIP`（CI 编出的 friday-voice 包）和 `FRIDAY_TEST_VOICE_MODELS`（模型目录）时再跑真的：
经能力层听写、合成、停了再自动拉起；语音模式整条链路（一段录音当麦克风 → 听写 → 假大脑按块回 → 边写边读 → 播完接着听）；
声纹（`voiceprint`：用中文例句念三遍登记，再喂中英文混在一起的录音——中文交给助手、助手在答时说完也交，英文那句是另一个人，不交、不打断）。
`voiceEcho`：外放回声的判断（不用真包）。
`TestAssistant::voiceprintMath`：相似度、取平均、阈值、存取。

## 8. 本地大模型与生图 / 生视频（v2.7）

用户要求（2026-10-11）：「powers 新建使用本地模型能力和使用本地图片模型和视频模型的能力，本地大语言模型要支持多并发」「每个功能可以单独下载使用」
「大语言模型和图片 / 视频模型要可以在点击选模型弹出的模型库中选择全部模型，跟 v1.0 时的模型库一样」「模型库使用 v1.x 最后的模型库 UI」。
两个新 Power（Friday.Powers 的 `llm`、`diffusion`，schema 2 清单，协议见那边的 `docs/llm-protocol.md`、`docs/diffusion-protocol.md`）：

- **friday-llm**（llama.cpp）：OpenAI 兼容的 `/v1/chat/completions`；同时算几个请求，主智能体和同时派出的子智能体共用系统提示，
  前缀在基数树里共享、不重复预填（设计见那边的 `docs/llm-design.md`）。模型不跟包走：请求里 `model` 是模型 id，`x_friday` 带模型文件、会话、优先级。
- **friday-diffusion**（stable-diffusion.cpp）：异步任务 `/v1/jobs`，生图出 PNG、生视频出 WebM。清单的 `families` 列出认得的模型家族
  （Z-Image-Turbo、FLUX.2 klein、Qwen-Image、Wan2.2 TI2V 5B）：怎么认仓库、主模型之外要哪些配套文件（VAE、文本编码器）、缺省参数。

**模型库**（v1.x 最后的那版，68d840e，`qml/models/`、`ModelBrowserModel`、`core/llm/ModelCatalog` / `ModelLibrary` / `Downloader` /
`ModelScopeDownloader` / `HubAdapters` / `GGUF` / `ModelPreset` 从那里恢复）：模型菜单底部「显示更多模型…」打开；魔搭 / Hugging Face 上的 GGUF，
类型多了「生图」「生视频」（仓库名能被 diffusion 清单的家族认出来的）；认不出家族的生图 / 生视频仓库、配套的文本编码器 / VAE、向量模型算「其他」，
不能拿来对话（`LocalModels::looksNonChat`，目录和模型菜单共用）。下载到魔搭 / HF 自己的缓存布局（同 v1，`MODELSCOPE_CACHE`、`HF_HUB_CACHE` 可覆盖）。

**`LocalModels`**（`core/llm/`）：模型库「使用」、模型菜单「本地 ▸」都走它：
- 要用的 Power 没装就先装（`PowerManager::install`），模型没下完就先下（大模型：按量化档挑 GGUF、分片要齐、带 mmproj；生图 / 生视频：
  主模型 + 家族的配套文件，配套文件这个源没有就换另一个源）。
- 大模型：在 EngineStore 里建一个「本地」引擎（`local-<哈希>`，标题「本地 · 名字 量化档」，`baseURL power://llm`，model = 完整的模型 id
  「组织/仓库:量化档」——模型库给的是仓库 id，下完换成本机那份的完整 id，菜单才认得是同一个），给这段对话换上（会话隔离；
  从设置打开模型库时设成新对话默认）。胶囊上显示 CPU 图标和短名字。
- 给某段对话选的大模型（`use` 带上对话）在下的时候，那段对话的胶囊是「进度环 + 要换上的名字 + 百分比」（`pendingFor`），
  菜单最上面是「取消下载 …」；这期间这段对话又改选了别的（再选一个本地模型、选在线模型时 `releaseConversation`），
  之前那个下好了只建引擎、不把对话换回来，失败了也不提示。
- 生图 / 生视频：设成本地生图 / 生视频用的模型（`AppSettings::localImageModel` / `localVideoModel`）；设置 › 能力 里看得到、可以「不用了」。
- 下载在后台（「使用」后模型库窗口就关了）：`downloadIDs` / `downloads` 列出正在下、下载失败的（进度、已下 / 共要下；下好了、取消了就不列），
  设置 › 能力在对应的那一块下面一行一个（取消；失败的原因、「重试」「移除」），模型库的卡片上是进度条 / 「下载失败」。
  取消不算失败（回调的 error 为空，不弹提示）；重试接着原来的「使用」（给对话换上 / 设成默认 / 设成生图用的），又失败了只更新那一行、不再弹提示。
- 删除（模型菜单「本地 ▸ 删除模型文件」，确认后 `LocalModels::remove`）是强制删（v2.8，用户要求）：friday-llm 的进程还在就整个停掉
  （正在用本地模型回答的会被打断，下个请求自动重新拉起），等它退出、文件放开了再删（Windows 上加载着的 GGUF 删不掉）；
  删不掉（进程退出后句柄晚一点才放开、刚停掉又被正在跑的对话拉起来）就再停、隔半秒再试，最多 15 秒；还删不掉（别的程序占着）弹「删不掉模型文件」说原因。
  v2.8 之前确认框监听的是 `confirmed()`，而 `Alert` 默认点确认发的是 `accepted()`，点了「删除」什么都不发生。

**请求**：`OpenAIChatEngine` 遇到 `power://<id>` 时问 `powerResolver`（FridayCore 接到 `PowerManager::ensureRunning`）要地址和令牌，
请求体带 `x_friday {session, priority, main, model_path, mmproj_path}`（会话 = 对话 / 子智能体，主智能体优先、KV 池满时最后腾）。
生图 / 生视频是能力层的 `PowerBackend`：生图提交任务后等它做完（可取消），生视频是异步任务（`diffusion:<任务 id>`，轮询进度、做完取 WebM）。

**并发与前缀共享**：主智能体用本地大模型时子智能体跟它用同一个（`SubAgentRunner`：不按活换档；本地模型也不会被自动挑去当子智能体
或起标题的小活模型——friday-llm 一次只装一个模型，换模型要整个重新加载）。子智能体的名字不放系统提示里，放在任务消息的第一行
（`AgentTeamTools::subAgentNameLine`）：同一类子智能体的系统提示连同工具定义一字不差，friday-llm 能从先到的那个分叉（Qwen3.5 这类混合模型
只能在消息边界的检查点分叉，名字在系统提示开头时一个 token 也共用不上），在线服务的提示缓存也一样受益。

**显存**：本地大模型和生图 / 生视频共用一张显卡。friday-llm 按 llama.cpp 的 fit 加载，上下文开满、几乎占满显卡；只让它卸载不够
（智能体的下一个请求马上又按满显存加载回来，实测 Wan2.2 因此连计算空间都拿不到、直接失败）。所以生图 / 生视频前 `PowerBackend` 调
friday-llm 的 `/v1/memory/reserve`：预留 = 模型文件大小 + 计算空间（生图 1.5 GB、生视频 5 GB），15 分钟（生视频轮询时续；做完放掉 friday-diffusion 的模型、取消预留）；
friday-llm 留得不够就卸掉、下个请求按预留加载（上下文最低压到 32K，再不够少放几层到显卡），到期后再要回来。

**设置 › 能力**：两个 Power 和本地语音一样列出（状态、版本、平台、启用、卸载）；能力一栏写「对话（本地大模型）」「生图」「生视频」；
大模型那块有「模型库…」，新对话默认用本地大模型时写出是哪个；生图 / 生视频那块列出现在用的模型、「模型库…」（打开时类型筛好）和「不用了」；
两块下面都列出这一类正在下 / 下载失败的模型（`LocalModelDownloadRow`）。

**本地大模型关掉、没装时**（`LocalModels::chatAvailable` = friday-llm 装了且开着）：本地引擎（power://llm）都算关掉（同「服务与密钥」里关掉
一家在线服务，`refreshAvailability` 跟着 Power 的状态改 disabled），新对话默认、小活、助手这些选择器不列，用着它的对话回落到开着的模型；
对话的模型菜单不列「本地」「显示更多模型…」（v2.8 起开着时也不再列「最新的本地模型」）；本地引擎也不算在线服务（「服务与密钥」不列，不说成「在线服务都关掉了」）。

**测**（本机，2026-10-11）：隔离环境 `FRIDAY_DATA_DIR` / `FRIDAY_CACHE_DIR` / `FRIDAY_POWERS_DIR` 指到一个临时目录，
`FRIDAY_POWERS_INDEX=file:///…/index-v2.json`（Friday.Powers 的 `ci/make_index.py` 用本机打的包生成），`MODELSCOPE_CACHE` 指到放好模型
（或目录联接）的目录；DebugHarness 的 follow 模式：`evalon:type=ModelStatusView::App.openModelBrowser(conversationID)`、
`evalon:type=ModelBrowserWindow::browser.use("<仓库>")`、`ModelMenu.trigger({kind: "setLocalModel", value: …}, conversationID)`、
`vm.submit(文字, [图片路径])`。实测：0.8B 多模态读出图里的字；9B 当主智能体派子智能体生图（Z-Image-Turbo 768²）、生视频（Wan2.2 832×480）。

## 7. Swift 版（2026-10-08）

行为、文案和 Qt 一样（设置 › 能力、能力层的「本机」提供方、语音模式、声纹），文件对应：

- `Powers/Power.swift`（清单、平台：Apple 芯片 → `macos-arm64-metal`，Intel 的 Mac 不显示本地能力）、`Powers/PowerManager.swift`
  （目录 `~/Library/Application Support/Friday/Powers`，`FRIDAY_POWERS_DIR` / `FRIDAY_POWERS_INDEX` 同 Qt；下载断点续传、校验、
  `/usr/bin/tar` 解开，程序 `chmod 755`、去掉 quarantine；包里多包一层同名目录（`friday-voice/friday-voice`）时按「是文件」找程序）。
- `Powers/RealtimeVoice.swift`（`URLSessionWebSocketTask`）、`Capabilities/Providers/PowerBackend.swift`
  （`CapabilityRegistry.syncPowerProviders`，同样单独存在 `powerEntries`）。
- `Assistant/AudioIO.swift`：`AVAudioEngine` 的输入用 `AVAudioConverter` 转成 16 kHz 单声道 16 位（多声道混成一路）、算电平；
  `AVAudioPlayerNode` 边收边播 24 kHz，播完留 450 毫秒尾巴。音频线程上的回调都在 nonisolated 的工厂函数里生成（Swift 6 的隔离检查）。
- `Assistant/VoiceMode.swift` = Qt 的 `AssistantVoice` + `VoiceMode`；助手的内核在 helper（或远程主机）里，所以经 `AssistantModel` 的协议：
  `assistant.say`（`source: voice`）、`assistant.entry`（条目的 `data.reacting` 表示还在写）、`assistant.state`、`assistant.stop`。
  选项存在 UserDefaults（`voiceMode.listenWhileSpeaking`（v2.5，默认开）/ `voiceMode.onlyEnrolled` / `voiceMode.voiceprintStrictness`）。
- `Assistant/Voiceprint.swift`：声纹存 `~/Library/Application Support/Friday/voiceprint.json`，阈值、登记规则同 §4.1。
- 界面：`Views/PowersSettingsView.swift`（本地能力页 + 登记声纹的弹窗）、助手页工具栏的麦克风、输入框上方的字幕条。
- 麦克风：Info.plist 的 `NSMicrophoneUsageDescription`；正式签名开 hardened runtime，签名里要有 `com.apple.security.device.audio-input`，
  不然系统直接拒绝、不弹询问，系统设置的麦克风列表里也没有 Friday。它写在 `Friday/Friday.entitlements`（`CODE_SIGN_ENTITLEMENTS`）：
  只靠工程的 `ENABLE_RESOURCE_ACCESS_AUDIO_INPUT` 不够，Friday.build 为了公证设 `CODE_SIGN_INJECT_BASE_ENTITLEMENTS = NO` 时这一项也被去掉
  （v2.5.2 及以前的正式包都缺，系统日志里 tccd 报「requires entitlement com.apple.security.device.audio-input but it is missing」）。
  `Tools/CI/build.sh` 签完检查它还在，缺了就构建失败。
- 2.3–2.7 Mac 上助手的内核、通话在 Friday.app 自带的助手进程里，它用只读的 PowerManager（`FRIDAY_POWERS_READONLY=1`）跟着 App 装的这一份；
  v2.8 起助手在 App 里（ASSISTANT.md §2.2），只读模式删掉了。
- **Mac 上的合成**（2026-10-08 在 M1 上实测）：voice 0.2.0 及以前的 Metal 包，CosyVoice3 里 LLM 的 flash attention 在 Metal 上算错，
  念出来是乱语、一直念到长度上限（CPU 对；KV 缓存换 F16 也一样）；voice 0.2.1 起 Metal 上 LLM 不用 flash attention（flow 的照用）。
  M1 上合成约 3.5 倍实时（一句 10 秒的话要合成 35 秒左右），边写边读跟不上，念起来断断续续；听写和声纹在 M1 上都快。
- 测：Debug 的 `FRIDAY_UI_SCRIPT` 步骤 `powers[:refresh]`、`powersinstall:<id>`、`powersstart:<id>`、`voice[:off]`、`voicewait:<状态>`、
  `voicestate`、`enroll`；`FRIDAY_VOICE_INPUT=<录音>` 时用录音代替麦克风（按实时的节奏喂，登记时念三遍），`FRIDAY_VOICE_OUTPUT=<文件.wav>`
  时不出声、念的写进这个文件（按实时的节奏算放完，自动测试时 Mac 不外放）。
