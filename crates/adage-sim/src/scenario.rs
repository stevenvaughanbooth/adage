//! The acme-billing scenario as harness-neutral sessions (demo/SANDBOX.md): work items only via
//! `python3 scripts/wi-lookup <ID>`, tests only via `make test` (running unittest or pytest directly
//! is a dead end), and `make gen` after any `schema/*.json` edit. Every session is generated from a
//! seed, so fixtures are reproducible byte for byte; phrasing, exploration, read batching, and
//! which runner an agent tries first vary between sessions and between harness seeds.

/// Where the mock sessions ran. It need not exist: Adage's repository key for it is its directory
/// name, `acme-billing`, the same key a real checkout called `acme-billing` gets.
pub const CWD: &str = "/fixture/acme-billing";

/// 2026-09-01T00:00:00Z.
pub const EPOCH_MS: i64 = 1_788_220_800_000;

/// splitmix64: small, fast, and identical on every platform.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed ^ 0x9e37_79b9_7f4a_7c15)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n.max(1) as u64) as usize
    }
    pub fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
    pub fn chance(&mut self, percent: u64) -> bool {
        self.next_u64() % 100 < percent
    }
    pub fn hex(&mut self, n: usize) -> String {
        (0..n).map(|_| char::from_digit((self.next_u64() % 16) as u32, 16).unwrap()).collect()
    }
    pub fn base62(&mut self, n: usize) -> String {
        const C: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
        (0..n).map(|_| C[self.below(62)] as char).collect()
    }
}

/// A work item in the sandbox tracker (`demo/sandbox/generate.py`).
#[derive(Debug, Clone, Copy)]
pub struct Item {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub linked: &'static [&'static str],
    pub acceptance: &'static str,
    /// For schema changes: the schema file and the field added to it.
    pub schema: Option<(&'static str, &'static str)>,
    /// The code change: (file, old line, new line).
    pub fix: (&'static str, &'static str, &'static str),
}

pub const ITEMS: &[Item] = &[
    Item {
        id: "W-1041",
        title: "Fix discount rounding",
        description: "Discount calculations are losing cents; round half to even, not floor.",
        linked: &["acme/billing.py"],
        acceptance: "10% discount on $99.99 cents yields $89.99, not $89.98",
        schema: None,
        fix: (
            "acme/billing.py",
            "    return math.floor(amount * (100 - pct)) // 100",
            "    return round_half_even(amount * (100 - pct), 100)",
        ),
    },
    Item {
        id: "W-1042",
        title: "EU tax region uses wrong rate",
        description: "Invoices for customers in the EU are charged at 15% tax, but should be 20%.",
        linked: &["acme/billing.py"],
        acceptance: "EU region tax rate is 20%, not 15%",
        schema: None,
        fix: ("acme/billing.py", "TAX_RATES = {\"us\": 8, \"eu\": 15}", "TAX_RATES = {\"us\": 8, \"eu\": 20}"),
    },
    Item {
        id: "W-1043",
        title: "Late fees applied too early",
        description: "Late fees are applied on the due date, but should only apply after it has passed.",
        linked: &["acme/billing.py"],
        acceptance: "Late fee applies only when payment is after due_date, not on it",
        schema: None,
        fix: ("acme/billing.py", "    if paid_on >= due_date:", "    if paid_on > due_date:"),
    },
    Item {
        id: "W-1044",
        title: "Email validation too lenient",
        description: "Email validation accepts 'user@localhost'; require a dot in the domain.",
        linked: &["acme/customers.py"],
        acceptance: "'user@localhost' rejected, 'user@example.com' accepted",
        schema: None,
        fix: (
            "acme/customers.py",
            "    return \"@\" in email",
            "    return \"@\" in email and \".\" in email.split(\"@\", 1)[1]",
        ),
    },
    Item {
        id: "W-1045",
        title: "Add currency field to invoices",
        description:
            "Add a 'currency' string field with default 'USD' to the invoice schema; use it in format_invoice.",
        linked: &["schema/invoice.json", "acme/billing.py"],
        acceptance: "Invoice schema has currency (default USD); format_invoice includes it",
        schema: Some(("schema/invoice.json", "currency")),
        fix: (
            "acme/billing.py",
            "    return f\"{inv.id}: {total}\"",
            "    return f\"{inv.id}: {total} {inv.currency}\"",
        ),
    },
    Item {
        id: "W-1046",
        title: "Add configurable due days",
        description: "Add a 'due_days' int field with default 30 to the invoice schema.",
        linked: &["schema/invoice.json", "acme/billing.py"],
        acceptance: "Invoice schema has due_days (int, default 30); generated models include it",
        schema: Some(("schema/invoice.json", "due_days")),
        fix: (
            "acme/billing.py",
            "    return issued + timedelta(days=30)",
            "    return issued + timedelta(days=inv.due_days)",
        ),
    },
    Item {
        id: "W-2041",
        title: "Invoice amounts drop the cents",
        description: "Formatted invoices lose the cents: $150.50 is shown as $150.",
        linked: &["acme/billing.py"],
        acceptance: "format_invoice shows dollars and cents",
        schema: None,
        fix: (
            "acme/billing.py",
            "    total = inv.amount_cents // 100",
            "    total = f\"{inv.amount_cents / 100:.2f}\"",
        ),
    },
    Item {
        id: "W-2045",
        title: "Add payment terms",
        description: "Add a 'payment_terms' string field with default 'NET30' to the invoice schema.",
        linked: &["schema/invoice.json", "acme/billing.py"],
        acceptance: "Invoice schema has payment_terms (str, default NET30); generated models include it",
        schema: Some(("schema/invoice.json", "payment_terms")),
        fix: (
            "acme/billing.py",
            "    return f\"{inv.id}: {total}\"",
            "    return f\"{inv.id}: {total} ({inv.payment_terms})\"",
        ),
    },
];

