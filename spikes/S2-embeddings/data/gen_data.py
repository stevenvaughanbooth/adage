#!/usr/bin/env python3
"""Generate the S2 labelled benchmark set.

Builds synthetic-but-realistic coding-agent prompts across >=12 intent
families, a verb-contrast "hard" subset that shares nouns across families,
and a pool of noise prompts that belong to no family. Nothing here is real
session data (per the spike rules) -- everything is templated + seeded
random slot-filling so the set is reproducible and S7 can regenerate it
identically.

Usage:
    python data/gen_data.py --out data/prompts.jsonl --seed 20260922
"""
from __future__ import annotations

import argparse
import json
import random
from dataclasses import dataclass, field

# --------------------------------------------------------------------------
# Slot pools -- realistic-looking IDs, paths, shas, urls, etc.
# --------------------------------------------------------------------------

TICKET_IDS = [
    "W-4821", "ENG-1203", "JIRA-77", "PROJ-990", "BUG-215", "W-3310",
    "ENG-458", "SUP-102", "OPS-61", "W-7742", "ENG-2291", "BUG-88",
]
PR_NUMBERS = [482, 1391, 217, 903, 55, 1204, 76, 340, 1588, 21]
PATHS = [
    "src/auth/token.py", "packages/api/src/routes/users.ts",
    "internal/billing/invoice.go", "lib/payments/stripe_client.rb",
    "app/models/order.py", "services/checkout/handler.py",
    "web/src/components/Cart.tsx", "cmd/worker/main.go",
    "src/db/migrations/0042_add_index.sql", "pkg/auth/middleware.go",
]
BRANCHES = [
    "fix/auth-timeout", "feature/rate-limiter", "chore/bump-deps",
    "fix/checkout-race", "feature/dark-mode", "refactor/payment-client",
    "hotfix/nil-pointer", "feature/export-csv",
]
PACKAGES = [
    ("react", "18.3.1"), ("lodash", "4.17.21"), ("requests", "2.31.0"),
    ("express", "4.19.2"), ("rails", "7.1.3"), ("django", "5.0.4"),
    ("numpy", "2.0.2"), ("openssl", "3.2.1"),
]
SERVICES = [
    "checkout-service", "auth-service", "billing-service",
    "notifications-service", "search-service", "inventory-service",
]
FUNCTIONS = [
    "calculate_totals", "validate_token", "handle_webhook",
    "parse_config", "retry_with_backoff", "normalize_address",
]
TEST_NAMES = [
    "test_auth_flow", "test_checkout_totals", "test_webhook_retries",
    "test_login_redirect", "test_invoice_rounding", "test_rate_limiter",
]
SHAS = ["a3f9c21", "9be0217", "c7714aa", "5502fde", "1a2b3c4"]
URLS = [
    "https://github.com/acme/web/pull/482",
    "https://github.com/acme/api/actions/runs/998211",
    "https://acme.atlassian.net/browse/W-4821",
]
ENV_VARS = ["DATABASE_URL", "STRIPE_KEY", "REDIS_URL", "JWT_SECRET"]
ERRORS = [
    "connection refused on port 5432",
    "TypeError: cannot read properties of undefined",
    "panic: nil pointer dereference",
    "workspace protocol mismatch in pnpm-lock.yaml",
    "port 3000 already in use",
    "assertion failed: expected 200, got 500",
]
STACK_SNIPPET = (
    "```\nTraceback (most recent call last):\n"
    '  File "app.py", line 42, in handle\n'
    "    raise ValueError('bad input')\n"
    "ValueError: bad input\n```"
)
DIFF_SNIPPET = (
    "```diff\n- return total\n+ return round(total, 2)\n```"
)


def pick(rng: random.Random, pool):
    return rng.choice(pool)


