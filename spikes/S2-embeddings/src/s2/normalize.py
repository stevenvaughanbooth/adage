"""Prompt normaliser per Design Spec §8.2.1: typed placeholders for spans,
fence/log stripping with structured flags, and a head-truncation to ~128
whitespace tokens (a cheap proxy for the real tokenizer's 128-token budget --
good enough since we only need to bound working-set size before the model's
own tokenizer does the real subword truncation).
"""
from __future__ import annotations

import re
from dataclasses import dataclass

FENCE_RE = re.compile(r"```.*?```", re.DOTALL)
URL_RE = re.compile(r"https?://\S+")
TICKET_RE = re.compile(r"\b[A-Z]{2,6}-\d{1,6}\b")
PR_RE = re.compile(r"#\d{1,6}\b")
SHA_RE = re.compile(r"\b[0-9a-f]{7,40}\b")
VERSION_RE = re.compile(r"\b\d+\.\d+(?:\.\d+)?\b")
ENV_VAR_RE = re.compile(r"\b[A-Z][A-Z0-9]*(?:_[A-Z0-9]+)+\b")
PATHISH_RE = re.compile(r"\b[\w][\w\-./]*/[\w\-./]+\b")

DIFF_MARKERS = ("```diff", "\n+++", "\n---", "\n+ ", "\n- ")
STACK_MARKERS = ("Traceback", "traceback", "panic:", "Exception", "Error:", "error:")

HEAD_TOKENS = 128


@dataclass
class Normalized:
    text: str
    has_fence: bool
    has_diff: bool
    has_stacktrace: bool
    n_placeholders: int


def normalize(raw: str, *, head_tokens: int = HEAD_TOKENS) -> Normalized:
    has_diff = any(m in raw for m in DIFF_MARKERS)
    has_stacktrace = any(m in raw for m in STACK_MARKERS)
    has_fence = "```" in raw

    # Strip fenced code / pasted logs entirely; their presence is recorded
    # as flags above, not embedded as text.
    text = FENCE_RE.sub(" [code] ", raw)

    n_placeholders = 0

    def sub(pattern: re.Pattern, token: str, s: str) -> str:
        nonlocal n_placeholders
        s2, count = pattern.subn(token, s)
        n_placeholders += count
        return s2

    # Order matters: URLs and tickets before the generic path-ish pattern,
    # since URLs and "W-4821" both contain characters the path regex would
    # otherwise grab.
    text = sub(URL_RE, "{url}", text)
    text = sub(TICKET_RE, "{ticket}", text)
    text = sub(PR_RE, "{pr}", text)
    text = sub(PATHISH_RE, "{path}", text)
    text = sub(SHA_RE, "{sha}", text)
    text = sub(VERSION_RE, "{version}", text)
    text = sub(ENV_VAR_RE, "{env_var}", text)

    tokens = text.split()
    if len(tokens) > head_tokens:
        tokens = tokens[:head_tokens]
    text = " ".join(tokens).strip()

    return Normalized(
        text=text,
        has_fence=has_fence,
        has_diff=has_diff,
        has_stacktrace=has_stacktrace,
        n_placeholders=n_placeholders,
    )


def raw_head(raw: str, *, head_tokens: int = HEAD_TOKENS) -> str:
    """The 'without normalisation' condition: no placeholders, no fence
    stripping -- just the raw text (still head-truncated, since every
    condition needs to respect the token budget or comparisons aren't fair).
    """
    tokens = raw.split()
    if len(tokens) > head_tokens:
        tokens = tokens[:head_tokens]
    return " ".join(tokens).strip()