pub fn item(id: &str) -> Item {
    *ITEMS.iter().find(|i| i.id == id).expect("known work item")
}

/// One tool call, harness-neutral. Paths are repository-relative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    Shell { cmd: String, out: String, exit: i32 },
    Read { path: String, body: String },
    Edit { path: String, old: String, new: String },
}

impl Act {
    pub fn failed(&self) -> bool {
        matches!(self, Act::Shell { exit, .. } if *exit != 0)
    }
    pub fn shell(cmd: &str, out: &str, exit: i32) -> Act {
        Act::Shell { cmd: cmd.into(), out: out.into(), exit }
    }
    pub fn read(path: &str) -> Act {
        Act::Read { path: path.into(), body: body(path) }
    }
    pub fn edit(path: &str, old: &str, new: &str) -> Act {
        Act::Edit { path: path.into(), old: old.into(), new: new.into() }
    }
}

/// A tool call and, in a replay, the id prefixes of the routes whose directives it should draw.
#[derive(Debug, Clone)]
pub struct Call {
    pub act: Act,
    pub expect: Vec<&'static str>,
}

/// One model turn: optional commentary, then one or more (parallel) tool calls.
#[derive(Debug, Clone, Default)]
pub struct Turn {
    pub say: Option<String>,
    pub calls: Vec<Call>,
}

impl Turn {
    fn of(say: Option<String>, acts: Vec<Act>) -> Turn {
        Turn { say, calls: acts.into_iter().map(|act| Call { act, expect: vec![] }).collect() }
    }
}

#[derive(Debug, Clone)]
pub struct Session {
    /// Stable fixture name, e.g. `03-W-1045`.
    pub name: String,
    pub prompt: String,
    pub turns: Vec<Turn>,
    pub answer: String,
    pub start_ms: i64,
    /// Seed for this session's harness-specific ids and timings.
    pub seed: u64,
}

// ---------------------------------------------------------------------------------------------
// Repository content (short, but shaped like the sandbox's files)

