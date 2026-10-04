"""Bounded execution for these JSON-only fixture functions; not a general Python sandbox.

Only a single solve(value) definition, pure Python statements, selected methods and
builtins are accepted. Candidate imports, I/O, reflection, dynamic calls and powers
are unavailable. A fresh subprocess additionally enforces memory, CPU and wall limits.
"""
import ast
import json
import os
from pathlib import Path
import subprocess
import sys

MAX_SOURCE = 32 * 1024
MAX_INPUT = 128 * 1024
METHODS = frozenset("strip lstrip rstrip lower upper split join replace append pop get items keys values sort copy extend count index endswith startswith isdigit isalnum isspace".split())
BUILTINS = {name: __builtins__[name] if isinstance(__builtins__, dict) else getattr(__builtins__, name)
            for name in "abs all any bool dict enumerate float int len list max min range reversed round set sorted str sum tuple zip".split()}
ALLOWED = tuple(getattr(ast, name) for name in (
    "Module FunctionDef arguments arg Return Expr Assign AugAssign For While If Break Continue Pass "
    "Name Load Store Constant List Tuple Dict Set ListComp DictComp SetComp GeneratorExp comprehension "
    "BinOp BoolOp Compare UnaryOp Subscript Slice Attribute Call keyword IfExp JoinedStr FormattedValue "
    "Add Sub Mult Div FloorDiv Mod And Or Not UAdd USub Eq NotEq Lt LtE Gt GtE In NotIn Is IsNot"
).split())


def validate(source):
    if not isinstance(source, str) or len(source.encode()) > MAX_SOURCE:
        raise ValueError("source exceeds fixture limit")
    tree = ast.parse(source)
    if len(tree.body) != 1 or not isinstance(tree.body[0], ast.FunctionDef):
        raise ValueError("implement only def solve(value)")
    fn = tree.body[0]
    if (fn.name != "solve" or [arg.arg for arg in fn.args.args] != ["value"]
            or fn.args.defaults or fn.args.posonlyargs or fn.args.kwonlyargs or fn.args.vararg or fn.args.kwarg):
        raise ValueError("expected def solve(value)")
    nodes = list(ast.walk(tree))
    if len(nodes) > 1200 or any(not isinstance(node, ALLOWED) for node in nodes):
        raise ValueError("source uses unsupported fixture syntax")
    function_names = {node.name for node in nodes if isinstance(node, ast.FunctionDef)}
    for node in nodes:
        if isinstance(node, (ast.FunctionDef, ast.arg)):
            name = node.name if isinstance(node, ast.FunctionDef) else node.arg
            if name.startswith("_") or (isinstance(node, ast.FunctionDef) and (node.decorator_list or node.returns)):
                raise ValueError("decorators, annotations and private names are unavailable")
            if isinstance(node, ast.arg) and node.annotation:
                raise ValueError("annotations are unavailable")
        if isinstance(node, ast.Name) and node.id.startswith("_"):
            raise ValueError("private names are unavailable")
        if isinstance(node, ast.Attribute) and node.attr not in METHODS:
            raise ValueError("method is unavailable in this JSON-only fixture")
        if isinstance(node, ast.Call):
            if isinstance(node.func, ast.Name):
                if node.func.id not in BUILTINS and node.func.id not in function_names:
                    raise ValueError("dynamic function calls are unavailable")
            elif not isinstance(node.func, ast.Attribute):
                raise ValueError("dynamic function calls are unavailable")
    return tree


def evaluate(source, inputs):
    """Return one output per input or an explicit failure; no candidate code in this process."""
    validate(source)
    payload = json.dumps({"source": source, "inputs": inputs}, allow_nan=False).encode()
    if len(payload) > MAX_INPUT:
        raise ValueError("inputs exceed fixture limit")
    env = {"PATH": os.defpath, "LANG": "C.UTF-8"}
    process = subprocess.run([sys.executable, "-I", "-S", str(Path(__file__).resolve()), "--worker"],
                             input=payload, capture_output=True, env=env, timeout=4)
    if process.returncode:
        raise ValueError("candidate evaluation failed or exceeded a resource limit")
    if len(process.stdout) > MAX_INPUT:
        raise ValueError("candidate outputs exceed fixture limit")
    result = json.loads(process.stdout)
    if not isinstance(result, list) or len(result) != len(inputs):
        raise ValueError("invalid candidate results")
    return result


def worker():
    import resource
    resource.setrlimit(resource.RLIMIT_CPU, (1, 1))
    resource.setrlimit(resource.RLIMIT_AS, (128 * 1024 * 1024, 128 * 1024 * 1024))
    resource.setrlimit(resource.RLIMIT_FSIZE, (0, 0))
    payload = json.loads(sys.stdin.buffer.read(MAX_INPUT + 1))
    tree = validate(payload["source"])
    namespace = {"__builtins__": BUILTINS}
    exec(compile(tree, "<fixture-solution>", "exec"), namespace)
    outputs = [namespace["solve"](value) for value in payload["inputs"]]
    body = json.dumps(outputs, allow_nan=False)
    if len(body.encode()) > MAX_INPUT:
        raise ValueError("outputs exceed fixture limit")
    sys.stdout.write(body)


if __name__ == "__main__" and sys.argv[1:] == ["--worker"]:
    worker()
