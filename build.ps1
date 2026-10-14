# Hermes 构建脚本（Windows）
#
# 说明：Windows GNU 工具链编译 parking_lot_core 需要 MinGW binutils
# （dlltool / as / ld）。Rust 自带的 self-contained binutils 缺 as.exe，
# 因此把 MSYS2 的 mingw64\bin 加进 PATH 后再编译。
# 若你装的是完整 MinGW-w64（如 Qt 自带 mingw1310_64），把 $mingw 改成那个路径即可。

$mingw = "C:\msys64\mingw64\bin"
if (Test-Path $mingw) {
    $env:PATH = "$mingw;$env:PATH"
    Write-Host "已加入 MinGW binutils: $mingw" -ForegroundColor DarkGreen
} else {
    Write-Warning "未找到 $mingw`n请安装 MSYS2（https://www.msys2.org/）后重试，或把任意 MinGW-w64 的 bin 目录加进 PATH。"
}

$mode = if ($args -contains "--release") { "--release" } else { "" }
cargo build $mode