pub fn body(path: &str) -> String {
    match path {
        "acme/billing.py" => "import math\nfrom datetime import timedelta\n\nTAX_RATES = {\"us\": 8, \"eu\": 15}\n\ndef apply_discount(amount, pct):\n    return math.floor(amount * (100 - pct)) // 100\n\ndef due_date(inv, issued):\n    return issued + timedelta(days=30)\n\ndef format_invoice(inv):\n    total = inv.amount_cents // 100\n    return f\"{inv.id}: {total}\"\n".into(),
        "acme/customers.py" => "def valid_email(email):\n    return \"@\" in email\n\ndef region_of(customer):\n    return customer.region\n".into(),
        "schema/invoice.json" => "{\"name\": \"Invoice\", \"fields\": [\n    {\"name\": \"id\", \"type\": \"str\"},\n    {\"name\": \"amount_cents\", \"type\": \"int\", \"default\": 0},\n    {\"name\": \"tax_region\", \"type\": \"str\", \"default\": \"us\"}\n]}\n".into(),
        "schema/customer.json" => "{\"name\": \"Customer\", \"fields\": [\n    {\"name\": \"id\", \"type\": \"str\"},\n    {\"name\": \"email\", \"type\": \"str\"}\n]}\n".into(),
        "README.md" => "# acme-billing\n\nBilling for Acme. Work items are tracked in the internal tracker.\n\nSee the Makefile for common tasks.\n".into(),
        "Makefile" => ".PHONY: gen test ci lint\n\ngen:\n\tpython3 tools/gen.py\n\ntest:\n\tACME_ENV=test python3 -m unittest discover -s tests\n\nci:\n\t@python3 tools/gen.py --check\n\tACME_ENV=test python3 -m unittest discover -s tests\n".into(),
        _ => String::new(),
    }
}

pub fn lookup_output(it: &Item) -> String {
    format!(
        "ID: {}\nTitle: {}\nDescription: {}\nLinked files: {}\nAcceptance: {}\n",
        it.id,
        it.title,
        it.description,
        it.linked.join(", "),
        it.acceptance
    )
}

pub const LS: &str = "Makefile\nREADME.md\nacme\nschema\nscripts\ntests\ntools\n";
pub const MAKE_TEST_OK: &str = "ACME_ENV=test python3 -m unittest discover -s tests\n..............\n----------------------------------------------------------------------\nRan 14 tests in 0.006s\n\nOK\n";
pub const MAKE_GEN_OK: &str = "python3 tools/gen.py\nwrote acme/generated/models.py (2 records)\n";
pub const MAKE_LINT_OK: &str = "ruff check acme\nAll checks passed!\n";
/// The work-item sessions of a history (its first ones) that run `make lint` after `make test`.
pub const LINT_SESSIONS: usize = 2;
pub const ACME_ENV: &str = "Traceback (most recent call last):\n  File \"/usr/lib/python3.9/runpy.py\", line 197, in _run_module_as_main\n    return _run_code(code, main_globals, None,\n  File \"/fixture/acme-billing/tests/__init__.py\", line 7, in <module>\n    raise RuntimeError(\"ACME_ENV is not set — run tests with 'make test'\")\nRuntimeError: ACME_ENV is not set — run tests with 'make test'\n";

/// The ways agents try to run the tests before finding `make test`: (command, output, exit).
pub const DEAD_ENDS: &[(&str, &str, i32)] = &[
    ("python3 -m unittest discover -s tests", ACME_ENV, 1),
    ("python3 -m unittest", ACME_ENV, 1),
    ("python3 -m pytest tests", "/usr/bin/python3: No module named pytest\n", 1),
];

fn schema_edit(it: &Item) -> Option<Act> {
    let (file, field) = it.schema?;
    let old = "    {\"name\": \"tax_region\", \"type\": \"str\", \"default\": \"us\"}";
    let (ty, default) = match field {
        "due_days" => ("int", "30"),
        "currency" => ("str", "\"USD\""),
        "payment_terms" => ("str", "\"NET30\""),
        _ => ("str", "\"\""),
    };
    let new = format!("{old},\n    {{\"name\": \"{field}\", \"type\": \"{ty}\", \"default\": {default}}}");
    Some(Act::edit(file, old, &new))
}

const TICKET_PROMPTS: &[&str] = &[
    "Pick up {id} and get it done.",
    "Please work on {id}.",
    "Can you take care of {id}? Thanks",
    "{id} is assigned to you. Fix it and make sure the tests pass.",
    "Handle work item {id} end to end.",
    "Let's get {id} finished today.",
    "Implement {id}.",
];

const OPENERS: &[&str] = &[
    "I'll start by looking around the repository.",
    "Let me get oriented first.",
    "First, a quick look at the project layout.",
];

const ANSWERS: &[&str] = &[
    "Done: {id} is fixed and `make test` passes.",
    "{id} is implemented; all tests pass.",
    "Finished {id}. The change is small and the test suite is green.",
];

