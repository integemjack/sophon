# Friday.Style —— 原生样式设计系统（Qt Quick）

Friday 的 Qt 版（Windows / Linux，外加 macOS 开发机）用**各平台的原生观感**：系统标题栏与窗口边框（Windows 11 上工具栏行并进标题栏那一行，按钮仍由系统画）、原生 Qt Quick Controls
样式、系统调色板（浅色 / 深色、强调色）与系统字体。**没有任何玻璃效果**（模糊、折射、发光、高光描边、投影、半透明材质都没有；例外，都是用户要求的：自绘菜单的阴影；对话页 / 助手页输入框后面的模糊底板——有内容滚到输入栏那段时把它模糊了垫在下面，输入框底色半透明，见「滚动边缘渐隐」）。
**布局是定死的**：界面结构、元素顺序与位置、文案、交互、objectName、视图用到的 QML 类型名 / 文件名、提示文字一律不变，
只换视觉层。除观感以外（功能、结构、文案、交互、默认值）仍以 Swift 源码为准（`Friday/Friday/Views/*.swift`）。

本文是给改组件 / 改视图的人的**契约**：照着写，不要猜。

---

## 1. 目录与接入

```
qt/src/app/style/            QML 静态模块 Friday.Style（目标 friday_style，插件 friday_styleplugin / Friday_StylePlugin）
  CMakeLists.txt             GLOB 收集 *.qml / *.js / icons/**，新增文件无需改 CMake；不含 C++、字体、着色器
  Theme.qml                  单例：原生调色板派生的语义色、字体与字号阶梯、圆角、间距、动画、图标地址
  *.qml                      组件（见第 4–7 节）
  IconNames.js               生成物：已知图标名表（Icon.qml 与 Theme.iconSource 共用）
  icons/{regular,semibold,bold}/<SF 名>.svg   生成物：按 SF Symbol 名命名的 Lucide 图标（stroke="currentColor"）
  icons/mapping.json         SF Symbol → Lucide 映射（手写，改完运行 qt/tools/gen_icons.py）
  icons/LICENSE-Lucide.txt   ISC
qt/tools/gen_icons.py        生成图标与 IconNames.js
qt/ci/qml-imports/           只给 windeployqt 扫描的 QML（运行时才选的控件样式），不编进程序
qt/src/app/platform/         只在 Windows 上编译的原生代码（属于 app 的 Friday 模块，不在 Friday.Style 里）：
  WindowsTitleBar.{h,cpp}    Windows 11 合并标题栏 + 系统标题栏时的开窗防白闪（见第 4 节「标题栏」）
```

```cmake
add_subdirectory(src/app/style)
target_link_libraries(friday PRIVATE friday_style friday_styleplugin)
```

### main.cpp 做的事（qt/src/app/main.cpp）

1. **选控件样式**（建好 QML 引擎、加载 Main.qml 之前，`QQuickStyle::setStyle`）：

   | 平台 | 样式 | 说明 |
   |---|---|---|
   | Windows 11（`QOperatingSystemVersion >= Windows11`） | `FluentWinUI3` | QML 样式，缺的控件回退到 Fusion |
   | Windows 10，启动时系统是浅色 | `Windows` | NativeStyle（QStyle 绘制）；它没有的控件（ToolBar、ToolButton、ToolTip…）回退到 Fusion / Basic。**只有浅色**：选了它就用 `QStyleHints::setColorScheme(Light)` 把应用配色固定为浅色，运行中系统切到深色时标题栏、调色板都保持浅色（不会出现深色标题栏配浅色控件） |
   | Windows 10，启动时系统是深色 | `Fusion` | Windows 样式没有深色，改用跟随系统深色调色板的 Fusion |
   | Linux | `Fusion` | 调色板、字体来自桌面平台主题 |
   | macOS（开发机） | `macOS` | NativeStyle |

   样式只在启动时选一次，运行中切换深浅色不会换样式（Win10 要重启 Friday 才在 Windows / Fusion 之间切换）。
   环境变量 `QT_QUICK_CONTROLS_STYLE` 优先（开发时切样式对比用），但**只在 QML 导入路径里找得到这个样式模块时**才用：
   Qt 自带的样式名先规范成标准大小写（`fusion` → `Fusion`；Qt 的模块名区分大小写，`Theme.controlsStyle` 也按标准名比较），
   找不到（KDE 常设的 `org.kde.desktop`、Breeze / Kvantum 等包里没带的样式）时打一条警告、照常用上表的原生样式——
   否则界面加载失败、程序起不来。用 `Windows` 样式时（包括环境变量指定的）同样固定为浅色。

2. **中文回退字体**：原生样式的界面字体（Segoe UI Variable、Linux 桌面字体）没有中文，Qt 会挑到宋体一类的衬线字体。
   `installCjkFallbackFont()` 用 `QFontDatabase::addApplicationFallbackFontFamily`（Qt 6.8+）把第一个可用的
   Microsoft YaHei UI / Noto Sans CJK SC / … / PingFang SC 设为汉字与全角标点的回退。**不要在 QML 里写死字体名**，
   一律用 `Theme.fontFamily` / `Theme.monoFamily`（或不设，让原生控件用样式字体）。
3. **`Theme.controlsStyle`**：加载 Main.qml 之后写入 `QQuickStyle::name()`（`"FluentWinUI3"` / `"Windows"` / `"Fusion"` / `"macOS"`）。
4. **标题栏模式**（选好样式之后、加载 Main.qml 之前）：`useMergedTitleBar()` 为真（Windows 11 且 `FRIDAY_TITLEBAR` 不是 `system`）
   时把 `Theme.titleBarMerged` 设为 true（Theme 单例在这里第一次创建）；Windows 上总是 `WindowsTitleBar::install()`
   （合并的窗口挂上标题栏处理，其余窗口只在第一帧之前防白闪）。见第 4 节「标题栏」。
5. `Q_IMPORT_QML_PLUGIN(Friday_StylePlugin)`（静态 QML 模块）。不加载 Inter 字体；玻璃时代的 `WindowFrameHelper` 已删，原生代码只有
   Windows 上的 `platform/WindowsTitleBar`。只有 Windows 11 合并标题栏的窗口带 alpha 通道（FridayWindow 的 `color` 透明）。

### 打包

运行时选的样式不在界面源码的 import 里，打包脚本单独保证它们在包里：
`ci/package_windows.ps1`（windeployqt 多扫 `ci/qml-imports/`，再逐个核对 FluentWinUI3 / Windows / NativeStyle / Fusion / Basic
模块及其 DLL，并用 dumpbin 核对它们的依赖）、`ci/package_linux.sh`（核对 / 补齐 Fusion 与 Basic 模块和库，ldd 核对），
`ci/verify_linux_package.sh` 检查 `QtQuick/Controls/Fusion`、`Fusion/impl`、`Basic`。
合并标题栏不用多打包任何东西：`platform/WindowsTitleBar` 只链接 Windows 自带的 `dwmapi`、`uxtheme`（`docs/PACKAGING.md`「原生控件样式」）。
注意：`verify_linux_package.sh` 还检查 `QtQuick/Effects`、`QtQuick/Shapes` 等界面 import 的模块——**如果你删掉了全工程最后一处
`import QtQuick.Effects`（或 Shapes），同时把它从那个列表里删掉**，否则 Linux 打包校验会失败。

---

## 2. 原生样式的规则（必须遵守）

1. **组件包装原生控件**：Friday.Style 里有原生对应物的组件写成 `QtQuick.Controls` 控件的薄包装，在 style 模块内部
   `import QtQuick.Controls as QQC`（不要不加限定地 `import QtQuick.Controls`：`Button / Menu / MenuItem / TextField / Label /
   ScrollView / Slider / ProgressBar …` 会和本库重名）。视图里同理，要用 Qt 自带控件时写 `QQC.Xxx`。
2. **绝不覆盖 QQC 控件的 `background` / `contentItem` / `indicator` / `handle` / `label`**（也不覆盖 ComboBox 的 `popup`、
   Slider 的 `handle`、SpinBox 的 `up.indicator` 等委托）。Windows / macOS 原生样式会打印
   `The current style does not support customization of this control` 并画坏；FluentWinUI3 虽然不报警告，也会丢掉原生外观。
   只用控件自己的 API：`text`、`icon.source / icon.width / icon.height`、`display`、`flat`、`highlighted`、`checkable / checked`、
   `enabled`、`font`、`padding` 类、`implicitWidth / implicitHeight`、`ToolTip.*`；`palette` 只在确有必要时改单个颜色。
   需要在控件上叠东西（例如下拉小箭头）时，作为**额外子项**放进控件并用 padding 让位（见 ToolbarButton），不要替换委托。
3. **没有原生对应物的纯自绘组件**（Tag、Badge、Card、ListRow、SidebarRow、Panel、Capsule…）用**平面 Rectangle**
   + Theme 颜色画：不要渐变、阴影、模糊、ShaderEffect、MultiEffect、整块半透明叠层（Theme 里带 alpha 的颜色可以用；唯一的例外是输入栏后面的 `BackdropBlur`，见 §「滚动边缘渐隐」）。
   悬停叠 `Theme.hoverFill`，按下叠 `Theme.pressedFill`；聚焦的选中用 `Theme.selection` + `Theme.selectionText`，
   不聚焦的选中用 `Theme.unemphasizedSelection`；描边 1px `Theme.separator`；圆角取 `Theme.radius.*`（都很小）。
   这类组件基于 `QtQuick.Templates`（`T.AbstractButton` 等）或普通 Item 时可以自定 background / contentItem——规则 2 只针对 QQC 控件。
4. **图标**：沿用 Lucide SVG 与 SF Symbol 名。自绘处用 `Icon { name; color; size }`；原生控件用
   `icon.source: Theme.iconSource(name, weight)`，并在 `Component.onCompleted` 里调
   `Theme.adoptIconColor(control, () => <颜色>)`：FluentWinUI3 / Windows 的按钮会自己给 `icon.color` 赋状态色（**Fluent 的按钮文字
   颜色就取 `icon.color`，手动赋值会把文字颜色一起改掉**），Fusion 与 Windows 的菜单项不会——不着色时 SVG 是黑的，深色下看不见。
   颜色按控件取：按钮 `control.palette.buttonText`；菜单项 / 列表项 `control.highlighted ? control.palette.highlightedText : control.palette.text`。
   要强制某个颜色（例如红色的「删除」）时才直接赋 `icon.color`。
5. **提示（.help）**：原生 `QQC.ToolTip`。组件内用附加属性
   `QQC.ToolTip.text / .visible / .delay: 800`，或 `Tooltip { }` / `HelpArea { }`（两者都是原生 ToolTip 的包装）。
6. **菜单、弹窗、提示、对话框**：`QQC.Popup` / `QQC.Dialog`、`QQC.ToolTip`；**菜单和下拉列表例外：v2.0 起自绘**（用户要求，2026-10-05：
   原生菜单的系统级观感不好）——`Menu` / `MenuItem` / `MenuSeparator` 基于 `QtQuick.Templates`（T.Menu、T.MenuItem、T.MenuSeparator），
   `PopUpButton` 的按钮仍是原生组合框、下拉列表（`popup`、`delegate`）自绘，外观见第 7 节的表与 `Theme.menu*`；
   弹出层一律 `popupType: Popup.Item`（窗口内弹出层：DebugHarness 的 `closepopups` / 截图依赖它）。模态遮罩：`Sheet` / `Alert` 设了
   `QQC.Overlay.modal: ModalDimmer { color: … }`（Popup 的附加遮罩组件，不是控件委托，各样式都允许改）——FluentWinUI3 照它自带的
   `palette.shadow` 30% 画，其余样式用 `Theme.dimmer`（Fusion 自带的遮罩只有约 7% 黑，深色下看不出是模态）；`ModalDimmer` 在标题栏合并时
   空出右上角的系统标题栏按钮（不压暗它们），弹窗的 `topMargin` 设成按钮下沿（窗口很矮时也不盖到按钮）。自己写的模态弹窗照这样做。
   模态弹窗开着时窗口内容禁用（2026-10-08，用户「设置打开的时候能点到后面的内容」）：Qt 6.11 里弹窗里的控件接受了按下以后，
   按下还会接着发给后面窗口内容里的 pointer handler（`QQuickDeliveryAgent` 的 handlersOnly，遮罩的拦截那时已经跳过）——点设置里的按钮、
   左栏，后面同一位置侧边栏的会话被选中，右键弹出它的菜单。Sheet（居中弹出时）/ Alert 打开时登记到 `SheetHost.modalWindows`，
   `Main.qml` 的 `ContentView` 绑 `enabled: SheetHost.modalWindows.indexOf(mainWin) < 0`（禁用的子树什么事件都不收，handler 也不收；
   遮罩下面的工具栏图标会跟着变成禁用色）。在弹窗里垫一层接住点按的 MouseArea 没用，反而让后面的 handler 一定收到。自己写的模态弹窗也要登记。
   原生样式自己画的菜单 / 弹窗阴影属于样式，不算我们加的阴影。