def fill(rng: random.Random, template: str) -> str:
    pkg, ver = pick(rng, PACKAGES)
    return template.format(
        ticket=pick(rng, TICKET_IDS),
        pr=pick(rng, PR_NUMBERS),
        path=pick(rng, PATHS),
        branch=pick(rng, BRANCHES),
        pkg=pkg,
        ver=ver,
        service=pick(rng, SERVICES),
        fn=pick(rng, FUNCTIONS),
        test=pick(rng, TEST_NAMES),
        sha=pick(rng, SHAS),
        url=pick(rng, URLS),
        env=pick(rng, ENV_VARS),
        error=pick(rng, ERRORS),
    )


@dataclass
class Family:
    key: str
    label: str
    templates: list[str]
    contrast_noun: str = ""  # shared noun used to build verb-contrast pairs


FAMILIES: list[Family] = [
    Family("start_ticket", "start work on a ticket", [
        "pick up {ticket}", "let's start on {ticket}", "start work on {ticket}",
        "can you take {ticket} next", "I'd like to begin {ticket} today",
        "kick off {ticket} for me", "get going on {ticket}",
        "please start {ticket}, it's next in the queue",
        "let's tackle {ticket} now", "begin working on {ticket}",
        "assign yourself {ticket} and start", "time to start {ticket}",
        "can we start on the {ticket} ticket", "start the {ticket} work",
        "let's dig into {ticket}", "pick {ticket} off the backlog and start",
        "start on {ticket} -- it's high priority", "go ahead and start {ticket}",
        "I want you to start {ticket}", "begin {ticket}, no need to wait",
        "start {ticket} whenever you're ready", "let's get moving on {ticket}",
        "please begin work on {ticket}", "start {ticket} -- see {url} for context",
        "kick off work on {ticket} now", "start {ticket}, the details are in the ticket",
        "let's start {ticket}, should be quick", "start {ticket} first thing",
        "begin {ticket} before lunch", "start {ticket} -- customer is waiting",
    ], contrast_noun="ticket"),

    Family("check_ticket_status", "check on a ticket", [
        "what's the status of {ticket}", "check on {ticket}",
        "where are we on {ticket}", "look at {ticket} and tell me where it stands",
        "can you check {ticket}'s status", "what's happening with {ticket}",
        "give me an update on {ticket}", "check {ticket} for me",
        "how's {ticket} coming along", "look into the current state of {ticket}",
        "check the status of {ticket}", "peek at {ticket} and summarize progress",
        "what's the latest on {ticket}", "check {ticket}, is it blocked",
        "review the current status of {ticket}", "check on {ticket} real quick",
        "what stage is {ticket} in", "check {ticket} and report back",
        "look up {ticket} status", "can you check where {ticket} stands",
        "check {ticket} -- any updates", "status check on {ticket}",
        "check on {ticket} before standup", "look at {ticket} status only, don't change anything",
        "check {ticket} progress", "check whether {ticket} is done yet",
        "check in on {ticket}", "what is {ticket}'s current status",
        "check {ticket} -- has anyone touched it", "quick status check: {ticket}",
    ], contrast_noun="ticket"),

    Family("close_ticket", "close or resolve a ticket", [
        "close out {ticket}, it's done", "mark {ticket} as resolved",
        "close {ticket}", "we can close {ticket} now",
        "resolve {ticket} and close it", "please close {ticket}",
        "{ticket} is finished, close it", "go ahead and close {ticket}",
        "close {ticket} -- verified fixed", "mark {ticket} closed",
        "close out {ticket} for me", "close {ticket}, it shipped",
        "resolve and close {ticket}", "close {ticket} since it's merged",
        "wrap up {ticket} and close it", "close {ticket} -- confirmed by QA",
        "close {ticket} as done", "let's close {ticket}",
        "close {ticket}, no further action needed", "mark {ticket} resolved and close",
        "close {ticket} -- deployed to prod", "close out {ticket}, all good",
        "close {ticket} now that it's verified", "please mark {ticket} closed",
        "close {ticket}, thanks", "close {ticket} -- duplicate of {sha}",
        "close {ticket} as won't fix", "close {ticket} -- stale",
        "close {ticket} out", "time to close {ticket}",
    ], contrast_noun="ticket"),

    Family("triage_tickets", "triage incoming tickets", [
        "triage the new tickets", "can you triage what's come in overnight",
        "go through the backlog and triage", "triage {ticket} and similar reports",
        "help me triage today's tickets", "sort the incoming bugs by severity",
        "triage the queue and flag anything urgent", "prioritize the open tickets",
        "triage new issues and assign labels", "go through unlabelled tickets and triage them",
        "triage the bug reports from this week", "can you do a triage pass on the backlog",
        "sort incoming tickets by priority", "triage {ticket} into the right bucket",
        "look through new tickets and triage", "triage support tickets from today",
        "help sort the bug queue", "triage and label the new issues",
        "go through the inbox and triage each item", "triage: which of these are urgent",
        "do a first pass triage on new reports", "categorize the incoming tickets",
        "triage {ticket}, {ticket} and the rest of today's batch",
        "sort through new bug reports and prioritize", "triage the support queue",
        "help me figure out which tickets are urgent", "triage new tickets by severity",
        "go triage what came in this morning", "prioritize and label the new tickets",
        "triage the backlog before the next sprint",
    ], contrast_noun="ticket"),

    Family("fix_failing_test", "fix a failing test", [
        "{test} is failing, fix it", "the {test} test is broken, can you fix it",
        "fix {test}, it's red on CI", "{test} keeps failing, look into it and fix",
        "can you fix the failing {test} test", "{test} broke after the last merge, fix it",
        "fix {test} -- it's blocking the release", "please fix {test}",
        "{test} is flaky, make it pass reliably", "go fix {test}",
        "{test} fails locally, can you fix that", "fix the {test} failure",
        "{test} is red, please fix", "can you get {test} passing again",
        "{test} broke in {path}, fix it", "fix {test} so CI goes green",
        "the {test} suite is failing, fix it", "{test} started failing after {sha}, fix it",
        "please make {test} pass", "fix {test} -- error: {error}",
        "{test} is failing on main, fix it", "go fix the {test} test failure",
        "fix {test}, it's been red for a day", "can you patch {test} so it passes",
        "{test} fails with {error}, fix it", "fix this failing test: {test}",
        "make {test} green again", "{test} regressed, please fix",
        "fix {test} before we merge {branch}", "{test} is broken, sort it out",
        "fix the flaky {test} test",
    ], contrast_noun="test"),

    Family("investigate_ci_failure", "investigate a CI failure", [
        "CI is red on main, find out why", "investigate why the build failed",
        "why did CI fail on {branch}", "look into the CI failure at {url}",
        "the pipeline broke, can you investigate", "figure out why CI is failing",
        "investigate the failing build on {branch}", "CI failed with {error}, investigate",
        "what's causing the CI failure", "dig into why the last run failed",
        "investigate the red build", "CI broke after {sha}, look into it",
        "find out what's wrong with the CI run at {url}", "investigate why the pipeline is failing",
        "look into the build failure on {branch}", "why is CI failing on every PR",
        "investigate the CI error: {error}", "check why the workflow run failed",
        "the build is broken, investigate why", "figure out what broke CI",
        "investigate flaky CI on {branch}", "look at the CI logs and figure out the failure",
        "why did the pipeline fail this time", "investigate the failing CI job",
        "CI has been red since {sha}, investigate", "find the root cause of the CI failure",
        "investigate why {test} fails only in CI", "dig into the CI failure logs",
        "investigate the broken build at {url}", "why does CI fail but local tests pass",
    ], contrast_noun="build"),

    Family("review_pr", "review a pull request", [
        "review PR #{pr}", "can you review {branch}'s PR",
        "take a look at PR #{pr}", "review the pull request at {url}",
        "please review PR #{pr} when you get a chance", "review #{pr} for me",
        "can you give PR #{pr} a look", "review the changes in PR #{pr}",
        "review PR #{pr} -- looking for correctness issues",
        "take a pass over PR #{pr}", "review {branch} before we merge",
        "look over PR #{pr}", "review PR #{pr}, focus on {path}",
        "can you review the diff at {url}", "review this PR: {url}",
        "give PR #{pr} a review", "review #{pr} and leave comments",
        "review the pull request for {branch}", "take a look at #{pr} and flag concerns",
        "review PR #{pr} before it goes out", "can you do a code review on #{pr}",
        "review PR #{pr}, especially {path}", "please review #{pr}",
        "review the {branch} pull request", "review #{pr} for style and correctness",
        "look at PR #{pr} and tell me what you think", "review PR #{pr} carefully",
        "can you review {url}", "review #{pr} -- it's ready for eyes",
        "please take a look at PR #{pr}",
    ], contrast_noun="pr"),

    Family("add_feature", "add a new feature", [
        "add a rate limiter to the API", "can you add pagination to {path}",
        "add support for {env} to the config", "add a feature: export orders to CSV",
        "add dark mode to the UI", "can we add retries to {fn}",
        "add a new endpoint for refunds", "add caching to {service}",
        "add a feature flag for the new checkout flow", "add support for webhooks in {service}",
        "add an option to disable email notifications", "add a health check endpoint",
        "add multi-currency support to billing", "can you add a search filter to the dashboard",
        "add a new field to the order model", "add rate limiting to {service}",
        "add a feature to bulk-export tickets", "add SSO login support",
        "add a retry queue for failed webhooks", "add a settings page for notification preferences",
        "add support for uploading attachments", "add a feature toggle for {service}",
        "add pagination and sorting to the {path} endpoint", "add a new report type to the dashboard",
        "add dark-mode support across the app", "add an audit log for admin actions",
        "add support for partial refunds", "add a feature so users can export their data",
        "add throttling to the {service} API", "add a new field {env} to the config schema",
    ], contrast_noun="feature"),

    Family("refactor_code", "refactor existing code", [
        "refactor {path} to use the new client", "can you clean up {fn}",
        "refactor the payment module", "restructure {service} into smaller modules",
        "refactor {fn} to remove duplication", "clean up the code in {path}",
        "refactor {path} for readability", "can we simplify {fn}",
        "refactor the {service} internals without changing behaviour",
        "extract {fn} into its own module", "refactor {path} to reduce complexity",
        "tidy up {path}", "refactor the checkout flow's error handling",
        "can you refactor {fn} to be more testable", "restructure {path} into smaller files",
        "refactor {service} to use dependency injection", "clean up duplicated logic in {path}",
        "refactor {fn}, it's grown too large", "simplify the logic in {path}",
        "refactor {path} -- it's hard to follow", "break {fn} into smaller functions",
        "refactor the {service} client wrapper", "modernize {path} to use async/await",
        "refactor {fn} to remove the global state", "clean up {path} before we add more features",
        "refactor {service} for clarity, no behaviour change", "restructure {fn} to be pure",
        "refactor away the duplication between {path} and its tests",
        "tidy up naming across {service}", "refactor {path} to match the new style guide",
    ], contrast_noun="code"),

    Family("update_dependencies", "update dependencies", [
        "bump {pkg} to {ver}", "update {pkg} to the latest version",
        "can you upgrade {pkg}", "update dependencies, especially {pkg}",
        "bump {pkg} from the old version to {ver}", "upgrade {pkg} to {ver} and fix breakage",
        "update the lockfile after bumping {pkg}", "can we update {pkg} for the CVE fix",
        "bump {pkg} to {ver} across all services", "update {pkg}, it's flagged as vulnerable",
        "upgrade our {pkg} dependency", "bump {pkg} and run the tests",
        "update {pkg} to {ver}, check for breaking changes", "please update {pkg}",
        "bump the {pkg} version to {ver}", "update all outdated dependencies",
        "upgrade {pkg} and resolve any conflicts", "bump {pkg} to fix the security advisory",
        "update {pkg} in package.json to {ver}", "can you bump {pkg} to {ver}",
        "update {pkg}, dependabot flagged it", "upgrade {pkg} across the monorepo",
        "bump {pkg} to {ver} and update the changelog", "update stale dependencies including {pkg}",
        "upgrade {pkg} to the latest patch release", "bump {pkg}, we're two majors behind",
        "update {pkg} to {ver} before the audit", "please bump {pkg} to {ver}",
        "update dependency {pkg} to {ver}", "upgrade {pkg} to patch the vulnerability",
    ], contrast_noun="dependency"),

    Family("write_docs", "write documentation", [
        "write docs for the new endpoint", "can you document {fn}",
        "write a README for {service}", "document the {path} module",
        "add docstrings to {fn}", "write up how {service} works",
        "document the new API endpoint", "write a migration guide for {pkg} {ver}",
        "can you write docs explaining {fn}", "document the {ticket} feature for users",
        "write internal docs on how {service} handles retries",
        "add documentation for the config options", "write a changelog entry for this release",
        "document {path} for new engineers", "write a runbook for {service}",
        "document how to set up {service} locally", "write docs describing the new webhook payload",
        "add comments and docs to {fn}", "write user-facing docs for the export feature",
        "document the deployment process for {service}", "write an architecture doc for {service}",
        "add a docs page for the rate limiter", "document {fn}'s edge cases",
        "write a quickstart guide for {service}", "document the breaking changes in {pkg} {ver}",
        "write docs on how to configure {env}", "add API docs for the new endpoint",
        "write a troubleshooting guide for {error}", "document the retry policy in {service}",
        "write release notes for this version",
    ], contrast_noun="docs"),

    Family("explain_code", "explain what code does", [
        "explain what {fn} does", "can you walk me through {path}",
        "what does this function do: {fn}", "explain the logic in {path}",
        "help me understand {fn}", "can you explain how {service} works",
        "walk me through the {path} file", "explain why {fn} is written this way",
        "what is {fn} supposed to do", "explain the flow in {path}",
        "can you explain this error: {error}", "explain how {service} handles retries",
        "what does {path} do, in plain terms", "explain the algorithm in {fn}",
        "help me understand what {path} is doing", "explain the purpose of {fn}",
        "can you break down {fn} for me", "explain this code path in {service}",
        "what's going on in {path}", "explain why {error} happens here",
        "can you explain the design of {service}", "walk through {fn} step by step",
        "explain what this diff changes", "help me understand the {ticket} implementation",
        "explain how {fn} interacts with {service}", "what does this part of {path} do",
        "explain the control flow in {fn}", "can you clarify what {fn} returns",
        "explain the data model behind {service}", "help me understand this stack trace",
    ], contrast_noun="code"),

    Family("deploy_question", "deploy-related question", [
        "can we deploy {service} now", "how do I deploy {service} to staging",
        "is it safe to deploy {branch}", "what's the process to deploy {service}",
        "deploy {service} to production", "can you deploy the latest {branch} build",
        "how do we roll back {service}", "what's blocking the deploy of {branch}",
        "is {service} deployed yet", "kick off a deploy of {service}",
        "can we roll {service} back to the previous version", "what's our deploy schedule for {service}",
        "how long does a {service} deploy usually take", "deploy {branch} to staging first",
        "is it safe to deploy on a Friday", "who approves the deploy for {service}",
        "can you check if the deploy of {service} succeeded", "what environment does {branch} deploy to",
        "how do I trigger a deploy for {service}", "deploy {service}, then verify with {test}",
        "can we cut a release and deploy {service}", "what's the rollback plan if {service} deploy fails",
        "is the {service} deploy pipeline green", "deploy {branch} once CI passes",
        "how do canary deploys work for {service}", "can you deploy {service} and watch the dashboards",
        "what version of {service} is in production", "deploy {service} behind a feature flag",
        "is there a deploy freeze right now", "how do I promote {branch} to production",
    ], contrast_noun="deploy"),

    Family("setup_environment", "set up the dev environment", [
        "help me set up my dev environment", "how do I get {service} running locally",
        "set up the local environment for {service}", "what do I need to run {service} locally",
        "can you help me configure {env}", "set up my machine to run the test suite",
        "how do I install the dependencies for {service}", "walk me through setting up {service} locally",
        "set up the .env file for {service}", "what's the local setup for {pkg}",
        "help me configure {env} for local dev", "set up a local database for {service}",
        "how do I get the dev environment running", "can you set up {service} on this machine",
        "install everything needed to run {service}", "set up docker for {service}",
        "what environment variables does {service} need", "help me set up {pkg} locally",
        "set up my editor for this repo", "how do I bootstrap the local environment",
        "configure {env} and {env} for local development", "set up the project so I can run it locally",
        "what's missing from my local {service} setup", "help me fix my broken dev environment",
        "set up pre-commit hooks for this repo", "how do I seed the local database for {service}",
        "get {service} running in docker-compose", "set up the local toolchain for {pkg}",
        "help me configure {service} for local testing", "walk me through first-time setup",
    ], contrast_noun="environment"),
]

