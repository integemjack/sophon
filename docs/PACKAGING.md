# Friday（Qt 版）打包与发布

本文是 Windows / Linux 安装包的约定：产物清单、包内目录布局、从 1.x 升级时旧文件的处理、`latest.json` 里给 Qt 版
`UpdateManager` 用的 `assets` 结构、各平台换包更新的做法，以及 Windows 安装程序（§7）与 Linux deb（§8）。脚本本身的用法见
[`qt/ci/README.md`](../ci/README.md)。

v2.0 起 Friday 的包不带本地推理：包里没有 llama.cpp / stable-diffusion.cpp 的推理库、CUDA / Vulkan 运行库，
不需要显卡，一个平台只有一种包（1.x 分 CUDA 版与 Vulkan 版）。v2.7 的本地大模型、本地生图 / 生视频是 Friday.Powers 的 Power，用到时单独下载（POWERS.md §8），不进 Friday 的包。1.x 怎么升上来见 §3、§4。

## 1. 产物清单

每次 Friday.build 发布（标签 `vX.Y.Z` 正式版 / `vX.Y.Z-beta` 预发布）包含：

| 文件 | 平台 | 说明 |
| --- | --- | --- |
| `Friday-<标签>-macOS-arm64.dmg` / `.zip` | macOS 26+，Apple Silicon | Swift 版（原有，不变） |
| `Friday-<标签>-windows-x64-setup.exe` | Windows 10 / 11 x64 | 安装程序（§7），装到 `%ProgramFiles%\Friday` |
| `Friday-<标签>-windows-x64.zip` | 同上 | 同样内容的便携版，解压即用 |
| `Friday-<标签>-linux-{x64,arm64}.deb` | Linux x64（glibc ≥ 2.35）/ arm64（aarch64，glibc ≥ 2.39），Debian / Ubuntu 系 | 安装包（§8），装到 `/opt/friday` |
| `Friday-<标签>-linux-{x64,arm64}.AppImage` | 同上 | 同样内容，单文件，直接运行 |
| `Friday-<标签>-linux-{x64,arm64}.tar.gz` | 同上 | 同一个 AppDir 的压缩包（没有 FUSE 时用） |
| `Friday-<标签>-server-windows-x64.tar.gz`、`Friday-<标签>-server-linux-{x64,arm64}.tar.gz` | Windows 10 1803+ / Linux x64（glibc ≥ 2.35）、arm64（glibc ≥ 2.39） | 远程用的无界面 friday-server（v2.1，docs/REMOTE.md §9）：Friday 连远程机器时自动下载、经 SSH 装上去，用户不用下载。顶层目录 `friday-server/`：`bin/`（Linux 另有 `lib/`、`plugins/`），`server.json` 是 `--version` 的输出加标签；package_windows.ps1 / package_linux.sh 打完会解开到别处跑一次 `--version` 核对 |
| `Friday-<标签>-windows-x64-upgrade-{cuda,vulkan}.zip`、`Friday-<标签>-linux-x64-upgrade-{cuda,vulkan}.tar.gz` | — | 只给 1.x 自动更新用（§4）：与便携版 zip / tar.gz 内容相同，只有 `package.json` 的 `asset` 是 1.x 的老键；Friday.build 发布时生成，用户不用下载 |
| `SHA256SUMS.txt` | — | 以上全部文件的 SHA-256（`<hash>  <文件名>`） |

同一个 Windows 包在 10 和 11 上都能用（§2「原生控件样式」「标题栏」）；Linux x64 包在 ubuntu-22.04 上构建，所以要求 glibc ≥ 2.35；
arm64 包只能在 ubuntu-24.04-arm 上构建（Qt 官方的 `linux_gcc_arm64` 是在 Ubuntu 24.04 上编的，要 glibc 2.38+，22.04 上连 Qt 自己的
moc / qmlimportscanner 都跑不起来），所以要求 glibc ≥ 2.39（Ubuntu 24.04、Debian 13、Fedora 40 起）。Linux 只有 x64 和 arm64 两种架构：Qt 官方只有这两种的 Linux 预编译包（`linux_gcc_64`、`linux_gcc_arm64`），
32 位 ARM、RISC-V、LoongArch 等要从源码编 Qt，不出。arm64 包从 v2.0 起才有（1.x 只有 x64）。

## 2. 包内目录布局

