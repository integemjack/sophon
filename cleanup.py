# -*- coding: utf-8 -*-
import shutil, os
for f in ["e2e_run.py", "e2e_ask_reg.py", "fix_e2e_run.py", "fix_verify.py", "fix_rest.py",
          "fix_impl.py", "fix_kw.py", "fix_let.py", "fix_all.py", "fix_all2.py", "fix_all3.py",
          "fix_all4.py", "fix_all5.py", "fix_norm2.py", "fix4.py", "fix4b.py", "norm_rust.py",
          "reg_loop.py", "r5.py", "r6.py", "probe_api.py", "probe_tool.py", "probe_err.py",
          "probe_tail.py", "probe_marks.py", "probe_marks2.py", "probe_v.py", "probe_vi.py",
          "probe_errs.py", "probe_step.py", "probe_pc.py", "probe_bool.py", "probe_kw.py",
          "mock_api.py", "e2e_ask.py", "fix_e2e.py", "fix_e2e2.py", "update_readme.py",
          "mock_log.txt", "check_err.txt", "ship.py"]:
    if os.path.exists(f):
        os.remove(f)
for d in ["e2e_run", "e2e_tmp"]:
    shutil.rmtree(d, ignore_errors=True)
print("cleaned")