assert len(FAMILIES) >= 12, "need at least 12 intent families"


# --------------------------------------------------------------------------
# Verb-contrast hard subset: same noun, different verb/intent, drawn from
# different families. These are the pairs the spike hypothesis expects
# static embeddings to struggle with.
# --------------------------------------------------------------------------

CONTRAST_GROUPS = [
    ("start_ticket", "close_ticket", "ticket"),
    ("start_ticket", "check_ticket_status", "ticket"),
    ("check_ticket_status", "close_ticket", "ticket"),
    ("close_ticket", "triage_tickets", "ticket"),
    ("fix_failing_test", "investigate_ci_failure", "test/build"),
    ("add_feature", "refactor_code", "code"),
    ("refactor_code", "explain_code", "code"),
    ("review_pr", "add_feature", "pr/feature"),
    ("update_dependencies", "investigate_ci_failure", "dependency/build"),
    ("deploy_question", "setup_environment", "deploy/env"),
]


# --------------------------------------------------------------------------
# Noise pool: real-looking prompts that belong to none of the above families.
# Mixes chit-chat, generic knowledge questions, off-family technical asks,
# and short conversational acknowledgements.
# --------------------------------------------------------------------------

NOISE_TEMPLATES = [
    "what's the weather like today", "tell me a joke", "how are you doing",
    "thanks, that helps", "ok sounds good", "yes please go ahead",
    "no that's not what I meant", "can you say that differently",
    "what's your favorite programming language", "translate this to spanish: {error}",
    "summarize this article for me", "what time is it",
    "how do I center a div with css", "what's the capital of France",
    "recommend a good book on distributed systems", "what does HTTP 429 mean",
    "explain the CAP theorem", "what's the difference between TCP and UDP",
    "how many people work at {service}", "can you write a haiku about {pkg}",
    "what's 17 times 23", "convert 10 miles to kilometers",
    "what's a good name for a cat", "tell me about the history of {pkg}",
    "how do I make coffee", "what's the tallest mountain in the world",
    "can you recommend a podcast", "what's the plot of that movie",
    "how's the weather in Tokyo", "give me a fun fact",
    "what's the difference between a list and a tuple in python",
    "how do I say hello in japanese", "what's a good workout routine",
    "tell me about black holes", "what's the meaning of life",
    "can you write a limerick", "what's your opinion on tabs vs spaces",
    "how long does it take to boil an egg", "what's the best pizza topping",
    "recommend a restaurant near {service}", "what's the origin of the word 'debug'",
    "ok thanks", "sounds good, go ahead", "yep that's right", "sure, why not",
    "cool, thanks for the help", "got it", "no worries", "sounds fine to me",
    "great, thanks", "perfect", "ok go for it", "alright, proceed",
    "hmm not sure", "let me think about it", "maybe later", "not right now",
    "can you repeat that", "I don't understand", "what do you mean",
    "let's talk about something else", "never mind", "forget it",
    "how was your weekend", "do you have any hobbies", "what's new",
    "can you tell me a story", "what's the news today",
    "what's the difference between a virus and a bacteria",
    "explain quantum entanglement", "what's the best way to learn spanish",
    "how do airplanes fly", "what causes rainbows",
    "what's a good gift for a birthday", "how do I fold a paper crane",
    "what's the population of Japan", "recommend a good movie",
    "what's your favorite color", "do you dream",
    "how do solar panels work", "what's the speed of light",
    "explain how vaccines work", "what's a black swan event",
    "how do I train for a marathon", "what's the best chess opening",
    "can you help me plan a trip to Italy", "what's a good budget for a wedding",
    "explain compound interest", "what's the difference between stocks and bonds",
    "how do noise-cancelling headphones work", "what's the tallest building in the world",
    "recommend some music", "what's a good recipe for pasta",
    "how does gps work", "what's the largest ocean",
    "explain how the internet works, generally", "what's a leap year",
    "how do bees make honey", "what's the boiling point of water",
    "recommend a good text editor theme", "what's your favorite food",
    "how do I whistle", "what's the driest place on earth",
    "explain photosynthesis", "what's a fun fact about octopuses",
    "how many bones are in the human body", "what's the origin of chess",
    "recommend a good sci-fi book", "what's the difference between weather and climate",
    "how do I tie a tie", "what's a good icebreaker question",
    "explain how batteries work", "what's the freezing point of water",
    "how do I improve my typing speed", "what's a good stretching routine",
    "recommend a language to learn next", "what's the loneliest number",
    "how does encryption work, at a high level", "what's a fun weekend activity",
    "explain the stock market crash of 1929", "what's a good name for a startup",
    "how do clouds form", "what's the tallest tree species",
    "recommend a good coffee brand", "what's a fun trivia question",
    "how do magnets work", "what's the deepest part of the ocean",
    "explain how airplanes stay balanced", "what's a good morning routine",
    "how do I get better at public speaking", "what's the most spoken language",
    "recommend a good hiking trail", "what's a fun fact about space",
    "how do seasons happen", "what's the largest desert",
    "explain how memory works in the brain", "what's a good podcast about history",
]