7. **字号**：一律 `pixelSize`，取 `Theme.size.*` / `Theme.pixelSize(style)` / `Theme.font(...)`；原生控件不设 `font` 时用样式字体
   （Fluent：正文 14px，ToolButton 12px）。
8. **颜色**：一律取 Theme（第 3 节），不要写死十六进制颜色（`Theme.hex()` 给在线服务品牌色这类数据色用）；
   不要用 `Theme.dark ? A : B` 自己调色，除非是 Theme 没有的数据色。
9. **窗口**：标题栏按钮、拉伸边框、圆角、阴影都是系统的；不要画红绿灯、拖动区、自绘标题栏按钮、窗口圆角或描边。
   Windows 11 上窗口顶部的工具栏行就是标题栏（第 4 节「标题栏」）：**标题行里不要画不透明的底**（会盖住系统按钮），内容不要
   滚进工具栏行（在工具栏下沿剪掉），要能点的东西用控件（或接收鼠标的项），空白和文字自动是标题栏（拖动、双击最大化、右键窗口菜单）。
   窗口标题（`title`）照常设：系统标题栏（不合并时）、任务栏、Alt+Tab 显示它。
10. Qt Layout 的坑：`ColumnLayout / RowLayout` 的最大尺寸由子项决定——**子项都不 `Layout.fillWidth` 时，外层即使 `fillWidth`
    也撑不开**，文字行要给 `Layout.fillWidth: true`（Swift 的 `.frame(maxWidth: .infinity)`）。

### 各样式的特点（写组件时要知道）

| | FluentWinUI3（Win11） | Windows（Win10） | Fusion（Linux） |
|---|---|---|---|
| 窗口底色 `palette.window` | 浅 #F3F3F3 / 深 #202020 | 只有浅色 #F0F0F0（深色启动时改用 Fusion：深 #1E1E1E） | 桌面主题 |
| 强调色 `palette.accent` | 系统强调色（深色时是浅蓝，上面用黑字） | 系统强调色 | 桌面主题 |
| 按钮会给 `icon.color` 着色 | 是（文字色 = icon.color） | Button 是，MenuItem 否 | 否 |
| ToolBar 背景 | 透明 | 回退 Fusion：浅渐变 + 上下线（底边线不显示，见 ToolbarBackground） | 浅渐变 + 上下线（同左） |
| 选中的 ToolButton | 强调色底、白 / 黑字 | 回退 Fusion：凹下的按钮面 | 凹下的按钮面 |
| 控件字体 | Segoe UI Variable 14px（ToolButton 12px） | 系统界面字体（中文系统：微软雅黑 UI 9pt = 12px） | 桌面字体 |
| 自定义委托 | 不报警告，但会丢原生外观 | **报警告并画坏** | 允许，但不要做 |

---

## 3. Theme（单例）

颜色来自一个隐藏窗口里的原生控件探针的 `palette.active`（窗口外的孤立控件收不到深浅色切换），随系统浅色 / 深色、强调色实时变化；
`FRIDAY_COLOR_SCHEME=light|dark`（DebugHarness → `QStyleHints::setColorScheme`）强制配色时调色板一起切换。

### 保留的名字（取值换成原生的）

| 分类 | API |
|---|---|
| 外观 | `Theme.dark`（bool：调色板窗口色偏暗）；`Theme.pick(light, dark)` |
| 颜色工具 | `Theme.opacity(c, a)`（在原 alpha 上再乘）、`Theme.alpha(c, a)`、`Theme.mix(a, b, t)`、`Theme.hex(0xRRGGBB)`、`Theme.luminance(c)` |
| 文字 | `label`（= palette.windowText）`secondaryLabel`（墨色 62% / 深色 77%）`tertiaryLabel`（45% / 55%）`quaternaryLabel`（10% / 12%，细描边、浅底）`quinaryLabel`（5%）`placeholder`（= palette.placeholderText）`disabledLabel`（36%）`text`（= palette.text）`primary`（不透明墨色，`Theme.opacity(Theme.primary, 0.08)` 这类着色用） |
| 底色 | `windowBackground`（浅色 = palette.window，不透明；深色纯黑，用户要求 2026-10-06）`underPageBackground` `controlBackground` / `textBackground`（= palette.base）`sidebarBackground`（= 窗口色）`alternatingRow` |
| 分隔 / 选中 | `separator`（墨色 12% / 14%）`opaqueSeparator` `unemphasizedSelection`（墨色 8% / 10%）`selection`（= palette.highlight）`selectedTextBackground`（强调色 30% / 45%，配 `selectedTextColor: Theme.label`）`focusRing`（= accent）`link`（= palette.link）`dimmer` |
| 填充 | `fill` `secondaryFill` `tertiaryFill` `quaternaryFill` `quinaryFill`（淡墨色）`controlFill`（= palette.button）`controlFillPressed` `trackFill` `codeBackground` |
| 强调色 | `accent`（= palette.accent，可设 `accentOverride`）`accentForeground`（按强调色亮度取黑 / 白） |
| 系统色 | `red orange yellow green mint teal cyan blue indigo purple pink brown gray`（状态图标、着色卡片的底；浅色下比 macOS 的深一些）。**当文字用时改用下面「新增」里的 `orangeText / greenText / redText` 或 `readableOnSurfaces()`**：浅色白底上原色不够清楚（橙约 2.7:1、绿约 3.5:1、红约 4.3:1） |
| 字体 | `fontFamily`（= 当前样式 Label 的字体）`monoFamily`（Cascadia Mono → Consolas → DejaVu Sans Mono → Noto Sans Mono → … → SF Mono / Menlo → monospace）`monoCandidates`；`Theme.font(style, weight, monospaced)` 返回 `Qt.font`；`Theme.mono(style)`；`Theme.pixelSize(style)`（数字原样返回）；`Theme.defaultWeight(style)`；`Theme.iconSize(style)`（字号 × 1.25）；`Theme.tracking(px)`（恒为 0） |
| 字号 | `Theme.size.largeTitle / title / title2 / title3 / headline（semibold）/ body / callout / subheadline / footnote / caption / caption2（medium）` = macOS 字号（26/22/17/15/13/13/12/11/10/10/10）× 原生正文字号 / 13，取整，最小 10。Fluent：body 14、callout 13、subheadline 12、caption 11、title3 16；Linux 10pt：与 macOS 相同。`Theme.lineHeight.*` 同比缩放 |
| 圆角 | `Theme.radius.small 2 / control 4 / medium 4 / code 4 / group 6 / card 6 / panel 6 / composer 6 / bubble 10 / sheet 8` |
| 间距 | `Theme.spacing.xxs 2 / xs 4 / s 6 / m 8 / l 10 / xl 12 / xxl 16 / section 20`（不变） |
| 自绘控件高度 | `Theme.controlHeight.large / regular / small / mini`（30 / 24 / 20 / 16 × 字号比例）；`controlHeightFor(size)`；`controlFontSize(size)`（regular = body） |
| 工具栏 | `Theme.window.toolbarHeight` 48（工具栏行高；Windows 11 合并进标题栏时 36，也是标题行高）/ `toolbarItemHeight 32`（工具栏按钮高，只有图标的按钮是 32 见方）/ `toolbarIconSize 16` |
| 动画 | `Theme.duration.fast 120 / normal 220 / slow 350 / sidebar 250`；`Theme.snappyEasing` |
| 工具 | `Theme.hairline(dpr)`（1 物理像素的线宽） |

### 新增

| API | 说明 |
|---|---|
| `Theme.controlsStyle` | 当前控件样式名（main.cpp 写入；没写入时为空） |
| `Theme.titleBarMerged` | Windows 11 上工具栏行与系统标题栏合成一行（main.cpp 在加载界面前写入，之后不变；视图用所在窗口的 `FridayWindow.titleBarMerged`），见第 4 节「标题栏」 |
| `Theme.nativePalette` | 当前样式调色板的激活组（`window windowText base text button buttonText highlight highlightedText accent link placeholderText mid dark light …`），Theme 没给的颜色从这里取 |
| `Theme.selectionText` | = palette.highlightedText（`selection` 底上的文字） |
| `Theme.hoverFill` / `Theme.pressedFill` | 自绘可点区域的悬停 / 按下叠加色 |
| `Theme.baseFontSize` | 原生控件正文字号（px） |
| `Theme.scaledFontSize(macPoints)` | 把 macOS 字号换算成原生字号（最小 10） |
| `Theme.iconSource(name, weight)` | SF Symbol 名 → 图标 SVG 的完整 `qrc:` 地址（给 `icon.source`）；未知名字回退通用图标并警告一次 |
| `Theme.adoptIconColor(control, fn)` | 样式没给 `icon.color` 赋值时，把它绑定到 `fn()`（见第 2 节规则 4） |
| `Theme.relativeLuminance(c)` / `Theme.contrastRatio(a, b)` | WCAG 相对亮度 / 对比度（1–21，按不透明算） |
| `Theme.readableColor(c, background, minRatio = 4.5)` | 数据色（品牌色等）当文字用：在不透明背景上对比度不够时逐步往墨色混，直到达到 minRatio（服务卡片的能力标签） |
| `Theme.orangeText` / `greenText` / `redText` | 状态色当文字用（说明文字、警告 / 错误提示、「已接入」「缺密钥」这类标签的字、破坏性按钮 / 菜单项的字）：在窗口底色和 `controlBackground` 上都至少 4.5:1；浅色下比原色暗，深色下基本就是原色。图标（警告三角、勾）仍用 `orange / green / red` 原色——`Label` 写 `color: Theme.orangeText; iconColor: Theme.orange` |
| `Theme.destructiveFill` | 破坏性强调按钮（Alert 的「删除」「移除」）的底色：FluentWinUI3 的强调按钮浅色是白字、深色是黑字，`red` 往墨色混到与这个字色至少 4.5:1（浅色约 #CE2E2A，深色就是 `red`） |
| `Theme.readableOnSurfaces(c, tintOpacity = 0)` | 同上，任意颜色；`tintOpacity` 是文字所在的底叠了 c 的比例（着色卡片 `Card { tint: Theme.red; tintOpacity: 0.14 }` 里的红字写 `readableOnSurfaces(Theme.red, 0.14)`）。`Badge` 的 `foreground` 默认就是 `readableOnSurfaces(tint, tint.a × backgroundOpacity)` |

### 删除

`Theme.appearance` 与枚举 `Theme.System / Light / Dark`（强制配色用 `FRIDAY_COLOR_SCHEME`）、`Theme.glass.*`、`Theme.glassSamplingEnabled`、
`Theme.trafficLightsReservedWidth`、`Theme.window.trafficLightDiameter / trafficLightSpacing / trafficLightCenterX / trafficLightCenterY /
sidebarInset / resizeBorder`、`Theme.radius.window`、`Theme.radius.sidebar`、`Theme.windowBorder`、Inter 的 FontLoader。

### SwiftUI → Theme 速查

| SwiftUI | Friday.Style |
|---|---|
| `.foregroundStyle(.secondary / .tertiary)` | `color: Theme.secondaryLabel / Theme.tertiaryLabel` |
| `.foregroundStyle(.tint)`、`Color.accentColor` | `Theme.accent` |
| `.orange / .green / .red / .blue` | 图标、着色底：`Theme.orange / green / red / blue`；文字：`Theme.orangeText / greenText / redText`（其他颜色 `Theme.readableOnSurfaces(c)`） |
| `.background(.fill.tertiary, in: .capsule)` | `Rectangle { radius: height / 2; color: Theme.tertiaryFill }`（或 `Tag`） |
| `.background(.quaternary.opacity(0.5), in: .rect(cornerRadius: 8))` | `color: Theme.opacity(Theme.quaternaryLabel, 0.5)` |
| `Color.primary.opacity(0.08)` | `Theme.opacity(Theme.primary, 0.08)` |
| `.font(.callout.weight(.medium))` | `StyledText { textStyle: "callout"; weight: Font.Medium }` / `Theme.font("callout", Font.Medium)` |
| `.font(.system(.caption, design: .monospaced))` | `StyledText { textStyle: "caption"; monospaced: true }` |
| `.glassEffect(...)`（任何形式） | 没有玻璃：可点的块用 `Panel` / `Capsule` / 原生按钮，纯装饰去掉（见第 6 节） |
| `Color(nsColor: .windowBackgroundColor)`（GlassBackdrop） | `Theme.windowBackground` |

