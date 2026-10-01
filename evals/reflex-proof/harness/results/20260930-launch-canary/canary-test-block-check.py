"""Offline check: the supplied Finite tests pass a gold implementation and fail broken ones."""
import math, sys
sys.path.insert(0, '/Users/stevenbooth/Documents/skill-compile/.claude/worktrees/agent-ac9ae5e561a996d9d/evals/reflex-proof/harness')
import pytest
from marshmallow import ValidationError, validate


class Finite(validate.Validator):
    default_message = "Must satisfy Finite."

    def __init__(self, *, error=None):
        self.error = error or self.default_message

    def _repr_args(self):
        return ""

    def _format_error(self, value):
        return self.error.format(input=value)

    def __call__(self, value):
        if not math.isfinite(value):
            raise ValidationError(self._format_error(value))
        return value


class RejectsBooleans(Finite):
    def __call__(self, value):
        if isinstance(value, bool):
            raise ValidationError(self._format_error(value))
        return super().__call__(value)


class AcceptsInf(Finite):
    def __call__(self, value):
        if value != value:
            raise ValidationError(self._format_error(value))
        return value


class WrongMessage(Finite):
    default_message = "Not finite."


def run(cls):
    validate.Finite = cls
    import launch_canary  # noqa: imports harness modules only
    ns = dict(pytest=pytest, ValidationError=ValidationError, validate=validate)
    exec(launch_canary.FINITE_TESTS + launch_canary.BOOL_TEST, ns)
    failures = []
    for name, fn in ns.items():
        if name.startswith("test_"):
            try:
                fn()
            except BaseException as e:
                failures.append((name, type(e).__name__))
    return failures


# The base block passes with or without boolean rejection; only the later-edit test needs it.
for cls in (RejectsBooleans, Finite, AcceptsInf, WrongMessage):
    print(cls.__name__, run(cls))