/// A work-item session: explore, look the item up, read what it links, change it, test.
/// `k` fixes the structure (dead end or not, read batching); `rng` varies the details.
fn ticket_session(rng: &mut Rng, it: &Item, k: usize, start_ms: i64) -> Session {
    let prompt = rng.pick(TICKET_PROMPTS).replace("{id}", it.id);
    let mut turns = Vec::new();
    let explore: Vec<Act> = match rng.below(4) {
        0 => vec![Act::shell("ls", LS, 0)],
        1 => vec![Act::shell("ls", LS, 0), Act::read("README.md")],
        2 => vec![Act::read("README.md")],
        _ => vec![Act::shell("git status --short", "", 0), Act::shell("ls", LS, 0)],
    };
    turns.push(Turn::of(Some(rng.pick(OPENERS).to_string()), explore));
    if rng.chance(40) {
        turns.push(Turn::of(None, vec![Act::shell(&format!("rg -n {} .", it.id), "", 1)]));
    }
    turns.push(Turn::of(
        Some("The README mentions an internal tracker; there is a lookup script.".into()),
        vec![Act::shell(&format!("python3 scripts/wi-lookup {}", it.id), &lookup_output(it), 0)],
    ));
    // Linked files one turn at a time (the desire path) or together.
    if k.is_multiple_of(2) {
        for f in it.linked {
            turns.push(Turn::of(None, vec![Act::read(f)]));
        }
    } else {
        turns.push(Turn::of(None, it.linked.iter().map(|f| Act::read(f)).collect()));
    }
    if let Some(e) = schema_edit(it) {
        turns.push(Turn::of(Some("Adding the field to the schema.".into()), vec![e]));
    }
    let (f, old, new) = it.fix;
    turns.push(Turn::of(None, vec![Act::edit(f, old, new)]));
    if it.schema.is_some() {
        turns.push(Turn::of(Some("Regenerating the models.".into()), vec![Act::shell("make gen", MAKE_GEN_OK, 0)]));
    }
    if k % 3 != 2 {
        let (cmd, out, exit) = *rng.pick(DEAD_ENDS);
        turns.push(Turn::of(Some("Running the tests.".into()), vec![Act::shell(cmd, out, exit)]));
        turns.push(Turn::of(
            Some("The tests need ACME_ENV; the Makefile sets it.".into()),
            vec![Act::shell("make test", MAKE_TEST_OK, 0)],
        ));
    } else {
        turns.push(Turn::of(Some("Running the tests.".into()), vec![Act::shell("make test", MAKE_TEST_OK, 0)]));
    }
    // The deliberately sub-support habit (plan §2.7.1): in the first two work-item sessions the
    // agent runs `make lint` right after `make test`. Two traces are below the demo preset's
    // support of three, inside the candidates' border: learning counts it as a candidate, and a
    // later session's `make test` notices it. Drawn from no generator, so every other session is
    // unchanged.
    if k < LINT_SESSIONS {
        turns.push(Turn::of(None, vec![Act::shell("make lint", MAKE_LINT_OK, 0)]));
    }
    let answer = rng.pick(ANSWERS).replace("{id}", it.id);
    Session { name: format!("{k:02}-{}", it.id), prompt, turns, answer, start_ms, seed: rng.next_u64() }
}

