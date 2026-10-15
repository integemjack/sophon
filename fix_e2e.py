# -*- coding: utf-8 -*-
from pathlib import Path
p = Path(r"C:\Users\ultra\Desktop\works\hermes\e2e_ask.py")
t = p.read_text(encoding="utf-8")
old = '    f.write("# 测试清单\\n")'
new = '    f.write("http://127.0.0.1:%d/v1 mock-key\\n" % PORT)'
assert t.count(old) == 1, "anchor not found"
p.write_text(t.replace(old, new), encoding="utf-8", newline="\n")
print("fixed")
