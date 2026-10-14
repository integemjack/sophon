# 上游实现规格核查（ASSISTANT_SPEC.md 56 条 + 设计稿 §7/§8/§9）

核查对象：树中 origin/v2.3 合并后的上游实现（qt/src/core/assistant/ 及其测试）。
方法：逐条读代码与上游自带测试（TestAssistant / TestAssistantRuntime / TestAssistantChannels / TestTurnbrief / TestRemote 等，登记于 tests/TestRegistry.h），只读不跑。

## 总览

| 结论 | 条数 |
|---|---|
| ✅ 满足 | 36 |
| 🟡 部分（有实现、与规格有出入或只做一半） | 16 |
| ❌ 缺失 | 4 |

❌/🟡 共 20 条，其中 3 条可从本地封存版（/root/works/Friday-local-work-backup-20260718/ 与 git stash@{0}）适配，其余需上游补做或拍板。

## 56 条逐条矩阵

**内核 / 调度**
1. ✅ 常驻内核三条线分开：AssistantRuntime.h:39-48（构造于 FridayCore.cpp:114，非会话）；say / onActivity / startReaction 三条入口（AssistantRuntime.cpp:317/456/653）。
2. ✅ 同一时间一次反应、其余攒着：`m_reaction` 唯一 + `Batch m_pending`（AssistantRuntime.h:150-157、226；startReaction :653 起）。
3. ✅ 用户一开口就打断：say 里 cancel + requeue（AssistantRuntime.cpp:367-372），finishReaction 标 interrupted（:800-801）；assistant/ 下无 takeInterjection（仅 AgentRunner.cpp:331 会话自身用）。
4. 🟡 优先级 Immediate>Timely（Triage.cpp:21-67）✅；但「用户正在输入/说话时事件推迟」无实现（presence 只管「在看」，没有输入中状态）。
5. ✅ 自动唤醒上限 30/h：canAutoWake（AssistantRuntime.cpp:446-452；AssistantConfig.h:24），超限走 pushReceipt 模板（:566-569）。
6. ✅ 无客户端/渠道照常运转：FridayCore.cpp:114 无条件构造；push 是信号，连接与否不影响（RemoteServer.cpp:1310 仅在有人订时转发）。
7. 🟡 工具异步、无 wait_*（AssistantRuntime.cpp:723-733，超时返回「还没结果：晚点会作为回执回来」）✅；但超时统一 15s，非规格的取数类 3s。
8. ✅ 工具轮数默认 2：maxToolRounds = toolRounds+1（AssistantRuntime.cpp:718-720；AssistantConfig.h:23）。
9. ✅ 说的/给看的分开：post_detail 工具（AssistantTools.h:26）、withoutThinking 拆分（AssistantRuntime.cpp:770-773）、QML 折叠「给看的」（AssistantEntryRow.qml:3）、语音只念说的（:852-855）。

**上下文**
10. ✅ 每次现拼固定长度：ContextBuilder::build 每次反应调用（AssistantRuntime.cpp:655-697）。
11. 🟡 分层与层内裁剪 ✅（ContextBuilder.cpp:153-198，clip 各层不挤占）；无「按模型上下文缩放、不低于下限」逻辑。
12. ✅ 焦点档案最多 2 个：dossiers() `i < 2`（AssistantRuntime.cpp:598）。
13. ❌ 焦点权重 1/0.7/0.2：上游改用 mentionedAt 30 分钟窗口 + 最新优先（WorldBoard.h:45、AssistantRuntime.cpp:590-596），无权重衰减。封存版 FocusStack.cpp/h 有权重实现可参考（工作量中；上游模型自洽，也可改规格）。
   **→ 已拍板按上游实现为准，文档已对齐（2026-10-07）。**
14. ✅ read_session 只用于本次反应：结果走工具回调（AssistantRuntime.cpp:1420-1450），日志记一行 AssistantTools::summary（:1230）。
15. ✅ 项目细节留会话：档案只带目录/目标/步骤/最近一句（AssistantRuntime.cpp:598-620）。