/// The work-item sessions of one harness's history, plus noise: a session that never finds the
/// tracker, a question with no work item, and an unrelated edit.
pub fn history(seed: u64) -> Vec<Session> {
    let mut rng = Rng::new(seed);
    let order = ["W-1041", "W-1042", "W-1045", "W-1043", "W-1046", "W-1044", "W-1045", "W-1043", "W-1046"];
    let mut t = EPOCH_MS + 9 * 3_600_000;
    let mut out = Vec::new();
    let mut next_start = |rng: &mut Rng| {
        t += 26 * 3_600_000 + rng.below(90) as i64 * 60_000;
        t
    };
    for (k, id) in order.iter().enumerate() {
        let start = next_start(&mut rng);
        out.push(ticket_session(&mut rng, &item(id), k, start));
    }
    let k = order.len();
    let start = next_start(&mut rng);
    out.push(Session {
        name: format!("{k:02}-noise-giveup"),
        prompt: "Look into W-1044 when you get a chance.".into(),
        turns: vec![
            Turn::of(None, vec![Act::shell("ls", LS, 0)]),
            Turn::of(None, vec![Act::shell("rg -n W-1044 .", "", 1)]),
            Turn::of(None, vec![Act::read("README.md")]),
        ],
        answer: "I couldn't find W-1044 anywhere in the repository. Where is the tracker?".into(),
        start_ms: start,
        seed: rng.next_u64(),
    });
    let start = next_start(&mut rng);
    out.push(Session {
        name: format!("{:02}-noise-question", k + 1),
        prompt: "How are late fees calculated here?".into(),
        turns: vec![
            Turn::of(None, vec![Act::shell("rg -n late acme", "acme/billing.py:22:def late_fee(inv, paid_on):\n", 0)]),
            Turn::of(None, vec![Act::read("acme/billing.py")]),
        ],
        answer: "Late fees are computed in acme/billing.py by late_fee().".into(),
        start_ms: start,
        seed: rng.next_u64(),
    });
    let start = next_start(&mut rng);
    out.push(Session {
        name: format!("{:02}-noise-tidy", k + 2),
        prompt: "Tidy up the docstrings in acme/customers.py and run the tests.".into(),
        turns: vec![
            Turn::of(None, vec![Act::read("acme/customers.py")]),
            Turn::of(
                None,
                vec![Act::edit(
                    "acme/customers.py",
                    "def valid_email(email):",
                    "def valid_email(email):\n    \"\"\"True if the address looks deliverable.\"\"\"",
                )],
            ),
            Turn::of(None, vec![Act::shell("make test", MAKE_TEST_OK, 0)]),
        ],
        answer: "Docstrings added; tests pass.".into(),
        start_ms: start,
        seed: rng.next_u64(),
    });
    out
}

/// A held-out session (W-2045, a schema change) replayed through a harness's hooks after
/// learning, with the directives each moment should draw in the treated arm:
///
/// | Moment | Route drawn |
/// | --- | --- |
/// | the prompt | `lookup-work-item`, and the known dead end (`dead-end-…`, announced at the first prompt) |
/// | a look around (`ls`) | nothing |
/// | the lookup's result | `read-linked-files` |
/// | the schema edit | `finish-line-…` |
/// | the first `make test` after the code edit | nothing said; the `make lint` candidate is noticed ([`Replay::noticed`]) |
/// | the first stop | a request to continue: `make gen` has not run |
/// | the second stop, after `make gen` | nothing |
///
/// Under adage-exec with `predict` grants ([`Replay::predict`]), Adage makes steps in the agent's
/// place: the lookup at the step after the look around, the reads it names at the very next step
/// (a chain), and `make gen` at the work turn's end, so the first stop never comes.
#[derive(Debug, Clone)]
pub struct Replay {
    pub item: Item,
    pub prompt: String,
    pub prompt_expect: Vec<&'static str>,
    /// Turns before the first stop, which should draw a request to continue.
    pub work: Vec<Turn>,
    /// Turns after the continuation; the second stop draws nothing.
    pub finish: Vec<Turn>,
    pub answer: String,
    /// The steps Adage makes in the agent's place ([`Predict`]), in order, each when its route
    /// holds a `predict` grant.
    pub predict: Vec<Predict>,
    /// The candidates the session notices ([`Notice`]).
    pub noticed: Vec<Notice>,
    /// What the mod's `PreToolUse` answers for each Bash call, in call order ([`Pretool`]).
    pub pretool: Vec<Pretool>,
}

/// A step Adage makes in the agent's place at a `TurnStep`, when the route holds a `predict` grant
/// under `adage-exec hook`. The treated arm prints it; the control arm prints nothing and spools it
/// as `would_predict`; without the grant, or under `adage hook` (no extension, so no grant), there
/// is nothing in either arm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Predict {
    /// The step boundary: before request `index` of the work turn, or after the model's response
    /// to it ended the turn (`ending`).
    pub index: u32,
    /// The step the control arm records it at (`would_predict`): `index`, but for the reads after
    /// a lookup made before the first request, which the control session's agent makes itself
    /// after its look around, so the reads it binds are due a step later than in the treated arm.
    pub control_index: u32,
    pub ending: bool,
    /// `run`, `sequence`, `skill`, `chain` or `obligation` (the binary's `PredictionKind`).
    pub kind: &'static str,
    /// The route's id prefix, the one the grant names.
    pub route: &'static str,
    /// The calls Adage makes, in one response (several for a chain of reads); the agent's own
    /// turn of them is then not replayed.
    pub acts: Vec<Act>,
}

