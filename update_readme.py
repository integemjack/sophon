# -*- coding: utf-8 -*-
from pathlib import Path
p = Path(r"C:\Users\ultra\Desktop\works\hermes\dist\sophon-0.1.0-win64\README.txt")
t = p.read_text(encoding="utf-8")
old = "单文件程序，无外部依赖（仅用 Windows 系统自带 DLL）。\n联网抓取用本机 Edge/Chrome 的 headless 模式渲染。"
new = (
"  sophon ask      <prompt>   与远程模型对话：回复实时逐段打印；回答中的\n"
"                             <SOPHON_APPEND>地址 密钥 [模型名]</SOPHON_APPEND> 标记\n"
"                             会被验证（拉取 /models）后自动追加到 sophon.sub 末尾\n"
"  sophon tools               列出已安装的脚本工具（桌面 sophon 目录 tools\\）\n"
"\n"
"配置（桌面 sophon 目录，可用环境变量 SOPHON_DIR 换根）：\n"
"  sophon.sub     远程 API 清单，每行：<api地址> <api_key> [模型名]；从最后一行往上试\n"
"  sophon.prompt  系统提示词；不存在则自动写入内置提示词\n"
"\n"
"单文件程序，无外部依赖（仅用 Windows 系统自带 DLL）。\n"
"联网抓取用本机 Edge/Chrome 的 headless 模式渲染。"
)
assert t.count(old) == 1, "anchor not found"
p.write_text(t.replace(old, new), encoding="utf-8", newline="\r\n")
print("README updated")