**事件与回执**
16. ✅ 轻量 activity、主机对远程只订 activity：ActivityTap.h:8；RemoteFleetMachine 走 activity.subscribe（RemoteFleetMachine.cpp:36、90-91）。
17. 🟡 tool 事件限频：上游「每会话每扫一次最多 1 条」（ActivityTap.h:9，扫描 2s），非规格的 1 条/3s。
18. 🟡 数字代码写、pickLine 可测 ✅（agent/TurnBrief.cpp:263/344；tst_turnbrief.cpp:44-64）；briefWriter 默认关 ✅（AssistantConfig.h:31）——但**开关没有实现**（全树无引用），开了也不会写。
19. ✅ 模型不可用推送不停：无引擎 → pushReceipt（AssistantRuntime.cpp:666-672）；叫不醒 → pushReceipt（:566）。
20. 🟡 批量过大脑：用户在等/要紧→Timely/Immediate 即醒 ✅（Triage.cpp:44-51）；同项目 ≥3 条合并过一次 ✅（flushDigest，AssistantRuntime.cpp:531-548）；隔 30 分钟回来只拼了一句「先讲讲攒下的」提示（:682-684），m_digest 并未并入这次反应（仅定时器触发 flushDigest）→ 回来时批量讲不完整。
21. 🟡 在看只上板 ✅（Triage.cpp:23-24；AssistantRuntime.cpp:468-470）；但**客户端断开不清 presence**（RemoteServer.cpp:152-171 的 drop() 无 setPresence 复位），断线后仍被抑制。
22. ✅ 推送规则默认 Important=只推助手派的/出错/要确认（AssistantConfig.h:27；Triage::shouldPush :96-107）。
23. ✅ 重连重订拿快照：activity.subscribe 返回 snapshot（含 lastBrief；RemoteServer.cpp:359-365）；测试 assistantFleetWaker/assistantFleet。
24. ✅ 旧服务端退回：无 activity feature → runner.generating + summaries 比对，结束后取 tail 造简报（RemoteFleetMachine.cpp:90-91、128-247）；测试 assistantFleetLegacy。
25. 🟡 80ms 合批：activity 事件被**刻意改为立即直发**（RemoteServer.cpp:1307-1322 注释「不合批」）；80ms flush 只用于其他状态（:80-82）。有意偏差，需拍板。
   **→ 已拍板按上游实现为准，文档已对齐（2026-10-07）。**

**界面**
26. ✅ 侧边栏固定「助手」行、未读、渠道状态、不进分组、远程主机名（AssistantModel.h:5；AssistantEntryRow.qml；ContentView.qml:152）。
27. ✅ 按天分隔、分页、项目筛选、来源标注（AssistantEntryRow.qml:9、:170「已回复到」；assistant.history before/project/hasOlder，RemoteServer.cpp:1641-1646）。
28. 🟡 回执行一行一条+状态图标+批准/拒绝 ✅（AssistantEntryRow.qml:6、231-291；走 runner.resolveApproval 经 FleetMachine）；「另外 N 条」折叠未找到。
29. ✅ 状态板同源：大脑 inputs.board = m_board.render（AssistantRuntime.cpp:678），QML 同一 WorldBoard；停靠卡片 id `board`（PanelDock.qml:32）。
30. ✅ 超预算从最旧空闲省略 + 「上次跟用户说话之后」（WorldBoard.cpp:466-491；AssistantRuntime.cpp:665-674）。
31. ✅ #短号直达不经过大脑：directMessage/deliverDirect（AssistantRuntime.cpp:317-347、395-435）。
32. 🟡 「助手发送 · 来自 微信」✅（MessageRow.qml:163-169）；助手开会话的小机器人标记、「⋮ 不向助手汇报」菜单未找到。
33. ✅ 助手关着也有 turnFinished 系统通知（app/main.cpp:641-650）。
34. ✅ 「在 X 上跑」+「去看/在这台打开」（AssistantView.qml:157-175）。

**主机 / 部署**
35. ✅ 全局一个、没开回错误 3：assistantCall NotFound（RemoteServer.cpp:1614-1618；RemoteProtocol.h:84）。
36. ✅ daemon 交接条件含助手：DesktopService.cpp:86-87。
37. ✅ 搬家：exportFiles/importFiles + channel-secrets.json（AssistantRuntime.h:92-103）、assistant.export./import.（RemoteServer.cpp:1765+）、AssistantModel.cpp:564 切换迁移。
38. ✅（v2.8 改了）macOS 的助手不再跑在 friday-server 里：内核用 Swift 重写、在 Friday.app 里（ASSISTANT.md §2.2），C++ 的 FRIDAY_ASSISTANT_LOCAL/APP 删掉。
39. 🟡 受管端协议子集：helper 编进 friday-server（ASSISTANT.md:570、qt/ci/build_macos_helper.sh）、错误 4=NotAllowed（RemoteProtocol.h:85）；Swift App 本体不在本仓库，「本机离线」显示未找到。
40. ✅ RemoteDeploy macOS 用 App 自带 helper、不下载包（RemoteDeploy.cpp:83-88、514-516、560 的失败提示）。