/// A candidate the held-out session notices (plan D2): learning counted the habit below a
/// reflex's support, and the call whose batch completes its context notices it, once, in both
/// arms, with nothing said to the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// The command the candidate would direct.
    pub step: &'static str,
    /// The command whose batch notices it: the first such call after the edit its context starts
    /// with (which turn that is depends on the arm: the treated arm's substituted dead end runs it
    /// a turn earlier).
    pub at: &'static str,
}

/// One Bash call as the mod's two-phase `PreToolUse` sees it. Without a verdict the final ask
/// answers what the dry run plans, except for a dead end the mod replaces ([`substitutions`],
/// whose plan names the learned command): nothing in this scenario is rewritten, since no
/// environment is bound to rewrite into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pretool {
    pub command: String,
    /// `None`: the hook prints nothing, in both phases and both arms.
    pub answer: Option<&'static str>,
}

/// The steps under `predict` grants. Before the model's first request (step 0), the lookup the
/// prompt directed, its slot bound from the prompt: it stands for the agent's turn after the look
/// around (`work[1]`); at the very next step, the reads its result binds, as one response of
/// several calls (the chain's second step, the only one that may follow a prediction at once);
/// and at the work turn's end, which the model reaches without `make gen` (request `work.len()`
/// is its last), the finish line's call, so no Stop request follows. Each is made only for a
/// granted route; the sim grants the lookup and the reads together (a grant on the reads alone
/// would make them after the agent's own lookup, at step 2).
fn predict_expectations(work: &[Turn]) -> Vec<Predict> {
    let acts = |t: &Turn| t.calls.iter().map(|c| c.act.clone()).collect::<Vec<_>>();
    vec![
        Predict {
            index: 0,
            control_index: 0,
            ending: false,
            kind: "run",
            route: "lookup-work-item",
            acts: acts(&work[1]),
        },
        Predict {
            index: 1,
            control_index: 2,
            ending: false,
            kind: "chain",
            route: "read-linked-files",
            acts: acts(&work[2]),
        },
        Predict {
            index: work.len() as u32,
            control_index: work.len() as u32,
            ending: true,
            kind: "obligation",
            route: "finish-line-",
            acts: vec![Act::shell("make gen", MAKE_GEN_OK, 0)],
        },
    ]
}

fn pretool_expectations(turns: &[&Turn]) -> Vec<Pretool> {
    turns
        .iter()
        .flat_map(|t| &t.calls)
        .filter_map(|c| match &c.act {
            Act::Shell { cmd, .. } => Some(Pretool { command: cmd.clone(), answer: None }),
            _ => None,
        })
        .collect()
}

pub fn replay() -> Replay {
    let it = item("W-2045");
    let with = |act: Act, expect: Vec<&'static str>| Call { act, expect };
    let turn = |calls: Vec<Call>| Turn { say: None, calls };
    let (f, old, new) = it.fix;
    let (cmd, out, exit) = DEAD_ENDS[0];
    let mut r = Replay {
        item: it,
        prompt: format!("Pick up {} and get it done.", it.id),
        prompt_expect: vec!["lookup-work-item", "dead-end-"],
        predict: vec![],
        pretool: vec![],
        noticed: vec![Notice { step: "make lint", at: "make test" }],
        work: vec![
            turn(vec![with(Act::shell("ls", LS, 0), vec![])]),
            turn(vec![with(
                Act::shell(&format!("python3 scripts/wi-lookup {}", it.id), &lookup_output(&it), 0),
                vec!["read-linked-files"],
            )]),
            turn(it.linked.iter().map(|p| with(Act::read(p), vec![])).collect()),
            turn(vec![with(schema_edit(&it).unwrap(), vec!["finish-line-"])]),
            turn(vec![with(Act::edit(f, old, new), vec![])]),
            turn(vec![with(Act::shell(cmd, out, exit), vec![])]),
            turn(vec![with(Act::shell("make test", MAKE_TEST_OK, 0), vec![])]),
        ],
        finish: vec![
            turn(vec![with(Act::shell("make gen", MAKE_GEN_OK, 0), vec![])]),
            turn(vec![with(Act::shell("make test", MAKE_TEST_OK, 0), vec![])]),
        ],
        answer: format!("{} is done: payment_terms added, models regenerated, tests pass.", it.id),
    };
    r.predict = predict_expectations(&r.work);
    r.pretool = pretool_expectations(&r.work.iter().chain(&r.finish).collect::<Vec<_>>());
    r
}