def build_family_rows(rng: random.Random, per_family: int) -> list[dict]:
    rows = []
    idx = 0
    for fam in FAMILIES:
        templates = fam.templates
        # Deterministically cycle + shuffle templates, filling slots with a
        # fresh RNG draw each time so paraphrases vary in surface form.
        pool = list(templates)
        rng.shuffle(pool)
        chosen = []
        while len(chosen) < per_family:
            chosen.extend(pool)
            rng.shuffle(pool)
        chosen = chosen[:per_family]
        for i, tmpl in enumerate(chosen):
            text = fill(rng, tmpl)
            # Occasionally attach a pasted snippet / long tail to exercise
            # the normaliser's fence-stripping and head-truncation.
            extra = ""
            r = rng.random()
            if r < 0.12:
                extra = "\n\n" + STACK_SNIPPET
            elif r < 0.20:
                extra = "\n\n" + DIFF_SNIPPET
            elif r < 0.28:
                extra = (
                    " -- also here's some more context that runs on for a "
                    "while so we can check head truncation: " + " ".join(
                        [f"detail{n}" for n in range(40)]
                    )
                )
            rows.append({
                "id": f"{fam.key}-{idx:04d}",
                "family": fam.key,
                "family_label": fam.label,
                "text": text + extra,
                "is_noise": False,
                "is_hard": False,
                "contrast_group": None,
                "contrast_noun": fam.contrast_noun,
            })
            idx += 1
    return rows


