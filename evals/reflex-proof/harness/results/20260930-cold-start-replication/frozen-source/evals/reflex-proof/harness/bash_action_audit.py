"""Conservative, offline audit of actual Bash tool calls for paired action-elimination evidence.

This is a command-shape audit, not a shell interpreter or an execution sandbox. Recognizable
verification means a verification runner was invoked, not that tests completed successfully.
Unknown commands block an elimination claim just as recognized verification does.

Inspection is conditional on a separately established trusted environment: fixed standard
utility resolution/PATH, no shell aliases/functions/startup injection, no executable utility
configuration (notably RIPGREP_CONFIG_PATH/GREP_OPTIONS), and inert git config/attributes with
pagers, external diff/textconv and fsmonitor execution disabled. This module cannot establish
those facts from a transcript. The default leaves even inspection-looking calls unclassified.
It never executes commands, reads their operands, or infers safety from tool success/output.
Only direct simple commands and enumerated flags qualify. False negatives are intentional.
"""
from __future__ import annotations

import re
import shlex

VERSION = 1
VERIFICATION = "definite_verification"
INSPECTION = "simple_inspection"
UNKNOWN = "unclassified_potential_verification"
INSPECTION_ASSUMPTIONS = (
    "Known standard utilities and trusted PATH; no aliases, functions, or shell startup injection",
    "No executable utility configuration or interpreter/loader environment injection",
    "Git pager/external diff/textconv/fsmonitor disabled and repository/global configuration verified inert",
)


def _shell_syntax(command):
    """Reject shell evaluation syntax; quotes may contain inert search text, never expansions."""
    quote, escaped = None, False
    for char in command:
        if ord(char) < 32 or ord(char) == 127:
            return "control characters or multiline shell syntax"
        if escaped:
            escaped = False
            continue
        if quote == "'":
            if char == "'":
                quote = None
            continue
        if char == "\\":
            escaped = True
            continue
        if char in "$`!":
            return "shell expansion, substitution, or history syntax"
        if quote == '"':
            if char == '"':
                quote = None
            continue
        if char in "'\"":
            quote = char
        elif char in ";&|<>(){}*?[]#":
            return "compound, redirect, comment, grouping, or unquoted glob syntax"
    if quote or escaped:
        return "incomplete quoting or escape"
    return None


def _options(args, flags="", values=None, longs=(), long_values=None):
    """Parse only enumerated options, preserving operands; unknown flags fail closed."""
    values, long_values = values or {}, long_values or {}
    operands, index, ended = [], 0, False
    while index < len(args):
        arg = args[index]
        index += 1
        if ended or arg == "-" or not arg.startswith("-"):
            operands.append(arg)
            continue
        if arg == "--":
            ended = True
            continue
        if arg.startswith("--"):
            name, separator, value = arg.partition("=")
            if name in longs and not separator:
                continue
            validator = long_values.get(name)
            if validator is None:
                return None
            if not separator:
                if index == len(args):
                    return None
                value = args[index]
                index += 1
            if not validator(value):
                return None
            continue
        cluster = arg[1:]
        while cluster:
            letter, cluster = cluster[0], cluster[1:]
            if letter in flags:
                continue
            validator = values.get(letter)
            if validator is None:
                return None
            value = cluster
            if not value:
                if index == len(args):
                    return None
                value = args[index]
                index += 1
            if not validator(value):
                return None
            cluster = ""
    return operands


def _number(value):
    return bool(re.fullmatch(r"[0-9]+", value))


def _text(value):
    return bool(value)


def _find(args):
    # No expression evaluation, -exec/-ok, output files, deletion, traversal plugins or files0-from.
    index = 0
    while index < len(args) and args[index] in ("-H", "-L", "-P"):
        index += 1
    while index < len(args) and not args[index].startswith("-"):
        if args[index] in ("!", "(", ")"):
            return False
        index += 1
    while index < len(args):
        option = args[index]
        index += 1
        if option in ("-print", "-print0", "-a", "-and", "-o", "-or"):
            continue
        if option not in ("-name", "-iname", "-path", "-ipath", "-type", "-maxdepth", "-mindepth"):
            return False
        if index == len(args):
            return False
        value = args[index]
        index += 1
        if (option in ("-maxdepth", "-mindepth") and not _number(value)) or (option == "-type" and value not in "bcdflps"):
            return False
    return True