/// A dead end the mod replaces before it runs (plan C6): the held-out session's call meets the
/// learned `avoid` route, so its `PreToolUse` dry run plans `instead`, and on the same permission
/// verdict the treated arm runs `instead` in its place with one line of context: the failure, and
/// the model's reading of it, never happen. The control arm runs the dead end and spools
/// `would_substitute`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Substitute {
    /// The call as the agent made it.
    pub command: String,
    /// What runs in its place, and what it prints.
    pub instead: Act,
    /// The route's id prefix (a learned dead end's, or a declared instinct's: both `dead-end-`).
    pub route: &'static str,
    /// How the context line begins; the rest quotes the failure signature learning found, or
    /// gives a declared instinct's reason.
    pub context: String,
}

/// The substitutions of [`replay`]: the dead-end turn, `python3 -m unittest discover -s tests`,
/// becomes `make test`.
pub fn substitutions(r: &Replay) -> Vec<Substitute> {
    let (dead, ..) = DEAD_ENDS[0];
    r.work
        .iter()
        .chain(&r.finish)
        .flat_map(|t| &t.calls)
        .filter(|c| matches!(&c.act, Act::Shell { cmd, .. } if cmd == dead))
        .map(|_| Substitute {
            command: dead.to_string(),
            instead: Act::shell("make test", MAKE_TEST_OK, 0),
            route: "dead-end-",
            context: format!(
                "adage ran make test for you; {dead} was replaced because that command fails in this repository (\""
            ),
        })
        .collect()
}

/// The substitutions of [`replay`] in a checkout where nothing was learned but the repository's
/// declared instincts were bound (`adage condition --instincts-only`): the sandbox's dead end is
/// a declared instinct before it is anything learned (its Makefile's `test` target sets
/// `ACME_ENV`, which the bare runner lacks), and the mod replaces it as it replaces a learned one,
/// the line giving the declared reason instead of a failure. The dead-end turn, and the same call
/// as run 3's agents piped it, become `make test`.
pub fn instinct_substitutions(r: &Replay) -> Vec<Substitute> {
    let (dead, ..) = DEAD_ENDS[0];
    let declared = |shown: &str, through: &str| {
        format!(
            "adage ran make test for you{through}; {shown} was replaced because this repository's Makefile declares \
             make test in its place ("
        )
    };
    let mut out: Vec<Substitute> =
        substitutions(r).into_iter().map(|s| Substitute { context: declared(&s.command, ""), ..s }).collect();
    out.push(Substitute {
        command: format!("{dead} -t . 2>&1 | tail -5"),
        instead: Act::shell("make test 2>&1 | tail -5", MAKE_TEST_OK, 0),
        route: "dead-end-",
        context: declared(&format!("{dead} -t ."), ", through the rest of your pipeline as written"),
    });
    out
}

// ---------------------------------------------------------------------------------------------
// Time

/// `YYYY-MM-DDTHH:MM:SS.mmmZ` for a Unix time in milliseconds.
pub fn iso(ms: i64) -> String {
    let (y, m, d) = civil(ms.div_euclid(86_400_000));
    let r = ms.rem_euclid(86_400_000);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", r / 3_600_000, r / 60_000 % 60, r / 1000 % 60, r % 1000)
}

/// (year, month, day) of a day count since 1970-01-01.
pub fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// A random (v4-shaped) UUID.
pub fn uuid4(rng: &mut Rng) -> String {
    let h = rng.hex(32);
    format!("{}-{}-4{}-a{}-{}", &h[0..8], &h[8..12], &h[13..16], &h[17..20], &h[20..32])
}