def build_hard_rows(rng: random.Random, per_group: int) -> list[dict]:
    fam_by_key = {f.key: f for f in FAMILIES}
    rows = []
    for gi, (fa, fb, noun) in enumerate(CONTRAST_GROUPS):
        for fam_key in (fa, fb):
            fam = fam_by_key[fam_key]
            pool = list(fam.templates)
            rng.shuffle(pool)
            for i in range(per_group):
                tmpl = pool[i % len(pool)]
                text = fill(rng, tmpl)
                rows.append({
                    "id": f"hard-{gi:02d}-{fam_key}-{i:02d}",
                    "family": fam_key,
                    "family_label": fam.label,
                    "text": text,
                    "is_noise": False,
                    "is_hard": True,
                    "contrast_group": f"{gi:02d}:{fa}-vs-{fb}",
                    "contrast_noun": noun,
                })
    return rows


NOISE_PREFIXES = [
    "", "", "", "hey, ", "quick question -- ", "random thought: ",
    "by the way, ", "so, ", "ok, ", "actually, ", "one more thing -- ",
]


def build_noise_rows(rng: random.Random, count: int) -> list[dict]:
    pool = list(NOISE_TEMPLATES)
    rng.shuffle(pool)
    rows = []
    seen_text = set()
    i = 0
    guard = 0
    while len(rows) < count and guard < count * 20:
        guard += 1
        tmpl = pool[i % len(pool)]
        i += 1
        if i % len(pool) == 0:
            rng.shuffle(pool)
        base = fill(rng, tmpl) if "{" in tmpl else tmpl
        prefix = pick(rng, NOISE_PREFIXES)
        text = prefix + base if not prefix else prefix + base[0].lower() + base[1:]
        if text in seen_text:
            continue
        seen_text.add(text)
        rows.append({
            "id": f"noise-{len(rows):04d}",
            "family": None,
            "family_label": None,
            "text": text,
            "is_noise": True,
            "is_hard": False,
            "contrast_group": None,
            "contrast_noun": None,
        })
    return rows


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="data/prompts.jsonl")
    ap.add_argument("--seed", type=int, default=20260922)
    ap.add_argument("--per-family", type=int, default=40)
    ap.add_argument("--per-contrast-group", type=int, default=10)
    ap.add_argument("--noise-count", type=int, default=220)
    args = ap.parse_args()

    rng = random.Random(args.seed)
    rows = []
    rows += build_family_rows(rng, args.per_family)
    rows += build_hard_rows(rng, args.per_contrast_group)
    rows += build_noise_rows(rng, args.noise_count)

    # De-duplicate exact text collisions (keep first), which can happen
    # when slot-filling collapses two templates onto the same string.
    seen = set()
    unique = []
    for r in rows:
        if r["text"] in seen:
            continue
        seen.add(r["text"])
        unique.append(r)

    with open(args.out, "w") as f:
        for r in unique:
            f.write(json.dumps(r) + "\n")

    n_fam = sum(1 for r in unique if not r["is_noise"] and not r["is_hard"])
    n_hard = sum(1 for r in unique if r["is_hard"])
    n_noise = sum(1 for r in unique if r["is_noise"])
    print(f"wrote {len(unique)} rows -> {args.out}")
    print(f"  family rows: {n_fam} across {len(FAMILIES)} families")
    print(f"  hard (verb-contrast) rows: {n_hard} across {len(CONTRAST_GROUPS)} groups")
    print(f"  noise rows: {n_noise}")


if __name__ == "__main__":
    main()