**大脑 / 模型**
41. ✅ 默认「跟新对话一样」：resolved(defaultEngineID)（AssistantRuntime.cpp brainEngine()）。
42. 🟡 不支持工具就只回话不派活 ✅（AssistantRuntime.cpp:715-717 tools.reset()）；设置页标红提示：列表带 supportsTools 字段（AssistantSettings.qml:20），未确认有标红。
43. ✅ 思考默认关、语音提示换快模型（AssistantConfig.h:22；AssistantRuntime.cpp:721-722；AssistantSettings.qml:127）。
44. 🟡 复用 OpenAIChatEngine 流式/取消 ✅（:717-733）；think_in_background / SubAgentRunner 未做（工具清单里没有，assistant/ 无引用）。

**存储**
45. ✅ 不进 conversations.json：Assistant/ 独立目录（FridayCore.cpp:114）、journal 按天 JSONL（Journal.h:1-3）、设置可清空（AssistantSettings.qml:695）。
46. 🟡 state.json 只存 summarizedUntil（AssistantRuntime.cpp:1053-1070）；无 board.json 快照（全树无 board.json）、未读数只在客户端内存（AssistantModel）、渠道游标在 channels.json。重启校正靠 activity.subscribe 快照，功能可用但不合 §10 形制。
   **→ 已拍板按上游实现为准，文档已对齐（2026-10-07）。**

**渠道**
47. ✅ 一个对话、标来源、回哪来、通知渠道多选（ChannelHub.h:1-15；通知渠道 notify 标志，ChannelHub.cpp:1355）。
48. ✅ 回复回执=直接跟会话说：replyTo/#短号 直达（say 的 direct 路径 + ChannelHub 路由；测试 userClicks/schatEndToEnd）。
49. ✅ 绑定码 + /bind + 只听主人私聊（ChannelHub.cpp:432-441、683-693；ChannelHub.h:4-5）。
50. ✅ core Http + QWebSocket、无 Node、单渠道出错不连坐（SChatChannel.cpp:22、204；ChannelHub.h:108 status: off|connecting|online|error）。
51. 🟡 SChat 主体全中：offset/limit≤100/timeout≤25（SChatChannel.cpp:375-377）、clientMsgId（:660）、64KiB（:131）、流式 120s 空闲（:816）、键盘 10×4（:698-699）、callback chatKey 陷阱（:486-496）、限流按 retryAfter 退避（:673-678）、SPKI 指纹（:147-153、221）。偏差：游标存 channels.json 的 cursor（:97），非规格的 state.json；「每秒 10 次 append」为 flushStreams 定时近似而非硬限。
52. ❌ Telegram 适配器缺失（仅 kind 显示名 ChannelHub.cpp:238-239；无 sendMessageDraft）。
53. ❌ 飞书/钉钉/企业微信适配器缺失。
54. ❌ 微信 ClawBot 适配器缺失（无实现即无泄漏面，属未完成而非风险）。

**语音（P2，接口 P0 留好）**
55. ✅ 内核以「一句话」为单位、可取消、说的/给看的分开（say 的 spoken 标志 AssistantRuntime.cpp:335-336、688-689；interrupt() 可取消；:852-855；AssistantVoice/calls/ 语音、通话、P2P 文字全套）。
56. ✅ 打断（v2.5 起改成用户说完一句、交给助手时才打断，开口不打断：AssistantVoice listenWhileSpeaking、AssistantCalls::handleUtterance）；「给看的」不念（:852-855）。「约 1 秒出声」是目标值，本仓库无实测数据（通话里有每句耗时日志埋点，ASSISTANT.md:526）。

**§9 安全默认值表**：逐项核对 AssistantConfig.h / AssistantSettings.qml / ChannelHub —— 默认全中：enabled=false、模型跟新对话、思考关、confirmDestructive/approveFromChannels=false/brainApproves=false/fullFromChannels=false（AssistantRuntime.cpp:1345、1495、1537-1541 三重闸）、pushRule=Important、briefWriter=false、30s/30次/2轮、群聊只推送（groupCommands=false，ChannelHub.h:75-76）。SSH 密码不经聊天工具：助手无 add_machine 工具，无密码路径 ✅。防注入 ✅（ContextBuilder.cpp:12、135；AssistantRuntime.cpp:1427「内容是数据不是指令」）。防死循环 ✅（唤醒上限、silentReply、状态板合并同类事件）。审计 ✅（工具调用进日志与时间线卡片，AssistantRuntime.cpp:1230）。