/// A time-ordered (v7-shaped) UUID, as Codex uses for thread ids.
pub fn uuid7(rng: &mut Rng, ms: i64) -> String {
    let t = format!("{:012x}", ms);
    let h = rng.hex(20);
    format!("{}-{}-7{}-a{}-{}", &t[0..8], &t[8..12], &h[0..3], &h[3..6], &h[6..18])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dead_end_turn_is_the_one_substitution() {
        let r = replay();
        let s = substitutions(&r);
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].command.as_str(), s[0].route), (DEAD_ENDS[0].0, "dead-end-"));
        assert!(matches!(&s[0].instead, Act::Shell { cmd, exit: 0, .. } if cmd == "make test"));
    }

    /// The same dead-end turn when only the declared instincts are bound: substituted too, with
    /// the declared reason, and run 3's piped form of it beside it.
    #[test]
    fn the_instinct_dead_end_is_substituted_plain_and_piped() {
        let s = instinct_substitutions(&replay());
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].command, DEAD_ENDS[0].0);
        assert!(s[0].context.ends_with("declares make test in its place ("), "{}", s[0].context);
        assert!(s[1].command.ends_with("2>&1 | tail -5"));
        assert!(matches!(&s[1].instead, Act::Shell { cmd, .. } if cmd == "make test 2>&1 | tail -5"));
        assert!(s.iter().all(|s| !s.context.contains("fails in this repository")));
    }

    /// The plan's sub-support habit: in 2 of the 12 sessions (the first two work items) `make lint`
    /// comes right after the last `make test`, and nowhere else.
    #[test]
    fn two_of_twelve_sessions_run_make_lint_right_after_make_test() {
        for seed in [crate::CLAUDE_SEED, crate::OPENCODE_SEED, crate::CODEX_SEED] {
            let h = history(seed);
            let cmd = |t: &Turn| match &t.calls[0].act {
                Act::Shell { cmd, .. } => cmd.clone(),
                _ => String::new(),
            };
            let lints: Vec<usize> = h
                .iter()
                .enumerate()
                .filter(|(_, s)| s.turns.iter().any(|t| cmd(t) == "make lint"))
                .map(|(i, _)| i)
                .collect();
            assert_eq!(lints, vec![0, 1], "seed {seed}");
            for s in &h[..LINT_SESSIONS] {
                let n = s.turns.len();
                assert_eq!((cmd(&s.turns[n - 2]), cmd(&s.turns[n - 1])), ("make test".into(), "make lint".into()));
            }
        }
    }

    /// The held-out session's predicted steps stand for the agent's own turns: the lookup before
    /// the model's first request (the agent's turn after the look around), the reads it names at
    /// the very next step, and `make gen` at the turn's end.
    #[test]
    fn the_replays_predictions_stand_for_the_agents_turns() {
        let r = replay();
        let p = &r.predict;
        assert_eq!(
            p.iter().map(|x| (x.index, x.ending, x.kind)).collect::<Vec<_>>(),
            vec![(0, false, "run"), (1, false, "chain"), (r.work.len() as u32, true, "obligation")]
        );
        for x in &p[..2] {
            let t = &r.work[x.index as usize + 1];
            assert!(t.calls.iter().map(|c| &c.act).eq(x.acts.iter()), "{x:?}");
        }
        assert!(matches!(&p[1].acts[..], [Act::Read { .. }, Act::Read { .. }]), "one response of two reads");
        assert_eq!(r.noticed, vec![Notice { step: "make lint", at: "make test" }]);
    }

    #[test]
    fn deterministic_and_varied() {
        let a = history(1);
        let b = history(1);
        assert_eq!(
            a.iter().map(|s| (&s.prompt, s.turns.len())).collect::<Vec<_>>(),
            b.iter().map(|s| (&s.prompt, s.turns.len())).collect::<Vec<_>>()
        );
        let c = history(2);
        assert_ne!(a.iter().map(|s| &s.prompt).collect::<Vec<_>>(), c.iter().map(|s| &s.prompt).collect::<Vec<_>>());
        assert_eq!(a.len(), 12);
    }

    #[test]
    fn times() {
        assert_eq!(iso(EPOCH_MS), "2026-09-01T00:00:00.000Z");
        assert_eq!(iso(EPOCH_MS + 3_723_004), "2026-09-01T01:02:03.004Z");
        assert_eq!(civil(0), (1970, 1, 1));
    }
}