---

## 4. 窗口外壳

### FridayWindow（Window）

主窗口 / 设置 / 关于面板的外壳：**系统原生窗口**——最小化 / 最大化 / 关闭按钮、系统拉伸边框、Windows Snap、
深色标题栏（Qt 随配色处理）都由系统给。Windows 11 上顶部的工具栏行与系统标题栏合成一行，其余平台是系统标题栏 + 下面的工具栏行
（下面「标题栏」一节）。窗口底色 `backgroundColor`（= `Theme.windowBackground`）：不合并时就是 `color`；合并时 `color` 是透明的
（Qt 因此给窗口 alpha 通道），标题行以下由 FridayWindow 自己铺 `backgroundColor`。

| 属性 / 函数 | 说明 |
|---|---|
| Window 自带的 `title` `width` `height` `minimumWidth/Height` `maximumWidth/Height` `visible` `visibility` `x` `y`、`show()` 等 | 照旧 |
| 只读 `titleBarMerged` | 工具栏行与系统标题栏合成了一行（= `Theme.titleBarMerged`，启动后不变） |
| `titleBarHeight`（`Theme.window.toolbarHeight`：合并时 36，否则 48） | 合并时标题行的高度（从 `topInset` 往下算）：标题行透明、按这个高度延伸 DWM 标题栏、这一段里做标题栏命中测试。没有工具栏的窗口（关于面板）设成按钮高度 |
| `backgroundColor`（`Theme.windowBackground`） | 窗口底色，合并时也是 DWM 标题栏的底色（运行中改了标题栏跟着变） |
| `captionButtonsWidth` `captionButtonsHeight` | 原生代码写入（视图只读；不合并时 0）：右上角系统按钮从窗口右边 / 内容顶边算起占的宽 / 高（最大化时高度不含 `topInset`：按钮在窗口坐标里占 `topInset + captionButtonsHeight`）。顶部的 Toolbar 自动在右边让出宽度 |
| `topInset` | 原生代码写入（视图只读；不合并或没最大化时 0）：最大化时窗口上边溢出屏幕的边框厚度，子项整体往下让这么多 |
| `resizable`（默认 true） | true：flags 写明 `Qt.Window`、`WindowTitleHint`、`WindowSystemMenuHint`、`WindowMinMaxButtonsHint`、`WindowCloseButtonHint`（不加 `CustomizeWindowHint`，系统装饰与只写 `Qt.Window` 相同；Qt 在 Windows 上自己弹 Alt+Space 的窗口菜单时按 flags 启用「最小化」「最大化」，只写 `Qt.Window` 时两项是灰的）。false：没有最大化按钮（flags 是 `Qt.Window`、`CustomizeWindowHint`、`WindowTitleHint`、`WindowSystemMenuHint`、`WindowCloseButtonHint`，`minimizable` 时再加 `WindowMinimizeButtonHint`），尺寸用 minimum = maximum 固定（设置窗口、关于面板；Qt 让固定尺寸窗口的边框拖不动）。**不用** `Qt.MSWindowsFixedSizeDialogHint`：它让窗口在不同缩放的显示器之间移动时保持原来的物理像素尺寸 |
| `minimizable`（默认 true） | 只在 `resizable: false` 时起作用：false 时也没有最小化按钮，只剩关闭（关于面板，同系统的关于框） |
| 只读 `maximized` | 最大化或全屏 |
| `toggleMaximized()` | 最大化 ↔ 还原 |

子项直接放进窗口（`anchors.fill: parent`）：默认属性 `contentData` 把它们放进内部的内容区（铺满窗口，合并且最大化时从 `topInset`
开始），视图不用管 `topInset`。**删除**：`frameMode` `customFrame` `frameHelper` `roundedCorners` `showTrafficLights`
`glassBackdrop` `chrome` 与 `content` 别名。

### NavigationSplitView

侧边栏 + 详情（`NavigationSplitView(columnVisibility:)`）。侧边栏贴窗口左边、底色 `Theme.sidebarBackground`（只铺在工具栏行以下），
与详情之间一条 1px 竖分隔线，**从窗口顶边贯穿到底边（穿过工具栏行 / 合并后的标题栏行）**——用户要求（2026-10-01：「侧边栏的竖线贯穿显示」），
不合并（系统标题栏 / Linux）时同样从工具栏行顶边画到底；侧边栏顶部是一行与详情 Toolbar 同高同背景的工具栏（合并时就是标题栏的
左半段），右侧放 `sidebarToolbar` 的按钮和「隐藏侧边栏」。
顶部一行的最左边可以放 `leadingToolbar` 的按钮（主窗口的应用菜单 ☰）：位置同工具栏的第一个按钮（x = 8，工具栏行里竖直居中，
按钮间距 4），盖在侧边栏顶部一行 / 详情工具栏上面，**固定不动**——侧边栏显示时它是侧边栏顶部一行的左端（`sidebarToolbar` 和
「隐藏侧边栏」仍在侧边栏这一列的右端），收起时它是详情工具栏的左端；合并与不合并的标题栏一样。
收起 / 展开有动画（`Theme.duration.sidebar`）；`sidebarToolbar` 的按钮（「新对话」）和「显示 / 隐藏侧边栏」是一行，收起时随动画移到
详情左上角、**不隐藏**（== Swift 收起侧边栏后工具栏里仍有新对话；用户要求 2026-10-04）：x = 8，有 `leadingToolbar` 时在它们右边
再空 4（主窗口有 ☰ 时：☰ 8、新对话 44、显示侧边栏 80），工具栏行里竖直居中，详情的 Toolbar 自动让出 `detailLeadingInset`
（到最右边那个按钮的右边：有 ☰ 时 112，标题从 120 开始；macOS 用原生菜单栏、没有 ☰ 时 76，标题从 84 开始）；
分隔线处（工具栏行以下）可拖动改宽度：平时只有 1px 线，鼠标放上去（或正在拖）时中间亮出一根 5 × 50 的强调色把手（`SplitDivider`；
后台任务面板左边的拖动区同样，用户要求 2026-10-04）。后台任务面板（v2.0 前叫 Agent 树）是嵌在窗口里的一张卡片：工具栏行以下、上下左右各空 5，
1px 分隔线色描边、`Theme.radius.panel` 圆角，不顶到窗口顶边、没有贯穿工具栏行的竖线（用户要求 2026-10-05）；拖动区跨在卡片左边框上、自己不画线。
v2.2 起后台任务、终端两张卡片都能停在对话区左右上下，每边还能分几列 / 几行（`PanelDock`，TERMINAL.md §2）：边的内侧、相邻两组之间、
同一组两张卡片之间是拖动区（上下方向用 `SplitDivider { vertical: true }`），按住卡片标题栏的空白处（`CardDragArea`）拖到别处——
拖动中被拖的卡片保持原来的大小跟着鼠标（0.85 不透明、强调色 2px 描边）；排进某一组时那个位置腾出来（别的卡片让开，空位是强调色框），
新开一列 / 一行时布局不动、只亮一根 4 宽的强调色悬浮条；松手才挪。
后台任务、终端按钮留在工具栏最右、开关时不挪位置。侧边栏、对话区、面板卡片都是同一个窗口底色。

| 属性 / 信号 / 函数 | 说明 |
|---|---|
| `sidebar` / `detail`（Item） | 自动铺满各自区域（sidebar 在顶部工具栏行以下） |
| `sidebarToolbar`（list&lt;Item&gt;） | 侧边栏顶部右侧的按钮（ToolbarButton） |
| `leadingToolbar`（list&lt;Item&gt;，新增） | 顶部一行最左边、固定不动的按钮（ToolbarButton；主窗口放应用菜单 ☰）；不可见的按钮不占位置 |
| `sidebarMinimumWidth 200` `sidebarIdealWidth 240` `sidebarMaximumWidth 320` `sidebarWidth` | 列宽 |
| `sidebarVisible` | 显示 / 收起（按窗口宽度自动收起属于 ContentView 的逻辑） |
| `showSidebarToggle`（true）`animationsEnabled`（true） | |
| 只读 `detailLeadingInset` `sidebarColumnWidth` `detailArea` | `detailLeadingInset` = 收起程度 ×（有侧边栏的按钮或「显示侧边栏」时到最右边那个的右边，否则到 `leadingToolbar` 的右边，都没有时 0） |
| `toggleSidebar()`、信号 `sidebarToggled(bool visible)` | 内置按钮的提示文字「隐藏侧边栏」/「显示侧边栏」不变 |

**删除**：`sidebarStyle`（floating / flush）、`reserveTrafficLights`、`sidebarBackdrop`。

### Toolbar

窗口顶部的工具栏行：左边前置项 + 页面标题 / 副标题（普通原生文字：固定 14px 常规字重（2026-10-06 起不加粗）+ subheadline 二级色；v2.0 起标题不随原生正文字号缩放，没有副标题时按字形的实际上下边界竖直居中，用户要求 2026-10-05），右边是按钮。
Windows 11 上它就是标题栏（`inTitleBar`）：右边让出系统按钮（`resolvedTrailingInset`），窗口不活动时标题 / 副标题变淡
（`Theme.tertiaryLabel`，同系统标题栏的文字）；其余平台在系统标题栏下面。背景是 `ToolbarBackground`，**底边不画横分隔线**（用户要求，2026-10-01：「去掉标题栏底部的横线」；
所有窗口、合并与不合并都一样，标题栏行与下面的内容同色、直接相接，同 Windows 11 自带应用）：
不合并时**不透明**（窗口底色 + 当前样式的 ToolBar 外观；Fusion 的 ToolBar 自带的底边深色线放在剪裁区外面，不显示），合并时透明（露出系统标题栏）。内容不要滚到它下面：聊天页的内容层在它
下沿剪掉（不合并时看起来与被挡住一样），所以也不需要滚动边缘效果。放不下时标题先截断，再把最左边放不下的尾部项收进「»」原生溢出菜单
（一行按钮展开成里面的按钮；可用宽度扣掉了 `resolvedTrailingInset`）。菜单项文字取尾部项的 `overflowText`（有且非空时）> `text` > `help`；
不是按钮、但给了 `overflowText` 的项收起时在菜单里是一行禁用的文字（v2.0 之前聊天页的用量徽标就是这样）。

| 属性 | 说明 |
|---|---|
| `title` `subtitle` `titleVisible` | 页面标题（窗口标题另在系统标题栏 / 任务栏） |
| `leading`（list） | 标题左边的项 |
| 默认子项 = 尾部项 | ToolbarButton / SearchField … |
| `backgroundVisible`（**默认改为 true**） | false 时整行透明 |
| `leadingInset`（-1） | -1 = 自动：在 NavigationSplitView 里取它的 `detailLeadingInset`，否则 0 |
| `trailingInset`（-1，新增） | -1 = 自动：这一行在合并的标题栏里时取所在窗口的 `captionButtonsWidth`，否则 0 |
| `windowTitleBar`（true，新增） | 这一行在窗口顶部、贴着窗口右边；不在窗口顶部的工具栏写 false（不当标题栏、不让位） |
| 只读 `inTitleBar` `resolvedLeadingInset` `resolvedTrailingInset` | 这一行就是合并后的标题栏；实际生效的左 / 右留白（新增） |
| `horizontalPadding`（8）`itemSpacing`（4，新增：尾部项之间的间距） | |
| 只读 `overflowCount`；`openOverflowForTest()` | 收进「»」的按钮数 |

**删除**：`backdrop`、`glassBackdrop`；QML 里的拖动窗口 / 双击缩放（系统标题栏或合并标题栏的原生命中测试负责）。

### ToolbarButton（QQC.ToolButton）

原生 ToolButton（平的；悬停、按下、选中由样式画）。只有图标时是 `toolbarItemHeight` 见方；有文字时宽度随内容（左右内边距 10）。

| 属性 | 说明 |
|---|---|
| `iconName` `iconWeight` `iconSize`（16） | 图标（`Theme.iconSource`），颜色由样式或 `adoptIconColor` 决定 |
| `labelStyle`：`"titleAndIcon"`（有文字时默认）/ `"iconOnly"` / `"titleOnly"` | 映射到 `display` |
| `help` | 提示；没有时只有图标的按钮用 `text` 当提示 |
| `menu`（T.Menu）`menuIndicator` | 点击在按钮下方弹出（有 `popupBelow` 就用它）；`menuIndicator` 在右内边距里画向下箭头 |
| ToolButton 自带的 `text` `checkable` `checked` `enabled` `clicked()` `toggle()` … | 照旧；`checked` 不配 `checkable` 也能表示「当前页」（设置页的页签） |