「程序目录」= `QCoreApplication::applicationDirPath()`，即 `friday` 可执行文件所在目录（Windows 是包的根目录 `Friday\`，Linux 是 `usr/bin/`）。

### Windows（zip，顶层只有一个 `Friday\`；安装程序装出来的程序目录与它相同，另加 `uninstall.exe`、`FridaySetupUI.exe`，§7）

```
Friday\
├── friday.exe                  图形界面（WIN32 子系统）；带命令参数时是命令行版（friday server … 是服务），v2.8 起
│                               （嵌 activeCodePage=UTF-8 清单，cmake/FridayCodePage.cmake：窄字符 API 直接收发 UTF-8 路径）
├── friday.com                  终端里敲 friday 时先跑它：把参数和标准句柄交给 friday.exe、等它结束（v2.8；v2.6–v2.7 是 friday-cli.exe，
│                               之前另有 friday-server.exe）
├── package.json                包信息：{"tag","platform":"windows-x64","asset":"windows-x64"}
├── Qt6*.dll、avcodec-*.dll 等   Qt 运行库（windeployqt）与 Qt Multimedia 自带的 FFmpeg
├── msvcp140*.dll、vcruntime140*.dll …   VC++ 运行库（应用本地部署，不用装 vc_redist）
└── qml\  platforms\  imageformats\  iconengines\  multimedia\  texttospeech\  tls\  …   Qt 插件与 QML 模块
```

1.x 的包里还有推理库（`friday-llama.dll`、`llama*.dll`、`ggml*.dll`、`stable-diffusion.dll`…）、CUDA 运行库、`backend.json`、
`licenses\` 和过渡期的 `backend\`，v2.0 都没有了（升级时怎么清掉见 §3）。

### Linux（AppImage、tar.gz、deb 是同一个 AppDir；tar.gz 的顶层目录叫 `Friday/`，deb 装在 `/opt/friday/`，§8）

```
Friday/                         （AppImage 挂载点 / tar.gz 解压目录）
├── AppRun                      启动入口（linuxdeploy 生成）
├── friday.desktop  friday.png  .DirIcon
└── usr/
    ├── bin/                    ← 程序目录：friday（带命令参数时是命令行版）  qt.conf  package.json（"asset": "linux-x64"）
    ├── lib/                    Qt 与依赖库（linuxdeploy，friday 的 RPATH=$ORIGIN/../lib）
    ├── plugins/  qml/  translations/
    └── share/applications/friday.desktop、share/icons/hicolor/<尺寸>/apps/friday.png
