"""Stacks: one class per language/toolchain. Adding one (rust-cargo, go) means a new module here with
a Stack subclass (see stacks/base.py for the contract), `<lang>_good/_bad/_test` variants in
bugs.py, and an entry in STACKS."""

from stacks.node_npm import NodeNpm
from stacks.python_make import PythonMake

STACKS = {c.NAME: c for c in (PythonMake, NodeNpm)}
