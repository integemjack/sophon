# -*- coding: utf-8 -*-
from pathlib import Path
import re
for ln, l in enumerate(Path("src/agent_loop.rs").read_text(encoding="utf-8").splitlines(), 1):
    if re.search(r"io::|anyhow|kill_tree|kill_tree|resolve_path|resolve_path|[Ss]andbox\.", l):
        print(ln, l)