**删除**：`glass`、`tint`、`backdrop`、`foreground`、`itemHeight`（视图里已经没有这些写法）。

### ToolbarBackground

- `ToolbarBackground`（新增，Rectangle）：工具栏行的背景（窗口底色 + `QQC.ToolBar` 外观），`separatorVisible`
  **默认 false**（用户要求去掉标题栏底部的横线；只有不在窗口顶部、确实需要分隔的自拼工具栏才显式打开）。
  `inTitleBar`（默认跟随所在窗口的 `titleBarMerged`）时透明、不画 ToolBar 外观（也不画底边分隔线），并带 `titleBarDragArea: true`
  （原生命中测试落在它上面、没落在上面的控件上 = 标题栏空白处）。自己拼工具栏样子的一行（例如弹窗顶部）可以直接用它，
  不在窗口顶部时写 `inTitleBar: false`。

### 视图里的工具栏

- 主窗口（ContentView）：顶部一行最左边是**应用菜单 ☰**（`qml/AppMenuButton.qml`，放在 NavigationSplitView 的 `leadingToolbar` 里）——
  Windows / Linux 没有菜单栏，应用命令放在这里（用户要求，2026-10-02：主窗口左上角要有三条横线的菜单）。原生 ToolButton（图标 `line.3.horizontal`，
  提示「菜单」，`Accessible.name`「应用菜单」，objectName `appMenuButton`），点了在按钮下方左对齐弹出设计系统的 `Menu`：
  新对话 Ctrl+N、停止生成 Ctrl+.（生成中才可用）、—、设置… Ctrl+,、—、检查更新…、关于 Friday、—、退出 Friday（v2.0 去掉了「模型库…」）。
  每项调用托盘菜单 / 快捷键调用的同一个方法（`ConversationStore.createConversation("")`、`App.stopGenerating()`、`App.openSettings("")`、
  `App.checkForUpdates()`、`App.showAbout()`、`App.quit()`）；快捷键只在菜单项上显示提示（`MenuItem.shortcut`），
  由 Main.qml 的 Shortcut 注册，不重复注册。菜单开着时再点按钮是收起（`closePolicy` 用 `CloseOnPressOutsideParent`：按在按钮上不算按在外面，
  点击时收起），按在别处、Esc 收起。键盘：**F10** 打开并高亮第一项（同没有菜单栏的 Windows 程序），方向键选、回车执行，再按 F10 或 Esc 收起；
  模态弹窗开着时 F10 不响应（被弹窗挡住，同按钮被遮罩盖住）。**单按 Alt 不打开**：Alt+Space（窗口菜单，合并标题栏时原生代码在 Qt 之前截下）、
  Alt+Tab、Alt+F4 都以 Alt 开头，Qt 收到的只是 Alt 按下 / 松开，分不清。macOS 用原生菜单栏（`App.useNativeMenuBar()`）时不显示；
  设置、关于面板没有 ☰。调试：预设 `menu:app`（打开后截图）。
- v2.0 起设置不是独立窗口，是主窗口里的 sheet（`SettingsSheet.qml`：宽 min(1040, 窗口宽 - 80)、高 min(740, 窗口高 - 80)；
  左栏 200 宽的页面列表，右侧页名标题行（右上角「×」关掉，`ToolbarButton`；2026-10-08 起，原来是左栏底部的「完成」）+ 当前页），没有自己的标题栏 / 工具栏。页面里打开的子流程（服务库、管理模型、
  新建脚本工具、MCP 的添加 / 粘贴 / 编辑 mcp.json、让 Agent 做…）是普通 `Sheet`：设置开着时 `SheetHost.embedHost` 登记了右侧内容区，
  它们铺满那块区域显示（没有外框、阴影、遮罩、进出动画，左栏照样能点；换页、关掉设置时一并关掉；Esc 先关最上面的一层）。
  （v2.0 之前还有设置窗口 `SettingsWindow.qml` 与模型库窗口 `ModelBrowserWindow.qml`。）
- 聊天页的内容层在 Toolbar 下沿剪掉（消息不滚进工具栏行；没有滚动边缘效果）。
- 拖动 / 双击最大化 / 右键窗口菜单由系统标题栏或合并标题栏的原生命中测试负责，视图里不放拖动区。
- 关于面板（没有工具栏）：合并时标题行是右上角关闭按钮那么高的一条空白（`titleBarHeight = max(30, captionButtonsHeight)`），
  内容从它下面开始，窗口高 = 240 + 标题行（与系统标题栏时外观一样大）。

### 标题栏

**各平台**

| 平台 | 标题栏 |
|---|---|
| Windows 11（build 22000+，Qt Quick 用 D3D11（默认）/ D3D12 渲染） | **合并**：窗口顶部的工具栏行就是标题栏，右上角是系统（DWM）画的最小化 / 最大化 / 关闭按钮 |
| Windows 10 | 系统标题栏 + 下面的工具栏行：没有 `DWMWA_CAPTION_COLOR`，延伸出来的标题栏没法上色。不自绘按钮，所以用不到 Segoe MDL2 Assets 字形，也没有 Windows 10 自定义外框要补画的上边框 |
| Linux（X11 / Wayland）、macOS 开发机 | 系统标题栏 + 下面的工具栏行（不能把内容放进系统标题栏，除非整个标题栏自绘）。这一节的原生代码不参与编译 |
| 环境变量 `FRIDAY_TITLEBAR=system`（任何平台） | 系统标题栏 + 下面的工具栏行：合并出问题时的退路，也用来在 Windows 上预览 Linux 的布局。`merged` / 不设 = 自动；其他值打一条警告、按自动处理；在不支持的平台上写 `merged` 打警告、用系统标题栏 |
| `QT_QUICK_BACKEND=software`、`QSG_RHI_BACKEND=vulkan` / `opengl` 等 | 系统标题栏（软件渲染、Vulkan 没有带 alpha 的窗口合成，标题行会是黑的；OpenGL 的内容画在 GDI 重定向表面上，与下面涂重定向表面的做法可能冲突，没在应用里验证过） |

**做法**（`src/app/platform/WindowsTitleBar.{h,cpp}`，微软的 Custom Window Frame Using DWM；按钮由系统画，**不自绘**）：

1. main.cpp 在加载 Main.qml 之前写 `Theme.titleBarMerged`，再 `WindowsTitleBar::install()`：装一个应用级事件过滤器和一个原生事件过滤器。
2. 合并时 FridayWindow 的 `color` 是透明的：`QQuickWindow::setColor` 据此给窗口要 alpha 缓冲，D3D11 / D3D12 走 DirectComposition
   （预乘 alpha）。标题行什么都不画，标题行以下铺 `backgroundColor`。
3. 窗口第一次显示时（`QEvent::Show`：原生窗口已建好、还没 ShowWindow）子类化它的窗口过程（`SetWindowLongPtr(GWLP_WNDPROC)`，
   其余消息照常交给 Qt），触发一次 `SWP_FRAMECHANGED` 并把客户区恢复成 QML 要的位置和大小；原生窗口销毁前（`SurfaceAboutToBeDestroyed`）摘下。
4. DWM：`DwmExtendFrameIntoClientArea(上边 = (topInset + titleBarHeight) × dpr)`，`DWMWA_CAPTION_COLOR = backgroundColor`；
   设了底色后按钮字形的深浅跟着这个底色（实测：运行中把应用配色切成浅色——Qt 这时会按新配色重设 `DWMWA_USE_IMMERSIVE_DARK_MODE`——
   #202020 底上的字形仍是白的；反过来浅色底上仍是黑的），字形与底色总有对比，不用另外设。不用 Mica / 标签背景（标题行会是两种底色、偏蓝）。

| 消息 | 处理 |
|---|---|
| 所有消息 | 先给 `DwmDefWindowProc`：按钮的悬停 / 按下 / 点击、按钮处的命中测试、悬停最大化弹出的 Snap Layouts、系统提示 |
| `WM_NCCALCSIZE`（在原生事件过滤器里） | `DefWindowProc` 算出左 / 右 / 下边框后把上边改回原值（去掉标题栏和上边框）；最大化时也不加上边框——客户区从溢出屏幕的窗口顶边开始，否则 `DwmDefWindowProc` 不认按钮。放在 `QAbstractNativeEventFilter` 里是因为 Qt 的窗口过程比较处理前后的矩形记下边框，`geometry` / `frameMargins` 才自洽 |
| `WM_NCHITTEST` | 左 / 右 / 下边框交给 `DefWindowProc`（固定尺寸的窗口返回 `HTBORDER`，不给拉伸光标）；最上面一个边框厚度（`SM_CXSIZEFRAME + SM_CXPADDEDBORDER`）是 `HTTOP` / `HTTOPLEFT` / `HTTOPRIGHT`（最大化、固定尺寸时没有）；标题行里按 QML 项树找这一点最上面的项（按绘制顺序，剪裁的项之外不算）：控件（`QQuickControl`）、`MouseArea`、接收鼠标的项、接按键的 Pointer Handler（TapHandler / DragHandler…）→ `HTCLIENT`（照常交给 QML）；只有悬停提示的展示项（只带 HoverHandler，如用量徽标上的 `HelpArea`）也是 `HTCLIENT`（悬停提示要收鼠标移动），拖动见下面的 `WM_LBUTTONDOWN`；`titleBarDragArea: true` 的项、纯文字（Text / Label）、空白、模态弹窗的遮罩层 → `HTCAPTION`（拖动带 Aero Snap、双击最大化 / 还原由系统处理） |
| `WM_LBUTTONDOWN` | 标题行里只有悬停提示、下面也没有接按键的项（MouseArea / 控件 / TapHandler…）的地方：自己 `QWindow::startSystemMove()`，拖动 / Aero Snap / 贴靠预览同标题栏空白处（双击最大化没有）；其余照常交给 Qt。验证可设 `FRIDAY_HITTEST_PROBE=1`，DebugHarness 里用 `fridayHitTest.itemHit / itemDrag` 断言 |
| `WM_NCRBUTTONUP`（`HTCAPTION`）、`WM_CONTEXTMENU` | 标题行在几何上属于客户区，`DefWindowProc` 不会弹窗口菜单、Qt 会把 `WM_CONTEXTMENU` 吃掉：自己 `GetSystemMenu` 按窗口状态启用各项（还原、移动、大小、最小化、最大化；固定尺寸的窗口「大小」「最大化」是灰的）、「关闭」设为默认项（粗体），`TrackPopupMenu` 在光标处弹出，选中的命令走 `WM_SYSCOMMAND` |
| `WM_SYSKEYDOWN`（Alt+Space，不带 Ctrl / Shift / Win） | Qt 会拦下 Alt+Space 自己弹窗口菜单（按 `QWindow::flags` 启用各项、弹在客户区左上角，盖住合并的标题行），所以在交给 Qt 之前拦下：弹同一个菜单（启用规则同右键），位置在标题行下沿的左端（同原生窗口弹在标题栏下面），照原生高亮第一项；拿掉队列里随后的 `WM_SYSCHAR ' '` |
| `WM_NCMOUSEMOVE` / `WM_NCMOUSELEAVE` / `WM_MOUSEMOVE` / `WM_MOUSELEAVE` / `WM_TIMER` | 悬停修正：光标从客户区直接跳进按钮时系统紧跟着补发一个 `WM_NCMOUSELEAVE`，DWM 收到就把刚亮起的悬停态清掉（照微软示例写的纯 Win32 窗口也一样）。只吞掉这一个（光标还在按钮上时），之后真正离开由下一条移动消息或 50ms 轮询补发 |
| `WM_ERASEBKGND` / `WM_PAINT`；显示时变成可见后的第一条消息 | 画 GDI 重定向表面（在 Qt 的 DirectComposition 内容下面，Qt 从不画它，默认是白的）：标题行 alpha 0（DWM 的标题栏从这里透出来），其余窗口底色 alpha 255（`BeginBufferedPaint` + `BufferedPaintSetAlpha`，画完 `GdiFlush`）；只涂需要的那一块：`WM_PAINT` 涂更新区（`GetUpdateRect`，不 Validate，Qt 照常拿到 expose；Qt 的 `BeginPaint` 随后发来的 `WM_ERASEBKGND` 不再涂），其余的 `WM_ERASEBKGND` 涂 DC 的剪裁区——拉伸时不整窗重涂。每次显示（`WM_SHOWWINDOW`）后，窗口变成可见收到的第一条消息里就同步涂一次：`ShowWindow` 先发激活消息、后发 `WM_ERASEBKGND`，进程里第一个窗口激活时输入法初始化（实测搜狗拼音约 100ms），等到 `WM_ERASEBKGND` 再涂，主窗口的开窗动画就是白的。开窗动画、拉伸时 Qt 还没画到的地方都是底色——**不闪白、不透出桌面** |
| `WM_ACTIVATE` / `WM_SIZE` / `WM_DPICHANGED` / `WM_WINDOWPOSCHANGED`（`SWP_FRAMECHANGED`、`SWP_SHOWWINDOW`） | 重新延伸（系统激活、Qt 改窗口 flags 时会把延伸清零），更新 `topInset` 和按钮尺寸（最小化时不更新：那时 `IsZoomed` 为假、客户区是 0 大小，还原时再读） |

