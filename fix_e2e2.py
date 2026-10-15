# -*- coding: utf-8 -*-
from pathlib import Path
p = Path(r"C:\Users\ultra\Desktop\works\hermes\e2e_ask.py")
t = p.read_text(encoding="utf-8")
# 1. drop dead accept-lambda thread line
lines = [l for l in t.splitlines()
         if "callable_iterator" not in l and "iter(srv.accept, None)[0:1]" not in l]
t = "\n".join(lines) + "\n"
# 2. assert the seed mock line survives instead of the old comment line
t = t.replace('if "# 测试清单" not in content:', 'if ("http://127.0.0.1:%d/v1 mock-key\\n" % PORT) not in content:')
p.write_text(t, encoding="utf-8", newline="\n")
print("e2e fixed")