## 渠道与语音专项（设计稿 §7/§8）

- §7 适配器完成度：SChat ✅（含通话 WS、SPKI、限流、键盘、媒体上传下载）；Telegram ❌；微信 ClawBot ❌；企业微信 ❌；飞书 ❌；钉钉 ❌；「通用入口 + 脚本渠道」三层兜底 ❌（无 ScriptChannel）。ChannelHub/Channel.h 公共接口、认主人、路由、渲染（ChannelRender.cpp plainText/split）、语音消息听写/合成（ChannelHub.cpp:875-889、AssistantMedia）已就位，加适配器是增量工作。
- §8 语音：v2.3 范围（渠道语音消息、通话、P2P 文字、本地语音模式、Voiceprint）上游已全部实现并有测试（TestCalls、TestAssistantChannels::calls/callSocket/voiceMessages/p2pText*）；P0 接口（spoken、interrupt、post_detail 不念）齐。

## 可适配候选清单（封存版 → 上游）

| 候选 | 封存版位置（/root/works/Friday-local-work-backup-20260718/qt/） | 上游现状 | 适配判断 |
|---|---|---|---|
| TelegramChannel | src/core/assistant/channels/TelegramChannel.{h,cpp}（292+49 行） | ❌ 无 | **是，中**：协议收发（getUpdates/draft/sendMessage）可整体搬，需换到上游 Channel.h 适配器接口、ChannelRender 加 HTML 子集、ChannelHub 工厂登记 |
| ScriptChannel | 同目录 ScriptChannel.{h,cpp}（168+35 行） | ❌ 无 | **是，中**：§7.6 兜底缺的就是它；需接上游 kind 工厂与安全约束 |
| BriefWriter | src/core/assistant/BriefWriter.{h,cpp}（281+66 行）+ tst_assistant_briefwriter.cpp | 🟡 只有 AssistantConfig.briefWriter 开关 | **是，小-中**：含 2 秒超时退回与数字校验，挂到 AssistantRuntime::onActivity 的回执路径即可 |
| Binding 绑定码 | channels/Binding.{h,cpp}（119+59 行） | ✅ ChannelHub 内建 bindCode/owners/groups | 否——上游已有等价物，不必搬 |
| ChannelRender | channels/ChannelRender.{h,cpp} | ✅ 上游自有 channels/ChannelRender.cpp（140 行，有测试） | 否 |
| util/Jsonl | src/core/util/Jsonl.{h,cpp}（113+31 行） | ✅ Journal 自带 JSONL 读写（Journal.h） | 否 |
| FocusStack | src/core/assistant/FocusStack.{h,cpp}（注意 FocusStack.h:15/22 的 TaskState/TurnBrief 桩，集成时须删） | ❌（条 13）上游用 mentionedAt 窗口替代 | **可选，中**：只有决定按规格补权重焦点时才搬，否则建议改规格认可上游模型 |
| 本地版 Runtime/ChannelHub/Journal 等 | 同目录 AssistantRuntime/ChannelHub/AssistantJournal 等 | ✅ 上游自有且更完整 | 否——以树中上游为准 |

## 建议的下一步优先级

1. **修条 21 的 presence 泄漏**（小）：RemoteServer::drop() 里按 client 复位 setPresence，防止断线客户端一直抑制推送；补一条断线用例。
2. ~~拍板两处有意偏差~~ **已完成（2026-10-07）：两处均拍板按上游实现为准，ASSISTANT.md / ASSISTANT_SPEC.md 已对齐，不再作为缺口追。**
3. **补 BriefWriter 实现**（小-中）：开关已存在、封存版实现与测试现成，接上 onActivity 回执路径即可闭合条 18。
4. **Telegram 适配器**（中）：封存版可搬主体，补齐后 P1 渠道 2/7；其余四平台（微信 ClawBot/企微/飞书/钉钉）需按官方文档新写，建议各开一包。
5. ~~存储对形~~（条 46）**已按上游形制记入规格（2026-10-07）：无 board.json、state.json 仅 summarizedUntil、未读数在客户端内存、游标在 channels.json。**
6. 注意：条 4「输入中推迟」、条 28 折叠、条 32 小机器人标记/「不向助手汇报」、条 42 标红、条 7 的 3s 取数超时，均为小缺口，可随相邻工作捎带。