**按钮的尺寸和颜色从哪来**：全部来自系统，Theme 里没有标题栏按钮的常量。尺寸 = `DWMWA_CAPTION_BUTTON_BOUNDS`（窗口显示时才读：
显示之前系统给的还是原来系统标题栏的矩形）→ `captionButtonsWidth` / `captionButtonsHeight`；150% 缩放实测三个按钮 146.7 × 29.3、
只有关闭 49.3 × 29.3（v2.0 之前的设置窗口不能最大化但能最小化，系统照样画三个按钮、最大化是灰的）。颜色由 DWM 按标题栏底色算，与系统标题栏的增量
相同：深色底 #202020 悬停 #2D2D2D、按下 #2A2A2A；浅色底 #F3F3F3 悬停 #E9E9E9、按下 #EDEDED；关闭悬停 #C42B1C 白字，
按下深色 #B22A1B、浅色 #C84031；失活时字形变淡；提示（「关闭」「最小化」…）是系统的：Windows 给所有 Win32 窗口的标题栏按钮都用同一种
经典样式的提示（窗口类 `MicrosoftWindowsTooltip`，米黄底方框；系统标题栏时、系统自带的字符映射表 charmap 也一样），与程序清单里有没有
公共控件 6.0 无关（试过加上，没有变化）。

**窗口菜单的深浅色**：系统画的弹出菜单默认总是浅色。`applyMenuTheme` 用 uxtheme 未公开的 `SetPreferredAppMode`（序号 135，Windows 10
1903 起）+ `FlushMenuThemes`（序号 136）把它设成深色 / 浅色：合并标题栏的窗口弹菜单前按窗口底色设；`install()` 时与配色变化
（`QStyleHints::colorSchemeChanged`）时按应用配色设（系统标题栏右键、Qt 弹的 Alt+Space）。对整个进程生效；版本太老或找不到函数时不设（浅色）。

**写视图时**：标题行（`titleBarHeight` 那一段）里不要画不透明的底（会盖住系统按钮）；内容不要滚进去；能点的东西用控件或接收鼠标的项；
想让某块（即使下面有可交互的项）当标题栏空白处，给它 `titleBarDragArea: true`。新窗口用 FridayWindow，顶部放 Toolbar 就自动是标题栏。

**系统标题栏时的开窗白闪**（Windows 10、`FRIDAY_TITLEBAR=system`，以及任何时候的图片预览这类普通 `Window`）：Qt 的第一帧出来之前窗口
显示的是没初始化的 GDI 重定向表面（深色时开窗动画里白一下）。`install()` 给这些窗口挂一个临时窗口过程，把它涂成底色（FridayWindow 用
`backgroundColor`，其余不透明的 `Qt.Window` / `Qt.Dialog` 类型窗口用 `color`；同上，变成可见后的第一条消息里就涂），第一帧（`frameSwapped`）
出来就摘掉（之后不能再用 GDI 往窗口上画：不透明窗口的 D3D 交换链是 flip 模式）。

**限制**

- 系统按钮固定约 46 × 30（逻辑像素）在右上角，不能拉到工具栏行的 48 高；工具栏的按钮在 48 的行里竖直居中，中心比系统按钮的字形低约 9。
  有意不往上挪：行高与行内布局是定死的（48 高、项目竖直居中），而且最上面约 7.3（150% 缩放时 11 物理像素）是上边拉伸区，
  按钮往上挪就会有一截落进拉伸区。
- 合并时整个窗口走 alpha 合成（DirectComposition），GPU 开销略多。
- 悬停修正依赖观察到的系统行为（没有文档）；所有测过的进出方式都对，Windows 以后改了行为可能要再调。
- 「在标题栏和窗口边框上显示强调色」这个系统设置不起作用（我们设了 `DWMWA_CAPTION_COLOR`）。
- 模态弹窗（Sheet / Alert）的遮罩层盖在标题行上（标题行空白处照样能拖），但空出右上角的系统按钮（`ModalDimmer`）：按钮那一块
  保持标题栏底色，和系统标题栏时一样不变暗。
- 窗口菜单的深色靠未公开的 uxtheme 接口（见上「窗口菜单的深浅色」）。
- 主窗口开窗时，开窗动画的前约 200ms 是一块只有底色和系统按钮的空窗，之后内容一下子出现（第一个窗口激活时输入法初始化、
  主界面第一帧都比较慢）；没有白闪。用 `DWMWA_CLOAK` 盖到第一帧可以避免，但会去掉系统的开窗动画（原型实测），所以不做。
- 没验证：Windows 10 真机、多显示器之间移动时 DPI 变化（`WM_DPICHANGED` 时重新延伸、重读按钮尺寸）、高对比度主题、触控 / 笔操作标题栏按钮、全屏。
- 运行中切换深浅色：原生代码跟随 `backgroundColor` 的变化（运行中改它，DWM 标题栏底色跟着变，已验证；按钮字形随底色由 DWM 挑，
  harness 的 `scheme:` 切过之后字形仍与底色有对比）；但 `scheme:`（`QStyleHints::setColorScheme`）在 FluentWinUI3 下不改 Theme 的调色板
  （系统标题栏时也只有标题栏变色），系统真正切换深浅色没有测。
- Qt：只用了 Qt 6.8 就有的公开 API（`QPlatformSurfaceEvent`、`QAbstractNativeEventFilter`、`QQuickWindow::frameSwapped`、
  `QStyleHints::colorSchemeChanged`；Qt 的窗口过程比较 `WM_NCCALCSIZE` 前后的矩形记边框、Alt+Space 由 Qt 自己弹也早于 6.8），
  最低版本仍是 6.8，但只在 6.11.3 上验证过。

验证（真实屏幕截图 + 模拟鼠标）：命中测试、按钮悬停 / 按下颜色（与系统标题栏对照）、Snap Layouts、拖动 / Aero Snap / 双击 / 右键 / Alt+Space
（菜单位置、启用项，按 N / X / R 最小化 / 最大化 / 还原；v2.0 之前的设置窗口「大小」「最大化」是灰的）、上边与四角拉伸、最大化不被裁、
最小化 / 还原 / 最大化动画与阴影（与系统标题栏逐帧对照一致）、开窗闪烁（黑 / 白背景板上逐帧统计，主窗口 / 设置 / 关于 / 图片预览，
浅色 / 深色，合并与系统标题栏）、拉伸时的边缘、模态弹窗开着时系统按钮不变暗。

---

## 4.5 主界面（v2.0 简化，== Swift；方案见 https://claude.ai/artifact/U4pa5XQJgkHvxbFurZJCUc）

- **列宽**：消息列、工具卡、输入框共用最大内容宽 720（`MessageList.columnWidth`、`ChatView` 里 ComposerView 的宽度）。消息间距 12。
- **默认收起来**：用户消息的复制 / 重新输入、助手消息的复制 / fork / 朗读 / ⓘ 只在鼠标停在这条消息上时显示（`MessageRow.hovered`，
  HoverHandler；不显示时 opacity 0 照样占位，不跳）。结束后的用量统计不再占一行，进 ⓘ 的悬停提示（`StatsLabel.finalString` +
  `helpString`）；生成中照旧一行状态。助手消息没有头像。用户气泡中性灰（`Theme.primary` 0.08 / 0.09，无描边，`radius.bubble` 12）。
  框架提示（message.notice）是一行灰字（`info.circle` + caption）。
- **分叉**：有兄弟分支的消息操作行最前面常显「‹ 2 / 3 ›」（`MessageRow.BranchSwitcher`，按 `ChatViewModel.forkPoints` 找这条消息所在的分叉点），
  没有浮在左上角的分叉指示器了。
- **工具卡**：收起时一行——图标（二级色）· 标题（子 Agent 派发卡用名字）· 参数摘要（`ToolEvent::argumentSummary`，toVariant 的 `summary`）·
  状态图标（待确认例外：橙色「待确认」）· 箭头。相邻的工具块连成一组（`BlockView.groupTop / groupBottom`，由 MessageRow / SubAgentTranscript
  按 `MessageListModel::isToolAt` 判断前后块）：组内上下紧贴、中间一条缩进的分隔线、只有两端是圆角（`Rectangle.topLeftRadius` 等）。
  底色是内容卡的 `Theme.tertiaryFill`；只有待确认（橙）、失败 / 已拒绝（红）那一行叠状态色 0.10。模型胶囊、遗留的「幻觉」挪到展开后。
- **标题栏**：没有用量徽标；后台任务按钮（AgentTreeButton，v2.0 前叫「Agent 树」）只在这段对话派过子 Agent（`agentTree.isEmpty` 为假）或面板开着时出现。
- **输入框**：工具行在输入框里（正文下面），`Capsule { bordered: false }`；深度思考在模型不支持时不显示；工作目录在对话有内容后不显示（v2.1，不能再改）；
  发送 / 停止在这一行最右边。空闲时占位「给 Friday 发消息」。
- **侧边栏**：会话单行（标题 …… 右边一格：等确认 > 转圈 > 悬停的「⋮」> 相对时间）；分组标题整行点击折叠 / 展开、箭头常显，「+」悬停才出现；
  没有工作目录的那组叫「其他」。
- **空状态**：48 的图标圆 + 标题（title2）+ 两列没有边框的建议，去掉了副标题。
- **代码块**：没写语言时没有标题行；「复制」悬停时出现在右上角。「回到底部」是输入框正上方居中的胶囊。
- **滚动边缘渐隐**（用户要求 2026-10-05，== Swift 的 `.scrollEdgeEffectStyle(.soft)`）：消息滚到工具栏下面时工具栏下沿往下、被输入栏那段底板
  （`bottomCover`）挡住时底板上沿往上，各盖一道 16 高、从窗口底色（贴边处和工具栏行 / 底板同色）到透明的渐变（`EdgeFade`），
  内容像是淡出到边后面；只盖内容那一列（不压滚动条），没有内容穿过时淡出（`MessageList.contentUnderTop / contentUnderBottom`）；
  「回到底部」压在上面。不是阴影：不画比底色深的颜色。内容在工具栏下沿剪掉（Windows 11 上工具栏行就是系统标题栏、不画底，内容不能滚到它下面）。
- **输入栏后面的模糊底板**（用户要求 2026-10-08「输入框背景没有模糊」，== Swift 输入框的 Liquid Glass）：底板是 `BackdropBlur`——有内容滚到
  输入栏那段时，把那一段内容截下来（`ShaderEffectSource`，只在有内容时截）、`MultiEffect` 模糊（32）、再盖一层 0.55 的窗口底色；没有内容时就是窗口底色。
  输入框（`ComposerView` 的 Panel）底色是 0.6 的 `controlBackground`，模糊的内容从输入框里透出来；底板上沿的渐隐接的是底板盖的那层颜色。
  对话页（MessageList）和助手页（AssistantTimeline）一样。
- **后台任务面板的标题栏**（用户要求 2026-10-05）：面板顶部一条固定的标题栏（40 高，不随内容滚动）：左边列表页「后台任务」+ ⓘ（用量）+ 个数，
  详情页是返回路径「‹ 后台任务 › 上级 …」（原来在详情内容的最上面）；右边最大化 / 还原（`arrow.up.left.and.arrow.down.right` /
  `arrow.down.right.and.arrow.up.left`，Lucide maximize-2 / minimize-2）和关闭（`xmark`）。最大化时面板撑满整个内容区（工具栏下面、侧边栏右边），
  对话区宽度不变（不重排）、完全盖住后不画；关上面板（或换会话）时还原。待确认 / 失败的小标签在列表最上面一行。
