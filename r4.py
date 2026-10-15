# -*- coding: utf-8 -*-
# r4b: make stream_tests self-contained
from pathlib import Path
p = Path(r"C:\Users\ultra\Desktop\works\hermes\src\remote_api.rs")
t = p.read_text(encoding="utf-8")

# keep normal use line (undo nothing yet - r4b not applied before)
old_use = "    use std::io::{Read, Write};\n    use std::net::SocketAddr;\n    use std::time::Duration;"
new_use = "    use std::io::{Read, Write};\n    use std::net::SocketAddr;\n    use std::time::Duration;"

# 1. spawn_mock -> self-contained spawn_mock_ct calls (rust-escaped bodies)
a1 = 'spawn_mock("200 OK", "{\\"data\\":[{\\"id\\":\\"m-1\\"}]}")'
b1 = 'spawn_mock_ct("application/json", "{\\"data\\":[{\\"id\\":\\"m-1\\"}]}")'
a2 = 'spawn_mock("404 Not Found", "no")'
b2 = 'spawn_mock_ct("application/json", "no")'
c1, c2 = t.count(a1), t.count(a2)
t = t.replace(a1, b1).replace(a2, b2)

# 2. ensure stream_tests has `use super::*;` (it does from r3) - nothing to change
p.write_text(t, encoding="utf-8", newline="\n")
print("mock calls replaced:", c1, c2)
