#!/usr/bin/env python3
"""Self-check: every work item fails untouched and passes with its reference solution; the dead end
and the finish-line obligation both trigger. Delegates to solutions.verify()."""
import subprocess
import sys
from pathlib import Path

sys.exit(subprocess.call([sys.executable, str(Path(__file__).with_name("solutions.py")), "verify"]))