- **侧边栏自动收起**只看窗口宽度（< 840 收起、> 900 展开回来），打开后台任务面板不再把侧边栏收起（原来阈值要加上面板宽）。
- **消息列表按锚点排版**（v2.6，用户 2026-10-10「会话在处理的时候往上翻，内容会抖」「在下面添加内容，上面在变是什么意思」「要完美的方案，
  不是靠限制或者补偿」）：`AnchoredList`（Flickable + C++ 的 `AnchoredColumn`），对话页 MessageList 和助手页 AssistantTimeline 共用。
  原来（v2.0 起）是 `ListView` 从下往上排：以最下沿为基准往上堆，下面任何一行变高（出字、子智能体卡片、工具卡片），上面整片都被推走，
  只能事后改 contentY 拉回来——这一步排在下一帧（先画出跳过去的那一帧，闪一下），用户正滚着时还不能改（会打断滚动），就一直跳。
  现在排版这一步就按锚点排（== 浏览器的 scroll anchoring、Android RecyclerView 的 anchor layout）：跟随底部时锚点是最下沿（内容往上长，
  视口贴着底部看最新的）；往上翻时锚点是顶边落在视口里的第一行（一行盖满视口时就是这一行），它的顶边不动，下面的往下长、上面的往上长。
  排版在 polish 里做，和行高变化同一帧；contentY 只有用户和「回到底部」会改（跟随时视口自己变高变矮才贴回底端），不闪、不打断滚动。
  内容的坐标可以是负的，滚动范围（`minContentY` / `maxContentY`，内容不满一屏时相同、底部对齐）换成 Flickable 的 topMargin / contentHeight / bottomMargin。
  `MessageListModel` 仍是倒序（第 0 行最新、排在最下面）；排队的消息在最下面；往上翻到离顶不到一屏时自动往前补一批（v2.9 去掉了「显示更早的…」按钮；锚点只在消息行里选，
  往前补出的消息插在正在看的内容上面、往上长）。模型重建（切分支、删除）后按消息 id 找回正在看的那一条。
  测法：逐帧（`frameSwapped`）记视口里那一行的内容坐标 y 和 contentY，滚动中、停着时让下面的行变高变矮、流式出字、一轮轮出工具卡片、
  往前补 60 条，都是 0 位移。
- **消息列表的滚动条**：从工具栏下沿一直到窗口底边（用户要求 2026-10-05；原来停在输入栏那段底板上面），叠在底板、渐隐和「回到底部」上面。
- **全部行都建出来**（2026-10-08，用户「在底部的时候要往上滚很多次才能滚上去」）：MessageList / AssistantTimeline 的行（模型暴露出来的那一页）
  一直全部建着（v2.6 起是 Repeater，原来是 ListView 的 `cacheBuffer: 1000000` 加 `BuildAllRows`），内容高度是精确的、来回翻不重建；
  离视口一屏以外的剔除（`QQuickItemPrivate::setCulled`，同 ListView 对视口外委托的处理：不剔除的话长对话每帧都画全部行，掉到 20 帧）。
  另外滚轮一往上就停止跟随，「在底部」只留 4 的容差（原来 60，往上滚一点又被贴回底部）。
- **后台任务面板**（v2.0 前叫 Agent 树，用户要求 2026-10-05）：嵌在窗口里的卡片（见上文「分隔条」）。顶部没有底色卡片：「后台任务」（headline）+ ⓘ
  （悬停是这段对话全部 Agent 的速度和用量，`AgentTreeModel.usageHelp`），右边灰字「N 个 · 运行中 M」；只有待确认（橙）、失败（红）有时才出小标签
  （`AgentTreeStats.approval` 是运行中里等确认的）。轮次标题只有 1 层时不写「· 1 层」。子 Agent 卡片两行：左边一列状态图标（运行中转圈），有下级时展开箭头叠在它下面（不留空位；树的竖线在上级状态图标正下方，每层缩进 20）；名字 · 模型（三级色灰字，
  悬停看 usage.detail）·「并行」· 下级个数 · 最右运行中的实时速度 / 结束后的生成 token；第二行在做什么或结论；速度 · 生成 · 输入在悬停提示里。
  详情没有底色卡片：返回路径「‹ 后台任务 › 上级 …」、状态 + 名字（title3）、一行灰字（模型 · 速度 · 生成 · 输入 · N 次工具）、运行中的停止键；
  任务 / 结论是小标题 + 正文（先 3 行，截断了才有「展开全文」）；过程直接是工具组，新的在上面。工具栏图标 `checklist`。

## 5. Panel 与 Capsule（取代 GlassPanel / GlassCapsule）

### Panel（Item）

一块带内容的平面底：输入框外壳、底部操作条、搜索框外壳、「回到底部」这类可点的块。
底色（默认 palette.base，同输入框）+ 1px 分隔线色描边 + 小圆角；可点时悬停 / 按下叠淡墨色。implicit 尺寸 = 第一个子项的 implicit 尺寸 + 内边距。

| 属性 / 信号 | 说明 |
|---|---|
| 默认子项 | 放进内容区；`contentItem`（只读）是内容区 |
| `radius`（`Theme.radius.panel` = 6） | 胶囊形状写 `radius: height / 2` |
| `color`（`Theme.controlBackground`） | 底色 |
| `tint`（透明） | 状态叠加色，例如拖放目标 `Theme.opacity(Theme.accent, 0.18)` |
| `borderColor`（`Theme.separator`）`borderWidth`（1） | |
| `interactive`（false） | 整块可点：悬停 / 按下高亮，发 `clicked()`（只有指针；要能 Tab 聚焦、空格触发的块用 `T.AbstractButton` + `background: Panel { … }`，见 EmptyStateView 的建议按钮） |
| `inputFocused`（false） | 当输入框外壳用时绑到里面输入框的 `activeFocus`：按当前样式画原生文本框的聚焦样子——FluentWinUI3 描边不变、底边一条 2px 强调色（两端随圆角收进去）；Fusion / Windows / macOS 整圈描边换成强调色（输入栏、对话搜索条） |
| 只读 `hovered` `pressed` | |
| `help` | 原生提示 |
| `padding` `leftPadding` `rightPadding` `topPadding` `bottomPadding` | 内边距 |

GlassPanel 去掉的：`backdrop` `variant` `shadowEnabled` `shadowRadius` `shadowOffset` `shadowColor` `baseColor`（→ `color`）`live` `backdropFill`，按下时的放大。

### Capsule（T.AbstractButton）

小按钮块（chip）：输入框里工具行的「附件 / 权限 / 工作目录 / 深度思考 / 模型」，筛选、地区切换等。
按钮底色（palette.button）+ 1px 分隔线色描边 + 胶囊圆角（v2.0 起真胶囊，`radius` 默认 `height / 2`）；`bordered: false`（输入框的工具行）
没有底色和描边、只在悬停 / 按下时出淡底；悬停 / 按下叠淡墨色；状态用 `tint` 叠色；键盘焦点时 2px 强调色描边；禁用时整体 0.6 透明
（WorkingDirectoryChip 锁定时自己把 opacity 设回 1、换三级色）。

| 属性 / 信号 | 说明 |
|---|---|
| `text` `iconName` | |
| `foreground`（`Theme.secondaryLabel`）`iconColor`（= foreground） | |
| `textStyle`（`"caption"`）`weight`（`Font.Medium`）`elide`（中间截断）`maximumWidth` `monospacedDigit`（false） | |
| `color`（`Theme.controlFill`）`tint`（透明）`borderColor`（`Theme.separator`）`radius`（`height / 2`）`bordered`（true） | `tint` 只表示状态（开启 / 选中：`Theme.opacity(Theme.accent, 0.18)`）；原来 chipGlass 的 `Theme.opacity(Theme.primary, 0.08)` 这类玻璃着色**删掉** |
| `interactive`（true） | false 时不做悬停 / 按下高亮 |
| `leading`（Component，如进度环）`trailingIcon` `trailingColor` | 输入区底栏的胶囊只放图标 + 当前值，不放下拉箭头（== `.menuIndicator(.hidden)`） |
| `help` `menu`（T.Menu，点击在下方弹出） | |
| `horizontalPadding 10` `verticalPadding 5` `spacing 6`；`clicked()` 等 AbstractButton API | |

GlassCapsule 去掉的：`backdrop`、按下放大。

---

## 6. 删除的组件 → 用什么代替

| 删除 | 代替 |
|---|---|
| `GlassEffect` | 可点的圆形 / 胶囊按钮 → 原生按钮（发送 / 停止：`QQC.RoundButton { highlighted: canSend; icon.source: Theme.iconSource("arrow.up") }`，或 Button / ToolbarButton）；带内容的块 → `Panel`；纯装饰（空状态大图标的圆底、卡片底）→ 平面 `Rectangle`（`Theme.opacity(Theme.accent, 0.12)` 之类的 Theme 颜色）或直接去掉 |
| `GlassPanel` | `Panel`（同名属性照搬，去掉玻璃属性） |
| `GlassCapsule` | `Capsule`（同上；`component X: GlassCapsule` → `component X: Capsule`） |
| `GlassContainer` | `Row` / `RowLayout`（`spacing` 照搬）；`grouped: true` 的去掉共用底 |
| `GlassButton` | `Button`（`prominent: true` → `buttonStyle: "borderedProminent"`） |
| `Shadow` | 删掉，不画阴影 |
| `ScrollEdgeEffect` | 删掉；内容在工具栏下沿剪掉（工具栏行与下面的内容同色、不画分隔线），底部操作条不透明且自带描边。确实需要分界时用 1px `Theme.separator` 的 Rectangle 或 `Divider` |
| `WindowDragArea` | 删掉（拖动 / 双击最大化由系统标题栏或合并标题栏的原生命中测试负责；想让某块当标题栏空白处用 `titleBarDragArea: true`） |
| `TrafficLights` / `WindowChrome` | 删掉（系统画的标题栏按钮） |
| `Gallery.qml`、`qt/tools/style-gallery/` | 删掉（玻璃展示与自检工程）；验证改用第 9 节的截图流程 |
| `platform/WindowFrameHelper`（C++） | 删掉（原生窗口不需要）；`platform/WindowsTitleBar` 是新的、只在 Windows 上：合并标题栏的命中测试与 DWM 设置，不画任何外框 |
| `fonts/Inter*`、`shaders/**`、`tools/build_shaders.sh` | 删掉（系统字体；没有着色器、不需要 Qt ShaderTools） |

**删除的属性**（出现就删）：任何 `backdrop:`、`glassBackdrop`（含视图里自己声明的 `property Item glassBackdrop` 与给玻璃垫底的
`backdropLayer` 之类的层，若只为玻璃存在）、`backdropFill`、`glass:`、`shadowRadius / shadowOffset / shadowColor / shadowEnabled`、
`refraction`、`blurRadius`、`variant`（玻璃）、`baseColor`、`live`；`buttonStyle: "glass"` → `"bordered"`、`"glassProminent"` →
`"borderedProminent"`；`fieldStyle: "glass"` → `"roundedBorder"`（SearchField 的默认样式也不再是玻璃）。

自查：`grep -rnE "Glass|backdrop|ScrollEdgeEffect|Shadow|WindowDragArea|trafficLight|Theme\.glass|Theme\.appearance|radius\.window|radius\.sidebar|windowBorder|\"glass" qt/src/app`
应当没有结果（`GlassBackdrop.swift` 这类 Swift 文件名出现在注释里可以）。

---

## 7. 其余组件（原生控件的包装 / 平面自绘；公开 API 照旧，玻璃 API 已去掉）