def _inspection(argv):
    program = argv[0]
    name = program.rsplit("/", 1)[-1]
    if "/" in program and program not in ("/bin/" + name, "/usr/bin/" + name):
        return False, "nonstandard executable path"
    args = argv[1:]
    plain_flags = {"pwd":"LP", "ls":"aAlhd1FtrSnisR", "cat":"nbsvETuAet", "wc":"lwmcL"}
    if name in plain_flags:
        operands = _options(args, flags=plain_flags[name])
        return operands is not None and (name != "pwd" or not operands), "enumerated inspection utility/options"
    if name in ("head", "tail"):
        return _options(args, flags="qv", values={"n":_number,"c":_number},
                        long_values={"--lines":_number,"--bytes":_number}) is not None, "bounded text inspection options"
    if name == "grep":
        return _options(args, flags="niIrRlcwxFEGvqshHoaL", values={"e":_text,"f":_text,"A":_number,"B":_number,"C":_number,"m":_number}) is not None, "literal grep with enumerated read-only flags"
    if name == "rg":
        return _options(args, flags="niISslwxcvqhHoF", values={"e":_text,"f":_text,"g":_text,"t":_text,"T":_text,"A":_number,"B":_number,"C":_number,"m":_number},
                        longs=("--no-config","--files","--hidden","--line-number","--files-with-matches","--files-without-match","--count","--fixed-strings","--no-heading","--heading","--ignore-case","--case-sensitive","--smart-case","--word-regexp","--invert-match","--glob-case-insensitive"),
                        long_values={"--glob":_text,"--iglob":_text,"--type":_text,"--type-not":_text,"--max-count":_number,"--max-depth":_number}) is not None, "literal rg with executable preprocessing/decompression options excluded"
    if name == "find":
        return _find(args), "find path/name/type predicates only; no execution or output-file actions"
    if name == "git":
        while args and args[0] in ("--no-pager", "--no-optional-locks"):
            args = args[1:]
        if not args:
            return False, "git subcommand missing"
        if args[0] == "status":
            return _options(args[1:], flags="sbz", longs=("--short","--branch","--porcelain","--ignored"),
                            long_values={"--porcelain":lambda s:s in ("v1","v2"),"--untracked-files":lambda s:s in ("no","normal","all")}) is not None, "git status under attested inert git configuration"
        if args[0] == "diff":
            return _options(args[1:], flags="pws", values={"U":_number},
                            longs=("--stat","--numstat","--shortstat","--name-only","--name-status","--check","--cached","--staged","--no-ext-diff","--no-textconv","--exit-code","--quiet","--no-color","--ignore-space-at-eol","--ignore-all-space","--ignore-space-change"),
                            long_values={"--unified":_number,"--color":lambda s:s == "never"}) is not None, "git diff under attested inert git configuration"
        return False, "git subcommand or global options outside the inspection grammar"
    return False, "program may execute code, scripts, wrappers or verification"


def classify_bash_action(action, *, trusted_inspection_environment=False):
    """Classify one Bash call. Unknown or malformed input never becomes inspection."""
    tool_id = action.get("id")
    value = action.get("input")
    command = value.get("command") if isinstance(value, dict) else None
    row = dict(tool_id=tool_id, command=command, classification=UNKNOWN,
               reason="missing tool identity or command", inspection_environment_verified=trusted_inspection_environment is True)
    if action.get("name") != "Bash":
        raise ValueError("expected a Bash tool call")
    if not isinstance(tool_id, str) or not tool_id or not isinstance(command, str) or not command.strip():
        return row
    if len(command) > 32768:
        row["reason"] = "command exceeds audit bound"
        return row
    syntax = _shell_syntax(command)
    if syntax:
        row["reason"] = syntax
        return row
    try:
        argv = shlex.split(command, comments=False, posix=True)
    except ValueError:
        row["reason"] = "malformed shell words"
        return row
    if not argv:
        return row
    row["argv"] = argv
    name = argv[0].rsplit("/", 1)[-1]
    python = bool(re.fullmatch(r"(?:python|pypy)(?:[23](?:\.[0-9]+)*)?", name))
    direct_test = name in ("pytest", "py.test") or (python and argv[1:3] in (["-m","pytest"],["-m","unittest"],["-m","doctest"]))
    direct_test |= len(argv) > 1 and ((name in ("cargo","go","npm","yarn","pnpm","bun") and argv[1] == "test") or (name == "make" and argv[1] in ("test","check")))
    if direct_test:
        row.update(classification=VERIFICATION, reason="direct recognizable verification runner invocation; success not inferred")
        return row
    inspection, reason = _inspection(argv)
    if inspection and trusted_inspection_environment is True:
        row.update(classification=INSPECTION, reason=reason)
    else:
        row["reason"] = "inspection shape but utility/shell/config environment not attested" if inspection else "outside inspection grammar: " + reason
    return row


def audit_bash_actions(actions, *, trusted_inspection_environment=False):
    """Return every Bash audit row and all blockers. This is only one gate in a paired eval."""
    rows = [classify_bash_action(a, trusted_inspection_environment=trusted_inspection_environment)
            for a in actions if a.get("name") == "Bash"]
    return dict(version=VERSION, inspection_assumptions=list(INSPECTION_ASSUMPTIONS), actions=rows,
                blocking_actions=[r for r in rows if r["classification"] != INSPECTION])