```

### 原生控件样式

界面用系统原生观感，Qt Quick Controls 样式在运行时按系统选（`main.cpp`）：Windows 11 `FluentWinUI3`、Windows 10 `Windows`
（两者缺的控件回退 Fusion / Basic；`Windows` 样式只有浅色，Win10 深色启动时改用 `Fusion`），Linux `Fusion`（后备 Basic）。
环境变量 `QT_QUICK_CONTROLS_STYLE` 指定的样式只有在包里找得到时才用（KDE 常设的 `org.kde.desktop` 等包里没有，会被忽略）。界面源码里没有 import 这些样式，按 import 部署的
windeployqt / linuxdeploy-plugin-qt 会漏掉，所以打包脚本单独保证 `qml/QtQuick/Controls/<样式>/`（Windows 还有
`qml/QtQuick/NativeStyle/`）和对应的 Qt 库在包里（`qt/ci/README.md`）。同一个 Windows 包在 10 和 11 上都能用。
没有玻璃效果，不需要 Qt ShaderTools，也不带额外字体（用系统字体，中文回退到微软雅黑 / Noto Sans CJK）。

**标题栏**：Windows 11 上工具栏行并进系统标题栏（`src/app/platform/WindowsTitleBar`，按钮由 DWM 画，见 `docs/STYLE.md`「标题栏」）；
同一个 Windows 包在 Windows 10 上运行时自动改用系统标题栏 + 工具栏行；Linux 包不编译这部分，始终是系统标题栏 + 工具栏行——
GNOME 这类不给服务端装饰的 Wayland 合成器上，系统标题栏由 Qt 的客户端装饰插件（`plugins/wayland-decoration-client/`）画，
`package_linux.sh` 保证它在包里、`verify_linux_package.sh` 核对（缺了窗口没有标题栏，拖不动也关不掉）。
这部分代码只链接 Windows 自带的 `dwmapi.dll`、`uxtheme.dll`（构建时用 Windows SDK 的导入库，运行时在 System32——
`package_windows.ps1` 的 dumpbin 核对本来就认 System32），没有新的 Qt 模块、私有头文件或第三方依赖：包里不用多放东西，CI 也不用多取东西。

Linux 包**没有带平台主题插件**（`plugins/platformthemes/`：`libqgtk3.so`、`libqxdgdesktopportal.so`），Fusion 的调色板、
字体、深浅色由 Qt 内置的桌面主题读取：KDE 上读 kdeglobals，和桌面一致；GNOME 上内置主题的界面字体固定是「Sans Serif 9pt」
（比 GNOME 默认的 11pt 小，Theme 的字号阶梯按它缩放，界面本身协调，只是比别的程序小一号），深浅色通过 xdg-desktop-portal 读取。
没带的原因：`libqgtk3.so` 依赖 GTK 3 / GLib，打进包会把它们一起带进来（不同发行版混用 GTK 容易出问题），不带则要在两处 ldd
核对里给它放行；`libqxdgdesktopportal.so` 只有在 Flatpak / Snap 里或设了 `QT_QPA_PLATFORMTHEME` 才会被选用，单带它不起作用。
这需要在 GNOME / KDE 真机上验证后再决定，见 `docs/STYLE.md` §10。

## 3. 从 1.x 升级：本地推理留下的文件

1.x 的 CUDA 版、Vulkan 版都自动更新到 v2.0 的这一套包（§4 的老键）。旧包里的推理库、CUDA 运行库、`backend\` 等按安装方式处理：

| 安装方式 | 旧的推理文件 |
| --- | --- |
| Windows 便携版（zip） | 换包是整个程序目录一起换（§5），旧文件随 `.old` 目录删掉 |
| Windows 安装程序装的 | 覆盖安装只写新包里有的文件，所以安装程序解包前先删（`installer/core/Install.cpp` 的 `removeObsoleteFiles`）：`backend\`、`licenses\`、`backend.json`、`friday-llama.dll`、`llama*.dll`、`mtmd*.dll`、`ggml*.dll`、`stable-diffusion*.dll` 与 CUDA 运行库（`cudart64_*`、`cublas64_*`、`cublasLt64_*`、`cufft64_*`、`curand64_*`、`cusparse64_*`、`cusolver64_*`、`nvrtc*`、`nvJitLink*`），以及 v2.6 起并进 friday-cli.exe 的 `friday-server.exe`、v2.8 起并进 friday.exe 的 `friday-cli.exe`；只认这几类名字，删不掉（被占用等）只记日志、不挡安装 |
| deb | 包名同为 `friday`，dpkg 升级时删掉旧包里有、新包里没有的文件 |
| AppImage | 单文件，整个替换 |
| tar.gz | 整个目录一起换（§5） |

用户数据不受影响。1.x 下载的模型（默认在 `~/.cache/modelscope`、`~/.cache/huggingface`，与 Python SDK 共用）、KV 缓存
（`<缓存目录>/KVCache`）和推理日志（`<缓存目录>/Logs/`）v2.0 不再读写，也不自动删，用不着了可以手动删掉；存档和设置里与本地模型
有关的内容读的时候忽略（`docs/ARCHITECTURE.md` §5）。

## 4. latest.json 的 assets（给 Qt 版 UpdateManager）

Friday.build 根目录的 `latest.json` 按渠道（`stable` / `beta`）各记最新一次发布。原有字段不变（macOS 版照旧读 `zip`、
`sha256` 等；Swift 的 `ReleaseEntry` 是编译器合成的 `Codable`，`JSONDecoder` 会忽略多出来的键），每个渠道条目新增
`assets`：

```json
{
  "stable": {
    "tag": "v2.0.3",
    "version": "2.0.3",
    "build": 42,
    "channel": "stable",
    "date": "2026-10-01T02:15:00Z",
    "zip": "https://github.com/integemjack/Friday.build/releases/download/v2.0.3/Friday-v2.0.3-macOS-arm64.zip",
    "dmg": "…",
    "sha256": "…",
    "size": 29753513,
    "notes": "提交说明",
    "minOS": "26.0",
    "release": "https://github.com/integemjack/Friday.build/releases/tag/v2.0.3",
    "source": "<源码提交>",
    "branch": "main",
    "assets": {
      "windows-x64": {
        "name": "Friday-v2.0.3-windows-x64.zip", "url": "…", "sha256": "…", "size": 123456789,
        "installer": { "name": "Friday-v2.0.3-windows-x64-setup.exe", "url": "…", "sha256": "…", "size": 120000000 },
        "server": { "name": "Friday-v2.0.3-server-windows-x64.tar.gz", "url": "…", "sha256": "…", "size": 12800000,
                    "version": "2.0.3", "protocol": 1 }
      },
      "linux-x64": {
        "name": "Friday-v2.0.3-linux-x64.AppImage", "url": "…", "sha256": "…", "size": 130000000,
        "tarball": { "name": "Friday-v2.0.3-linux-x64.tar.gz", "url": "…", "sha256": "…", "size": 135000000 },
        "deb": { "name": "Friday-v2.0.3-linux-x64.deb", "url": "…", "sha256": "…", "size": 110000000 },
        "server": { "name": "Friday-v2.0.3-server-linux-x64.tar.gz", "url": "…", "sha256": "…", "size": 34000000,
                    "version": "2.0.3", "protocol": 1 }
      },
      "windows-x64-cuda": {
        "name": "Friday-v2.0.3-windows-x64-upgrade-cuda.zip", "url": "…", "sha256": "…", "size": 123456789,
        "installer": { "name": "Friday-v2.0.3-windows-x64-setup.exe", "url": "…", "sha256": "…", "size": 120000000 }
      },
      "windows-x64-vulkan": { "name": "Friday-v2.0.3-windows-x64-upgrade-vulkan.zip", "…": "…", "installer": { "…": "…" } },
      "linux-x64-cuda": {
        "name": "Friday-v2.0.3-linux-x64.AppImage", "url": "…", "sha256": "…", "size": 130000000,
        "tarball": { "name": "Friday-v2.0.3-linux-x64-upgrade-cuda.tar.gz", "url": "…", "sha256": "…", "size": 135000000 },
        "deb": { "name": "Friday-v2.0.3-linux-x64.deb", "url": "…", "sha256": "…", "size": 110000000 }
      },
      "linux-x64-vulkan": { "name": "Friday-v2.0.3-linux-x64.AppImage", "…": "…", "tarball": { "…": "…" }, "deb": { "…": "…" } }
    }
  },
  "beta": { "…": "…" },
  "updatedAt": "2026-10-01T02:15:00Z"
}
```

- 键名 = `<BuildInfo::platform>-<BuildInfo::arch>`（`CurrentBuild::assetKey()`），即 `windows-x64`、`linux-x64`、`linux-arm64`
  （`linux-arm64` 条目的结构与 `linux-x64` 相同，上例从略）。
- `server`（v2.1 起）：远程用的 friday-server 包，不是给这台机器更新用的——客户端连远程机器时按**远程**的平台取（Qt 的 RemoteDeploy、
  Swift 的 RemoteDeploy.swift；macOS 版的 `ReleaseEntry` 只解 `assets.*.server`）。`version`、`protocol` 由 release job 从包里的
  `friday-server/server.json` 读出来；客户端只装 `protocol` 和自己一样的。
- **给 1.x 的老键**：1.x 按 `<平台>-<架构>-<后端>`（`windows-x64-cuda`、`linux-x64-vulkan`…）找包，便携 zip 和 tar.gz 解开后还核对
  `package.json` 的 `asset` 与这个键一致（安装程序、AppImage、deb 不核对）。所以 Friday.build 发布 v2.0 起的版本时另挂
  `windows-x64-cuda`、`windows-x64-vulkan`、`linux-x64-cuda`、`linux-x64-vulkan` 四个键：`installer`、AppImage（条目本身）、`deb`
  直接指向新包，zip（条目本身）与 `tarball` 换成 release job 重新打的 `-upgrade-<后端>` 包（内容相同，只把 `package.json` 的
  `asset` 改成老键）。1.x 的 CUDA 版、Vulkan 版就这样都升到同一套包，升上来以后按新键走。源码还带本地推理（v2.0 之前的分支）时
  照旧只有这四个带后端的键（§6）。
- 版本比较、渠道选择与 Swift 的 `ReleaseVersion` / `UpdateManager.pick` 完全一致（按 `tag` 解析；正式版渠道只看
  `stable`，Beta 渠道在 `stable` / `beta` 里取最高），只是把 Swift 里的 `runsOnThisMac` 过滤换成「条目的 `assets`
  里有自己的键」。
- 某次构建某个平台失败时，那个渠道条目里就**没有**对应的键：视为「这个版本没有你的平台的包」，保持现状、下次再查，
  不要报错。
- 下载后必须校验 `size` 与 `sha256`。Windows / Linux 包没有代码签名，SHA-256 是唯一的完整性校验；`latest.json`
  从 `raw.githubusercontent.com`（备用 jsDelivr）取，走 HTTPS。
- 同一个键下取哪个文件按这份 Friday 是怎么装的（`CurrentBuild::packageFormat()`）：Windows 便携版（zip）取条目本身，
  安装程序装的（程序目录里有 `uninstall.exe`）取 `installer`；Linux 用 AppImage 运行时（环境变量 `APPIMAGE` 存在）取条目本身，
  deb 装的（程序在 `/opt/friday/usr/bin`，且 `/var/lib/dpkg/info/friday.list` 存在）取 `deb`，从 tar.gz 解压运行时取 `tarball`。
  条目里没有那种文件（老版本、或这次那个格式打包失败）同样视为「这个版本没有你的包」。

## 5. 安装与换包更新

与 macOS 版一致：CI 打的包（`BuildInfo::releaseTag` 非空）才自动下载；本机开发版只提示有新版本。数据目录与程序目录分开
（Windows `%APPDATA%\Friday`，Linux `$XDG_DATA_HOME/Friday`），换包不碰数据。换包脚本启动成功后，应用保存会话、结束 MCP
（`beforeRelaunch`）再退出；脚本没启动起来时什么都不停，程序照常可用。

### Windows（zip 便携版）

建议解压到用户可写的位置（例如 `%LOCALAPPDATA%\Programs\Friday` 或 `D:\Friday`），这样自动更新不需要管理员权限；
程序目录不可写时（比如把 zip 解压到 `C:\Program Files`）只提示「请手动下载」并给出 Release 链接——要装进 Program Files 用安装程序（§7）。

换包流程：

1. 下载 `assets["windows-x64"]` 到 `%LOCALAPPDATA%\Friday\Cache\updates\`，校验大小和 SHA-256；
2. 解压到 `…\updates\<标签>\`（得到 `…\updates\<标签>\Friday\`），检查里面有 `friday.exe` 和 `package.json`，
   且 `package.json` 的 `asset` 与自己一致；
3. 在 `updates\` 里写一个 PowerShell 脚本并**分离启动**（`QProcess::startDetached`）：
   `powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File update.ps1 -ProcessId <本进程 PID> -Source <…\Friday> -Target <程序目录> -Log <…\update.log>`。
   脚本用系统自带的 Windows PowerShell 5.1（不是 pwsh），所以**文件要写成带 BOM 的 UTF-8（或纯 ASCII）**，否则中文路径会乱码；
4. 应用保存会话、结束 MCP 后退出；
5. 脚本：`Wait-Process -Id <PID> -Timeout 60`；程序目录里还有别的进程在跑时等它们退出（最多 30 秒），其中还有 `friday.exe` /
   `friday-cli.exe`（另一个窗口、终端里的命令行版；后台服务 `friday server …` 不算，经 CIM 看命令行）就放弃这次更新（没有别的 Friday 窗口时启动旧版本）；
   把 `<程序目录>` 改名为 `<程序目录>.old`（失败就每秒重试，最多约 30 秒，文件还被占用时放弃并记日志）→ 把新 `Friday` 目录移动到
   `<程序目录>` → 启动新的 `friday.exe` → 删除 `.old`；任何一步失败都把 `.old` 改回原名并启动旧版本。
   跨盘时 `Move-Item` 会变成复制，所以更新缓存应和程序目录在同一个盘上；不在时改用 `robocopy /MIR <新> <程序目录>`。
   1.x 的包（带推理库与 `backend\`）升上来也是整个目录一起换，旧文件随 `.old` 删掉。

### Windows（安装程序装的，§7）

**v2.8 起「只为我」安装**（§7「装机范围」）：普通用户运行安装程序不要管理员，装进 `%LOCALAPPDATA%\Programs\Friday`、卸载信息写
`HKCU`——程序目录本来就是当前用户自己的，下面的原地换包直接可用，不用开权限，也永远不走安装程序（不弹 UAC）。

**v2.5 起（用户 2026-10-10 选的取舍），为所有用户装的**：安装程序装完给内置的 Users 组开程序目录的「修改」权限（可继承，`util::grantUsersModify`）、
开 `HKLM\…\Uninstall\Friday` 的读写值权限（`grantUsersRegistryWrite`）。程序目录写得进去（`CurrentBuild::appDirWritable`）时：

1. 和便携版一样下 zip（`packageFormat()` 返回 Primary）、解压到 `updates\<标签>\`、核对；`installLocation()` 是 `WindowsInPlace`；
2. 不要管理员确认，**空闲时自动装**（同便携版、Mac）；
3. 换包脚本 `update-inplace.ps1`（`UpdateManager::windowsInPlaceScript`）：等 Friday 退出、别的 friday 退出（同便携版），
   **不改程序目录的名字**（它在 Program Files 下，上级目录普通用户写不了），逐个文件换——已有的先改名成 `<名字>.friday-old-<随机>`
   （在跑的程序文件也能改名，比如后台服务 `friday server`），再放新的、核对 SHA-256；有一个放不进去就把已换的全部还原、启动旧版本；
   安装版才有的 `uninstall.exe` 留着，新版本里已经没有的旧程序（v2.6 起并进 friday-cli.exe 的 `friday-server.exe`、v2.8 起并进 friday.exe 的 `friday-cli.exe`）同样改名挪开；
   成功后改「应用和功能」里的 `DisplayVersion`（HKCU、HKLM 里 `InstallLocation` 是这个程序目录的那把，两边展开成长路径再比）、启动新版本、删掉能删的 `.friday-old-*`
   （还被占用的留到下次换包再删）。路径用 .NET 的 `Directory.EnumerateFiles` 列（Get-ChildItem 会把 8.3 短名展开成长名，按长度截相对路径会放错地方）。
   代价：以用户身份运行的程序也能改 Friday 的程序文件。v2.5 之前装的，下一次（点了、UAC 确认）用新安装程序装完才有这个权限。

程序目录写不进去（v2.5 之前的安装程序装的、开权限失败）时照旧，换包交给安装程序：

1. 下载 `assets["windows-x64"].installer` 到 `…\updates\`，校验大小、SHA-256 和包尾标记（`FRIDAYPK`，确认是 Friday 的
   安装程序）；Friday 本身带有效 Authenticode 签名时，安装程序也必须是同一签名者签的（目前都没签，不要求）；
2. 要管理员确认，所以**不在空闲时自动装**：下载好只在侧边栏底部提示「点击更新到 x.y」，用户点了才装（人不在电脑前时 Friday
   退了、UAC 确认框却没人点）；
3. 用户点了：`ShellExecuteEx("runas")` 以管理员身份启动 `setup.exe /S /R /W=<本进程 PID> /D=<程序目录>`（工作目录 `updates\`）。
   UAC 由 Friday 自己发起，确认框弹在最前面；用户点「否」时什么都没停，状态变成「更新失败：无法启动安装程序：没有得到管理员确认」；
4. 点了「是」：Friday 保存会话、断开 MCP 后退出；安装程序先等这个 PID 退出（最多 60 秒），再静默覆盖安装（从 1.x 升上来时先删掉
   旧的推理文件，§3；被占用的文件挪开重建、登记重启时删除），装完（成功或失败）经 explorer.exe 以普通用户身份启动 `friday.exe`。
   日志在 `%TEMP%\friday-setup.log`。

### Linux deb（§8）

程序在 `/opt/friday`，由 dpkg 管着：

1. 下载 `assets["linux-x64"].deb` 到 updates 目录，校验大小、SHA-256 和 ar 归档头（`!<arch>` + `debian-binary`）；
2. 同 Windows 安装版，要认证（polkit），只在用户点了时装；没有 `pkexec` 时下载前就报「请手动下载」；
3. 分离启动一段 sh 脚本：等本进程退出 → `pkexec /bin/sh -c 'apt-get install -y --allow-downgrades <deb> || dpkg -i <deb>'`
   （没有 apt-get 时只用 dpkg）→ 删掉 deb → 启动 `/usr/bin/friday`。用户取消认证或装失败时照样启动（还是旧版本）。

### Linux AppImage

`APPIMAGE` 环境变量是当前 AppImage 文件的绝对路径：

1. 把新 AppImage 下载到**同一目录**下的临时名（`.Friday-<标签>.AppImage`，下载中带 `.part`），校验大小和 SHA-256，`chmod 755`；
2. 分离启动一段 sh 脚本：等本进程退出（最多约 2 分钟）→ `mv -f` 覆盖 `$APPIMAGE`（同一文件系统上 rename 是原子的）→ 启动新文件。
   等旧进程退出再换、再启动，免得新旧两个进程同时读写会话文件。

所在目录不可写时（例如放在 `/opt` 且非 root）下载前就报「请手动下载」并给出 Release 链接。

### Linux tar.gz

安装根目录 = `<程序目录>/../..`（即 `Friday/`）：

1. 下载 `tarball`，校验后解压到安装根目录旁边的 `Friday.new-<标签>/`（同一文件系统）；
2. 写一个小 shell 脚本分离启动：等本进程退出（`while kill -0 <PID>; do sleep 0.5; done`）→ `mv Friday Friday.old-<标签>`
   → `mv Friday.new-<标签>/Friday Friday` → 启动 `Friday/AppRun` → 删除旧目录；失败时还原。
   不要在进程还活着时改名：Qt 插件、QML 文件都是按路径懒加载的，运行中挪走目录会让后续加载失败。

## 6. CI（Friday.build）

工作流 `Friday.build/.github/workflows/build.yml` 分四个 job：

- **prepare**：解析源码提交、算版本号 / 渠道 / 标签（逻辑与原来相同），检查源码里有没有 `qt/`、`qt/installer/`，并按源码定 Qt 版
  出几套包：v2.0 起每个平台一套（矩阵里记作 `none`，包名、`latest.json` 的键都不带后端）；源码还带本地推理（有
  `qt/ci/build_backends.sh`，v2.0 之前的分支）时照旧 CUDA、Vulkan 各一套；
- **macos**：原来的 Swift 构建、签名、公证，产物上传为 artifact；
- **qt**：矩阵由 prepare 算好（`qt_matrix`：运行器、平台、Qt 的 arch、后端）：`windows-2022`（windows-x64）、`ubuntu-22.04`（linux-x64）、
  `ubuntu-24.04-arm`（linux-arm64，Qt 用 `linux_gcc_arm64`，原生构建、跑单测、装 deb 核对，AppImage 工具换 aarch64 版；
  24.04 的部分库改名成 `*t64`）；
  老源码只有前两个，再乘 `{cuda, vulkan}`。`fail-fast: false`。工具链版本全部取自源码的
  `qt/ci/deps.env`（Qt 6.11.3 用 aqtinstall 的已验证提交安装，附加模块 qtmultimedia / qtspeech / qtimageformats，
  与本机开发用的 Qt 同一版本；AppImage 打包工具），另有 ccache。步骤：
  1. `build_app` 只构建；单测单独一步：`run_tests`（`friday_tests` 按类分进程，某个类崩了也看得出是哪个）；
  2. 打包：`package_windows.ps1 -InstallerDir <构建目录>/installer`（zip + 安装程序，§7）、`package_linux.sh`（AppImage + tar.gz + deb，§8）。
     源码里没有 `qt/installer/`（老分支）时不传 `-InstallerDir`，Windows 只出 zip、Linux 只出 AppImage 与 tar.gz；
  3. 产物上传之后核对：Windows 用安装程序静默装到临时目录（运行器上是管理员，没有 UAC 确认框）→ 核对文件、`friday.com --version`（老分支是 `friday-cli`）、
     「应用和功能」的注册信息 → 静默卸载，目录和注册信息都要没了；Linux 用 `apt-get install` 装那个 deb → `/usr/bin/friday --version`（老分支是 `friday-cli`）
     → `apt-get remove`。单测、核对失败不挡打包和发布，最后一步把这个 job 标成失败；
- **release**：macOS 成功才发布；Qt 某个平台失败的照样发布其余产物，并在 Release 说明里列出缺哪些。v2.0 起的源码先给 1.x 打
  `-upgrade-<后端>` 包（§4），再重新计算全部文件的 SHA256SUMS.txt，一次性上传，然后「每个渠道只保留最新 Release」
  「更新 latest.json（含 assets 与给 1.x 的老键）」。

升级 Qt / 打包工具只改 `qt/ci/deps.env`（升级 Qt 时本机开发的 Qt 也换成同一版本）。Linux 包只在 CI 上构建与核对（本机只验证 Windows）；
要在本地按 CI 的顺序跑一遍 Linux 流程用 `qt/ci/docker/run_ci.sh`（`build_app.sh` → `package_linux.sh` → `verify_linux_package.sh`，
见 `qt/ci/README.md`）。

## 7. Windows 安装程序（`qt/installer/`）

自制安装器，移植自 Secret-Chat（`desktop/desktop-qt/installer/`，那边用它替换了 DPI 不感知、高缩放下发糊的 NSIS）。产物
`Friday-<标签>-windows-x64-setup.exe` 与 zip 内容相同，多装两个文件：`uninstall.exe`（「应用和功能」的卸载入口）与
`FridaySetupUI.exe`（安装 / 卸载界面）。

| 部件 | 链接 | 作用 |
| --- | --- | --- |
| `FridaySetup.exe`（stub） | 不链 Qt，静态链 VC++ 运行库 | 安装程序本体；mkpkg 把 LZMA 负载追加在它后面 |
| `uninstall.exe` | 同上 | 卸载器；交互卸载时拉起 `FridaySetupUI.exe --uninstall`，`/S` 静默卸载 |
| `FridaySetupUI.exe` | Qt Quick（动态 Qt） | 一页式安装 / 卸载界面（只用 QtQuick，不用 Controls：引导子集越小，界面出来越快） |
| `mkpkg.exe` | 不链 Qt，静态链 | 构建机上的打包工具 |
| `installer_shortcut_test.exe` | 不链 Qt，静态链 | 快捷方式的回归门禁，打包前必跑 |

**静态链 VC++ 运行库**（`installer/CMakeLists.txt`）：动态链的话 stub / 卸载器依赖 `vcruntime140*.dll`、`msvcp140.dll`，而它们
恰恰也在安装目录里——覆盖安装时安装程序自己占着要覆盖的那份、干净机器上双击直接 0xC0000135、卸载的收尾进程（复制到 `%TEMP%`
运行）起不来。`package_windows.ps1` 用 dumpbin 核对这三个 exe 只导入系统 DLL。

**包格式**（`installer/core/Pkg.h`）：`[stub][5 字节 LZMA props + 一条 solid LZMA 流][索引][112 字节包尾，magic FRIDAYPK]`。
「界面引导子集」（`FridaySetupUI.exe` + 对它跑 windeployqt 的产出 + VC++ 运行库，约 43 MB）排在流首：双击后 stub 先画原生启动图，
只解这段前缀到 `%TEMP%`（约 1 秒）就起 Qt 界面；与应用共用的 Qt DLL 只存一份。内容相同的文件只进流一次，其余记 `dupOf`，
解包时从已写好的那份复制（1.x 过渡期的包在 `backend\` 里另放一份 CUDA / VC++ 运行库，靠它省下约 500 MB）。压缩是流式的
（不把整个负载读进内存）。打完用安装时同一份解包代码解开、逐字节对照来源（`mkpkg --verify-dir`），不一致就删掉产物、打包失败。

**装机范围**（v2.8，`install::Scope`）：安装程序的清单是 `highestAvailable`——

| | 所有用户（Machine） | 只为我（User） |
|---|---|---|
| 谁 | 管理员双击（弹 UAC 点「是」） | 普通用户双击（不弹框） |
| 默认目录 | `%ProgramFiles%\Friday` | `%LOCALAPPDATA%\Programs\Friday` |
| 卸载信息 | `HKLM\…\Uninstall\Friday` | `HKCU\…\Uninstall\Friday`（同样列在「应用和功能」里） |
| 快捷方式 | 公用桌面 / 公用开始菜单（建不进去退到当前用户的） | 当前用户的桌面 / 开始菜单 |
| 自动更新 | 给 Users 开了写权限就原地换（§5），否则安装程序 + UAC | 原地换，不要管理员 |

范围怎么定（`detectScope`）：`/D=` 正是 HKCU / HKLM 里记的那份 → 那个范围；当前用户已经只为自己装过 → User；这台电脑已经为所有用户
装过 → Machine（升级不换地方，不在两处各装一份）；都没装过 → 管理员 Machine、普通用户 User。`/ALLUSERS`、`/CURRENTUSER` 可以指定。
要为所有用户装、进程又不是管理员（普通用户升级已为所有用户装的那份）：安装程序以管理员身份重新起自己（`runas`，带 `/ELEVATED /ALLUSERS`
防止来回起），UAC 里点了「否」就提示请管理员先卸掉那份、再只为自己装。`uninstall.exe` 的清单是 `asInvoker`：只为我装的直接卸，
为所有用户装的以管理员身份重新起自己。

**安装行为**（`installer/core/Install.cpp`）：默认目录见上表（升级沿用注册表里记的位置）；写卸载信息（`UninstallString`、`QuietUninstallString`、
`DisplayVersion`…，写完读回校验）。解包前先删掉旧版本留下、新包里已经没有的文件（1.x 的本地推理文件、v2.6 之前的 friday-server.exe、v2.8 之前的 friday-cli.exe；`removeObsoleteFiles`，§3）。覆盖安装时被占用的文件
挪成 `.old-<pid>-<n>` 隐藏文件、登记重启时删除。用户数据不在安装目录（`%APPDATA%\Friday`、`%LOCALAPPDATA%\Friday`），
卸载不碰会话和设置。日志 `%TEMP%\friday-setup.log`。

**命令行**：`FridaySetup.exe /S [/R] [/W=<pid>] [/ALLUSERS | /CURRENTUSER] [/D=<目录>]`——`/S` 静默（不碰 Qt、不弹窗）；`/D=` 沿用 NSIS 语义（最后一个参数、
不带引号、含空格也不带）；`/W=<pid>` 先等这个进程退出（最多 60 秒）；`/R` 装完（成功或失败）以普通用户身份启动 Friday。
自动更新用的就是 `/S /R /W=<Friday 的 PID> /D=<程序目录>`（§5）。`uninstall.exe /S` 静默卸载。

**打包时的门禁**（`package_windows.ps1`）：三个不链 Qt 的 exe 只导入系统 DLL；`installer_shortcut_test` 在没有 COM 的工作线程上
真建一次 `.lnk`（交互安装就在这样的线程上跑，Secret-Chat 踩过「装完桌面上什么都没有」）；`FridaySetupUI.exe --selftest` 在只有
引导目录的环境里（去掉 Qt 的 PATH / QT_PLUGIN_PATH / QML 路径）真把 QML 加载一次；卸载界面要的 DLL 在程序目录里都有；mkpkg 解回原样。
`FRIDAY_SETUP_THEME=light|dark` 可以强制安装界面与启动图的配色。

## 8. Linux deb

`Friday-<标签>-linux-{x64,arm64}.deb`（`package_linux.sh`，需要 `dpkg-deb`）：和 AppImage / tar.gz 同一个 AppDir，原样装到
`/opt/friday`（自带 Qt，不依赖发行版的 Qt）；`/usr/bin/friday`（经 `/opt/friday/AppRun` 启动；带命令参数时是命令行版，v2.8 起没有单独的 `/usr/bin/friday-cli`）是
启动脚本；桌面文件与图标装进 `/usr/share/applications`、`/usr/share/icons/hicolor`（dpkg 的触发器刷新桌面数据库和图标缓存）。

- 包名 `friday`（1.x 的 CUDA 版、Vulkan 版也叫这个名字，装上 v2.0 的 deb 就是升级，§3）；版本 `v1.2.3` → `1.2.3`、`v1.2.3-beta` → `1.2.3~beta`
  （`~` 排在正式版前面）；架构 `amd64` / `arm64`；`-Zxz` 压缩（Debian 11 / Ubuntu 20.04 的 dpkg 还不认 zstd）。
- **Depends 不手写**：对包里每个 ELF（程序、Qt 库与插件、QML 插件）跑 ldd，取解析到包外的系统库，用
  `dpkg -S` 查所属的包（ldd 报的路径查不到时再试 `/lib` ↔ `/usr/lib` 和真实路径）；查不到的（显卡驱动、手装的库）跳过并警告。
  `libc6` 带构建机 glibc 的版本下限（ubuntu-22.04 是 2.35），其余不写版本（24.04 的 t64 改名包都 Provides 了旧名）。
  Recommends `speech-dispatcher`（朗读）。
- 安装：`sudo apt install ./Friday-…deb`（apt 会补上缺的依赖）；卸载 `sudo apt remove friday`，会话与设置在 `~/.local/share/Friday`，不受影响。
- 自动更新见 §5「Linux deb」。