| 组件 | 实现 | 注意 |
|---|---|---|
| `Button` / `IconButton` | `QQC.Button`：bordered → 默认；borderedProminent → `highlighted: true`；borderless / plain → `flat: true`；link → `flat` + `palette.buttonText: Theme.link`（或自绘 Text）；`role: "destructive"` → 红色文字用 `palette.buttonText`；`controlSize` 只调字号 / 内边距，不改委托（small / mini 的有边框按钮上下至少各留 3，Fusion 的按钮面不至于顶住文字）；`defaultAction` / `cancelAction` 照旧 | 保留 `buttonStyle controlSize role iconName labelStyle tint foregroundColor textStyle weight iconSize iconWeight help menu menuIndicator defaultAction cancelAction radius`（`radius` 在原生按钮上可忽略）；玻璃时代的 `"glass"` / `"glassProminent"` 已删（用 `"bordered"` / `"borderedProminent"`） |
| `Toggle` | 根是 Item：文字（Label，点它也会切换）+ 原生 `QQC.Switch`（`switchItem`，开关不带 text） | `text checked toggled() tint help controlSize`（`controlSize` 忽略）；新增 `spacing hovered pressed switchItem`；没有 `toggle()`。辅助功能的按下 / 切换（读屏、自动化）和点击一样：切换并发 `toggled()`（onToggled 里的保存照常执行；包装与内部开关上都接了 `Accessible.onPressAction / onToggleAction`——附加属性的处理先于 Qt 的默认处理，不是改委托）；没有文字时内部开关沿用视图设在包装上的 `Accessible.name` |
| `Checkbox` | `QQC.CheckBox` | |
| `SegmentedControl` | 一排 `QQC.Button { checkable; checked; autoExclusive }` 或 `QQC.TabBar` + `QQC.TabButton` | `activated(index)` |
| `PopUpButton` | 按钮是原生 `QQC.ComboBox`；**下拉列表自绘**（v2.0）：`popup` 是和 `Menu` 一样的底板（`Theme.menuBackground` + 1px `menuBorder` + `RectangularShadow`，圆角 `radius.menu`），在按钮下方、与按钮同宽（至少 160），最高 420；`delegate` 一行是勾选列（当前项打勾）+ 文字，悬停 / 键盘高亮是 `menuHighlight` 圆角实底、文字换 `menuHighlightText`。宽度按最宽一项（`WidestText`），放不下时 `displayText` 先按可用宽度省略成「…」（QStyle 绘制的 Windows / macOS 样式除外：内边距随文字变，省略会成绑定环）；省略了且没有 `help` 时悬停提示全文（只读 `truncated`） | 保留 `model` `currentIndex` `pickerStyle`（忽略）`placeholder` `activated(index)` 与 `popupMenuForTest()`（返回组合框的 popup）；`{…}` 对象项显示 `text`，`enabled: false` 置灰不可选（图标、副标题、分隔、标题不画） |
| `Menu` / `MenuItem` / `MenuSeparator` / `MenuHeader` / `ContextMenuArea` | **自绘**（v2.0，`T.Menu` / `T.MenuItem` / `T.MenuSeparator`，各平台一样）：底板 `Theme.menuBackground`、1px `menuBorder`、圆角 `radius.menu`（8）、`RectangularShadow` 柔和阴影，内边距 5，淡入（90 ms，带一点放大）/ 淡出；菜单项一行：勾选列（菜单里有可勾选 / 单选项时整列留出）· 图标列（有带图标的项时整列留出）· 标题（+ 副标题）· 快捷键提示或子菜单箭头（chevron.right），悬停 / 键盘高亮是 `menuHighlight`（强调色）圆角实底、文字和图标换 `menuHighlightText`（== NSMenu），禁用置灰，`destructive` 红字；分隔线是左右缩进 8 的 1px 线；MenuHeader 是二级色小号半粗文字 | 保留 `popupBelow(anchor, gap)`、`popup()`、`addItem` / `insertItem` / `addMenu`、`iconName`、`leading`、`subtitle`、`shortcut`、`destructive`、`selectable/selected`（单选：选中的那项打勾，点按不会自己翻转）、`checkable/checked`、`hasCheckColumn` / `hasIconColumn`。Menu 的 `margins` 是 8（阴影不被窗口边裁掉），太高时 ListView 可滚（设计系统的 ScrollBar） |
| `TextField` / `SearchField` / `TextArea` | `QQC.TextField` / `QQC.TextField` + 放大镜、清空按钮（作为额外子项 + padding）/ `QQC.TextArea`（在设计系统的 `ScrollView` 里） | 保留 `input`、`textArea`、`forceInputFocus()`、`clear()`、`accepted()`、`escapePressed()`、`toolbar`（工具栏里的高度 = `toolbarItemHeight`，字号与原生 ToolButton 相同——Fluent 12px，不跟 `textStyle`）；SearchField 默认 `fieldStyle` 是 `"roundedBorder"`；`"plain"` 隐藏样式外框，**只用于自带外框的容器里**（`Panel { inputFocused: field.activeFocus }` 的搜索条）；表单行里的输入框（ServiceFieldRow、新建脚本工具、提供方选项 ProviderOptionField、MCP 服务表单 MCPServerForm）一律用带边框的原生文本框，一眼看得出能在哪里输入。`Windows` 样式（NativeStyle）的 TextArea 只有一块白底、没有边框，TextArea 在 bordered / filled 下额外叠一个 1px 外框（额外子项，挂在 ScrollView 上、不接收鼠标；平时 `palette.mid`、聚焦时强调色），其余样式用自己的边框；滚轮：内容放得下（框跟着内容长高）时 `wheelEnabled` 关掉、交给外层页面，放不下时才在框里滚——ScrollView 没有可滚的距离也会吞掉滚轮，设置 › 工具的源码编辑器长到一千多像素时指针下的整页滚不动 |
| `Slider` / `Stepper` / `ProgressBar` / `Spinner` | `QQC.Slider` / 文字 + 原生 `QQC.SpinBox`（`spinBox`；数字框可以输入，值夹在 from…to 里，回车或离开时生效；宽度只放得下最长的数——QStyle 绘制的样式除外；数字会和文字里的数字各显示一次，只支持整数）/ `QQC.ProgressBar` / `QQC.BusyIndicator` | Spinner 的 `color` 忽略，mini 是 12px（Windows 样式小于 12 不画）。Slider：FluentWinUI3 在 `stepSize` 整除量程时每一步画一道刻度（上下两排，`(to - from) / stepSize + 1` 个），步数多时挤成一片、量程大时委托数暴涨——步数多（提供方选项 > 20）时用 `stepSize: 0` 连续拖动（显示、保存取整），左右方向键自己按 1 步进（`Keys.onLeftPressed / onRightPressed`，见 ProviderOptionField）。Stepper 新增 `showsValue: false`：只有「减少 / 增加」两个原生小按钮（⌄ ⌃，同 Fluent SpinBox 的顺序；按住连续增减），不显示 SpinBox 的数字框——数已经在旁边自己的输入框里时用（ProviderOptionField 的数字选项：输入框能留空 = 用默认）；没有 `text` 时内部 SpinBox / 按钮的辅助功能名字沿用视图设在包装上的 `Accessible.name`。用户改动时 Stepper 自己给 `value` 赋值，调用方的绑定会断（同 `QQC.SpinBox`）：`onValueModified` 里保存后用 `Qt.binding` 重新绑定，否则别处再改值（输入框、恢复默认）后下一次从旧数接着加减 |
| `ScrollView` / `ScrollBar` | `QQC.ScrollView` / 自绘的 `T.ScrollBar`（ScrollView、TextArea 也换成它，位置同样式自带的那根）。**Windows**（三种样式一样）：滑块宽 8、悬停 / 拖动时 10 并露出浅轨道，墨色 45% / 60% / 72%（浅色、深色窗口底上都在 3:1 以上），内容放不下就一直显示、不淡出，放得下时整根隐藏；**其余平台**照 Fusion：6px 细条，滚动时出现、停下淡出 | 内容放得下时 `wheelEnabled` 关掉，滚轮交给外层（嵌在可滚动列表 / 页面里的小滚动区不挡整页滚动）；v2.6 鼠标不能按住拖着内容滚（`acceptedButtons: Qt.NoButton`，同侧边栏、对话 / 助手列表、后台任务面板；滚轮、触控板、触摸、滚动条照常；用户要求 2026-10-10）；`Form` 关掉横向滚动条；Fluent / Fusion 下滚动条盖在内容上，常显的滑块（离右边 3–11）不能压字：列表 / 表单右边留够边距，贴边的文字区自己让开（输入栏正文超出上限时右内边距 = 滚动条宽；工具卡片的参数 / 结果框让滚动条落在框的右内边距里）；Windows 样式的竖向滚动条占宽度，`Form` 的内容宽度这时取「宽度 - 左内边距 - 滚动条宽」（始终给滚动条留位置，不读 `availableWidth`），否则「换行 → 内容高度 → 滚动条出现 → 可用宽度」会转成绑定环 |
| `Tooltip` / `HelpArea` | `QQC.ToolTip` 的包装（`delay: 800`），HelpArea 用 HoverHandler + 附加 ToolTip | |
| `Sheet` / `Alert` | `QQC.Popup`（modal，窗口居中）/ `QQC.Dialog`（标题、按钮用 `QQC.DialogButtonBox` 或原生 Button 行） | 保留 `footer` `contentPadding` `backgroundColor`、`accepted()` `rejected()` `accept()`；Sheet / Alert 的模态遮罩（`ModalDimmer`）与 `topMargin` 见第 2 节规则 6。Alert 的确认按钮是强调按钮（destructive 时底色是 `Theme.destructiveFill`）：Qt 6.11 起 `DialogButtonBox` 有 `defaultButton`，打开时只让它保持 `highlighted`（按钮上写的 `highlighted: true` 会被置回 false），所以有这个属性时把确认按钮设成它；6.8–6.10 没有这个属性、也不置回，照旧靠 `highlighted: true`；自己用 `DialogButtonBox` 时同样处理。`closesOnConfirm: false`：点确认（回车）不关、只发 `confirmed()`，由调用方 `close()`（确认后要跑一阵的，如添加远程机器时用密码登录；Dialog 对 AcceptRole 是先发 accepted 再 close()，onAccepted 里 open() 留不住，所以这时确认按钮换成 ActionRole） |
| `Card` `Badge` `Tag` `ListRow` `SidebarRow` `SidebarSection` `DisclosureGroup` `Form*` `Divider` `ContentUnavailableView` `Label` | 平面自绘（第 2 节规则 3）；ListRow 的选中用 `Theme.selection / selectionText`，SidebarRow 用 `Theme.unemphasizedSelection` | |
| `StyledText` / `SelectableText` / `Icon` | 不变（字体取 `Theme.fontFamily`；Icon 照旧按 SF 名解析 Lucide） | |

---

## 8. 文字与图标

**StyledText**（Text）：`textStyle`（文本样式名或数字）、`weight`、`monospaced`、`monospacedDigit`；默认 `color: Theme.label`、
`font.family: Theme.fontFamily`、渲染方式用默认的（与原生控件相同）。**SelectableText**（只读 TextEdit）：同上，可拖选复制。v2.6 对话页、助手页的列表外面包一层 `SelectionArea`（C++）：从一段拖到别的段落、别的消息（或从空白处开始拖）能跨段选，拖到上下边自动滚，Ctrl+C / 右键「拷贝」拷全部（段与段之间空一行）；一段之内的拖选、双击选词仍是原生的（用户要求 2026-10-10）。Swift 版同样能跨消息选：消息正文、用户气泡、思考过程改用 `Views/SelectableText.swift`（TextKit 1 的只读 NSTextView + TextSelection 统一管选区）。
**不要写 `renderType: Text.NativeRendering`**：Windows 150% 缩放时，落在半个物理像素上的原生渲染文字会丢 / 重一行像素
（「一」、连字符、F / e 的横笔消失）；原生控件本身也用默认渲染方式。

**Icon**：`Icon { name: "checkmark.circle.fill"; color: Theme.green; size: 16; weight: Font.Normal; pulsing: false }`
- `name` 用 Swift 里的 SF Symbol 名（数据里存的也是这些名字，如 `ToolEvent.symbol`、`CapabilityKind.symbol`、`OnlineService.symbol`）。
- `size` 是图标框边长；Lucide 在 24 格里留了 2 格边距（`Theme.iconSize(style)` = 字号 × 1.25）。
- `weight`：`Font.Normal/Medium` → 笔画 2.0，`Font.DemiBold` → 2.35，`Font.Bold` 及以上 → 2.75。
- `.fill` 变体是实心 / 镂空（QtSvg ≥ 6.7）。`pulsing` = `.symbolEffect(.pulse)`。未知名字回退为 `circle-help` 并警告一次。

原生控件的图标：`icon.source: Theme.iconSource(name, weight)` + `Theme.adoptIconColor(...)`（第 2 节规则 4）。

**图标映射**：`icons/mapping.json` 覆盖 Swift 源码里出现的全部 SF Symbol 外加设计系统自用的（`sidebar.left`、`chevron.down`、`xmark`、
`ellipsis`、`chevron.forward.2` …）。新增：在 mapping.json 加一行 `"sf.name": { "lucide": "lucide-name" }`（可选 `fill: solid/knockout`、
`extra`、`compose`），然后 `python3 qt/tools/gen_icons.py`（`--lucide` 或 `LUCIDE_ICONS` 指定 Lucide 图标目录），提交生成物。

---

## 8.5 图标（v2.5 Friday 的光环）

App 图标、Windows 的 friday.ico、Linux 的 png、macOS 的 AppIcon.appiconset 和菜单栏模板图、Qt 在 macOS 上的 tray.png 都由 `visual/HaloRenderer`
画（`appIcon` / `trayIcon` / `templateIcon`），和视频通话的画面、实时语音的光环同一个；Windows / Linux 的托盘图标运行时直接用 `trayIcon` 画彩色的
（不再按任务栏深浅涂成黑 / 白）。所有图标（App 图标、托盘、菜单栏模板图）画的都是**说话时的同一帧**（用户 2026-10-09 要求，
`HaloRenderer.cpp` 的 `kTalkLevel` / `kTalkMoment`），桌面、程序坞、任务栏、托盘看起来是同一个。
- Windows 的 App 图标（用户 2026-10-09 要求）：深色圆角方块**铺满整张图、不留边、不画投影**（`appIcon(size, true)`），任务栏里和别的 App 一样大；
  macOS 的仍按系统网格四周留边带投影（用户确认保持；macOS 27 实测会把任何图标——包括铺满的方图——缩进同一大小的圆角方块，
  外框铺不到边），Linux 用 macOS 那套 png。Windows 上 main.cpp 不设窗口图标，Qt 的窗口类用 exe 里的 IDI_ICON1（friday.ico），
  任务栏、Alt+Tab 每个尺寸用画好的那张。
- 托盘图标（Windows / Linux）：**说话时的样子**（外沿起波纹、更亮；音量 0.6、波峰高低不齐的一帧，音量大了波峰整齐像个螺母），
  **裁到环碰到图的边、不留边**。托盘只有二十来个像素（用户 2026-10-09 嫌不清晰）：环不透明（先铺底色、彩色叠上去、再按环的形状抠）、
  没有光晕；Windows 上 Qt 6 的托盘按系统小图标尺寸（已是物理像素，150% 时 24）取图时又乘了一遍缩放（取 36），系统再缩回 24，缩两遍发虚——
  FridayApp.cpp 的 `TrayIconEngine` 不管倍数，按要的尺寸现画。macOS 菜单栏的模板图（Swift 的 MenuBarIcon、Qt 在 macOS 上的 tray.png）同一个外形，黑色、也不留边（用户 2026-10-09 要求）。

- 界面里助手的头像（v2.5，用户 2026-10-09 要求）：侧边栏的助手那一行、助手没打开 / 在别的机器上时的占位、聊天里「助手发送」那行小字，
  都是 Friday 的光环（同托盘图标那一帧：不透明、无光晕、碰边）。Qt：`Icon { name: "friday.halo" }`（app/HaloItem 的 `HaloImageProvider`，
  图片提供者 `halo`，按要的像素现画；彩色，不跟 color 着色，助手没打开时 opacity 0.45）；Swift：`AssistantAvatar(size:)`（资源 AssistantAvatar.imageset）。
  SChat 里机器人的头像是 `HaloRenderer::avatar`（深色方图，切成圆也切不到环），一键接入时传。

改了画法要重新生成：`FRIDAY_HALO_EXPORT=<目录> friday_tests TestHalo exportImages` 存出各尺寸 PNG，
再把 `app-*.png` 拷进 AppIcon.appiconset（icon_16 … icon_1024）、`app-512.png` 当 resources/friday.png、`template-18/36.png` 当菜单栏图和 tray.png /
tray@2x.png，friday.ico 用 16 / 20 / 24 / 32 / 40 / 48 / 64 / 128 / 256 的 `windows-*.png` 拼（每个尺寸用渲染器画好的那张，不让工具自己缩）；
`tray-64/128/192.png` 当 Swift 的 AssistantAvatar.imageset（assistant_avatar_64 / 128 / 192）。

## 9. 验证：构建与截图

先按 `qt/README.md` 构建 `friday`（同一时间只跑一个构建），再用 DebugHarness 的环境变量跑截图脚本（步骤与选择器见
`src/app/DebugHarness.h`）。脚本跑完自动退出，截图存到 `FRIDAY_SCREENSHOT` 目录，文件名 `<步骤>-<light|dark>.png`：

```bash
# Linux / macOS（Windows 的 PowerShell 里用 $env:NAME = "…" 设同样的变量）
export QT_QUICK_CONTROLS_STYLE=Fusion          # 覆盖 main.cpp 的选择：FluentWinUI3 / Windows（只在 Windows 上有，固定浅色）/ Fusion；找不到的样式被忽略
export FRIDAY_COLOR_SCHEME=dark                 # light / dark，各跑一遍
export FRIDAY_SCREENSHOT=shots/Fusion
export FRIDAY_SCREENSHOT_SCRIPT='main;settings;settings:tools;settings:secrets;models;about;sheet:search;sheet:newTool;menu:app;menu:model;menu:permission'
./friday 2> shots/Fusion/log-dark.txt           # 警告看 stderr（Windows 上再设 QT_FORCE_STDERR_LOGGING=1）
```

- 至少看 **FluentWinUI3**（Win11）和 **Fusion**（Linux 的样子）；动了原生控件的再看 **Windows**（Win10，最容易出
  「does not support customization」）。
- 截图是 `grabWindow()` 的窗口内容，**不含系统标题栏和按钮**（合并标题栏的窗口是透明的，harness 截图时垫上 `backgroundColor`）。
  标题栏本身（按钮、悬停、Snap Layouts、拖动…）只能用真实屏幕截图 + 模拟鼠标验证（`DwmGetWindowAttribute(DWMWA_EXTENDED_FRAME_BOUNDS)`
  取窗口可见范围、`CopyFromScreen` / `BitBlt` 截屏、`SetCursorPos` + `mouse_event`、`SendMessage(WM_NCHITTEST)` 查命中测试），
  与 `FRIDAY_TITLEBAR=system` 的系统标题栏对照。
- 布局对照改造前（玻璃观感）同一脚本的截图：元素的顺序、位置、文案应当一致
  （工具栏行高 52 → 48、去掉红绿灯后的左侧让位、原生控件比 macOS 的高几像素等细微差别是预期的）。
- 警告里**不允许**出现：`does not support customization`、`is not a type`、`Cannot assign to non-existent property`、`ReferenceError`、
  `TypeError`、`Binding loop`、`未知的 SF Symbol`。脚本结束时 harness 打印「QML 警告 N 条（引擎 a、console.warn / error b）」，应当是 0：
  引擎的 `warnings` 信号（绑定环、TypeError、does not support customization…）加上分类为 `qml` / `js` 的 console.warn / error
  （「未知的 SF Symbol」走这一路），从加载 Main.qml 之前开始算（`DebugHarness::countWarnings`）。其他分类的 C++ 警告不计，照样要看日志。

**卡不卡**（v1.17）：`stall:start` 开始记主线程被占住的时长，`stall:report:<名字>` 打印最长一次、超过 50 ms 的次数与合计；
`until:!LoadScheduler.busy` 等占位都换成内容；`snap:<名字>` 立刻截图（看占位、过渡中间的样子，`shot` 要等渲染稳定）。
设 `FRIDAY_LOAD_LOG=1` 时 LoadScheduler 每建一块打一行耗时、每轮打一行汇总。例：

```bash
export FRIDAY_SCREENSHOT_SCRIPT='main
wait:3000
stall:start
eval:ConversationStore.selectedID = "<会话 id>"
snap:switch-30ms
until:!LoadScheduler.busy
stall:report:切会话
eval:AgentTeam.treeFocus = "<子 Agent 的事件 id>"
until:!LoadScheduler.busy
stall:report:打开详情
quit'
```

消息列表里收起的内容（思考过程、工具的参数和结果、详情页的任务和结论）第一次展开时才建（== Swift 的 `if expanded`）；
切会话、往前加一批时建出来的块先占位（DeferredLoader），流式中的块直接建。新写的卡片要是有「收起时看不见」的大段文字，也照这样做。

**切页的骨架屏**（v2.3，用户要求 2026-10-07；== Swift `Views/LoadingSkeleton.swift`）：切到有消息的会话、开着的助手时，ContentView 这一帧先盖一页
骨架屏（`qml/PageSkeleton.qml`：新页面的标题 + 按它的卡片布局摆的卡片骨架（里面放一个 `showsLoading: false` 的 PanelDock）+ 对话区里按列宽贴底排的灰条——
输入框轮廓、几行字、工具卡片、右边的用户气泡），一道高光从左往右扫过（`qml/ShimmerShapes.qml`，三种骨架共用），
32 ms 后才建真正的页面；页面的 `contentReady`（MessageList 首次排版完、`LoadScheduler.hasVisiblePending(视口)` 为 false 再排一帧；空会话马上；
助手页建出来一帧后）变 true 时骨架屏淡出、页面淡入（180 ms）。空会话、没有对话直接换；5 秒兜底收起。块占位仍是静态灰条（同 Swift：那边每块一个
NSView，切一次会话多占主线程 50–110 ms）。高光是 XAnimator，在渲染线程上动——GUI 线程建页面时照样在扫。**坑**：XAnimator 只在启动时读
`from` / `to`，之后绑定变了也不接；绑到另算的属性上时，Repeater 建委托那一刻那个属性还没跟着新模型重算（读到 0 → 0），光带就停在原地。
起止要随委托自己的数据给（ShimmerShapes 把 `sweepFrom` / `sweepTo` 算进每块的 modelData）。截图时骨架屏一般早就收起了，看样子用
`evalon:type=ContentView::skeleton.shown = true`（`skeleton.layoutID` 换卡片布局）把它顶起来。

**卡片、侧边栏的骨架**（同一天，用户要求「后台任务 / 终端卡片和侧边列表也是一样」）：PanelDock 打开卡片时先画 `dock/CardSkeleton.qml`（标题栏 + list / lines
两种内容），32 ms 后才建内容；内容的 `contentReady` 是 false 时骨架只盖标题栏下面（标题栏的按钮照常能用）、变 true 时淡出——终端卡片等选中终端的
`hasOutput`（屏幕上出现了字：ConPTY 一启动先发的清屏、光标序列不算），最多 3 秒；后台任务、状态板没有这个属性，建出来就算好。侧边栏在
`ConversationStore.listLoaded` 为 false（远程副本还没收到服务端第一次给的元信息）、又不是连不上（RemoteBanner 的 failed）时盖 `sidebar/SidebarSkeleton.qml`。
看样子：`evalon:type=SidebarView::sidebarSkeleton.shown = true`；Swift 的 Debug 构建设 `FRIDAY_SIDEBAR_SKELETON=1` 一直盖着。

---

## 10. 已知限制 / 待办

- Windows 10 只在 Win11 上用 `QT_QUICK_CONTROLS_STYLE=Windows` 截图验证过，没有真机 Win10 验证。`Windows` 样式（NativeStyle）
  不支持深色（强制深色时整个界面仍是浅色），所以 Win10 深色启动时用 Fusion、浅色启动时用 `Windows` 并把配色固定为浅色（第 1 节）；
  运行中切换深浅色要重启 Friday 才生效。「深色启动 → Fusion」这条分支在 Win11 上走不到，只验证了 Fusion 深色本身。
- Linux 的样子只在 Windows 上用 `QT_QUICK_CONTROLS_STYLE=Fusion` + `FRIDAY_TITLEBAR=system` 模拟（调色板是 Windows 的系统调色板）；真机上调色板、字体来自桌面主题（KDE / GNOME），
  `Qt.styleHints.colorScheme` 取不到时 Theme 按调色板亮度判断深浅色。
- 菜单 / 弹出层仍是窗口内的弹出层（不能伸出窗口边界）。
- `Windows` 样式的多行文本框（TextArea）的外框是我们叠上去的 1px 线（NativeStyle 的 TextArea 本身没有边框，见第 7 节），不是 QStyle 画的：
  颜色取调色板（`mid` / 强调色），和单行文本框的边框色、悬停变化不完全一样，需要真机 Win10 再看。
- `Windows` 样式（NativeStyle：QStyle 画好的边框图按九宫格拉伸）在 150% 缩放下，文本框 / 组合框的边框偶尔画成错位的几段线
  （同一界面换个时机截图又正常；单独的 qml 测试里复现不出来），是 Qt NativeStyle 的绘制问题，需要真机 Win10 再确认。
- Linux 包没带平台主题插件（`libqgtk3.so` / `libqxdgdesktopportal.so`）：GNOME 上界面字体是 Qt 内置主题的「Sans Serif 9pt」，
  比 GNOME 默认小一号（KDE 读 kdeglobals，没有这个问题），原因与取舍见 `docs/PACKAGING.md`「原生控件样式」，待 GNOME / KDE 真机验证。
- `SelectableText` 是纯文本；Markdown 渲染属于应用层。
- 标题栏的限制见第 4 节「标题栏」（系统按钮高度固定、模态遮罩盖住按钮、未验证的场景等）。
