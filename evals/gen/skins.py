"""Domain skins: the vocabulary a generated repository is written in.

A skin names the repository, its package, modules and tools, the tracker (its ID prefix, where its
data hides, the lookup tool), the registry tables behind the finish line, and gives every bug
template (bugs.py) a function name and a noun phrase. Stacks never hard-code domain words; they
read them from here, so any skin works with any stack.

Adding a skin: add an entry to SKINS with every key below. Keep every text free of path-like tokens
(no decimals, no "e.g.", no file names): `adage condition` reads paths from plain-text lookup output.
"""

from dataclasses import dataclass, field


@dataclass(frozen=True)
class Table:
    name: str          # registry/<name>.csv
    noun: str          # reader function name (python), e.g. `currency`
    columns: tuple     # first column is always `code`
    rows: tuple        # tuples of strings, in file order


@dataclass(frozen=True)
class RegistryTask:
    table: str
    op: str            # "add" | "change"
    code: str
    values: dict       # add: every non-code column; change: the changed column(s)
    title: str
    description: str


@dataclass(frozen=True)
class Skin:
    name: str
    repo: str
    pkg: str
    scope: str                      # node path alias prefix, e.g. "@tallyhouse/billing"
    description: str
    tracker_phrase: str
    id_prefix: str
    id_digits: tuple                # default (min, max) digits of item numbers
    tool: str                       # lookup tool, repository-relative
    store: str                      # opaque tracker store
    magic: str                      # 4 ASCII bytes heading the store
    env: str                        # env_guard variable
    modules: tuple                  # three module names
    registry_module: str
    fields: dict                    # template id -> record field
    lookup_const: str
    lookup_table: dict              # ci_lookup constant: code -> int
    lookup_lc: str                  # a key of lookup_table, as typed in lower case
    noun: str                       # require_name noun
    functions: dict                 # template id -> (snake_case name, noun phrase)
    tables: tuple
    registry_tasks: tuple
    prompts: tuple                  # templates with {id}
    areas: tuple
    reporters: tuple
    decoy_items: tuple              # (title, status, description) for closed tracker items
    decoys: dict = field(default_factory=dict)  # decoy mode -> name used for the target/script


BILLING = Skin(
    name="billing",
    repo="tallyhouse-billing",
    pkg="tally",
    scope="@tallyhouse/billing",
    description="Invoicing, customer accounts and payment runs for Tallyhouse.",
    tracker_phrase="the internal tracker",
    id_prefix="BIL",
    id_digits=(3, 4),
    tool="scripts/wi-lookup",
    store=".tracker/cache.bin",
    magic="TLY1",
    env="TALLY_ENV",
    modules=("invoices", "accounts", "payments"),
    registry_module="catalog",
    fields={"sum_all": "units", "sort_desc": "balance"},
    lookup_const="PLAN_PRICES",
    lookup_table={"BASIC": 900, "PRO": 2900, "TEAM": 7900},
    lookup_lc="pro",
    noun="customer name",
    functions={
        "pct_round": ("discount_amount", "invoice discounts"),
        "after_boundary": ("is_overdue", "overdue invoices"),
        "ci_lookup": ("plan_price", "plan price"),
        "clamp_hi": ("clamp_seats", "seat counts"),
        "sum_all": ("total_units", "invoice unit"),
        "avg_empty": ("average_invoice", "average invoice"),
        "require_name": ("display_name", "customer name"),
        "sort_desc": ("top_accounts", "top accounts"),
        "unit_convert": ("grace_seconds", "payment grace windows"),
        "dedupe_order": ("unique_invoice_codes", "invoice code lists"),
        "inclusive_range": ("billing_days", "billing periods"),
        "percent_valid": ("discount_fraction", "discount percentages"),
    },
    tables=(
        Table("currencies", "currency", ("code", "name", "minor_units"),
              (("CHF", "Swiss Franc", "2"), ("EUR", "Euro", "2"), ("GBP", "Pound Sterling", "2"),
               ("JPY", "Japanese Yen", "0"), ("USD", "US Dollar", "2"))),
        Table("regions", "region", ("code", "name", "tax_bp"),
              (("APAC", "Asia Pacific", "1000"), ("EU", "European Union", "2000"), ("UK", "United Kingdom", "2000"),
               ("US", "United States", "800"))),
    ),
    registry_tasks=(
        RegistryTask("currencies", "add", "KWD", {"name": "Kuwaiti Dinar", "minor_units": "3"},
                     "Add the Kuwaiti dinar",
                     "Invoices for our new Kuwait customers cannot be issued: the currency KWD is unknown. Add it "
                     "as Kuwaiti Dinar with 3 minor units."),
        RegistryTask("currencies", "add", "SEK", {"name": "Swedish Krona", "minor_units": "2"},
                     "Add the Swedish krona",
                     "Nordic accounts are moving to local billing. Add the currency SEK, Swedish Krona, with 2 minor "
                     "units."),
        RegistryTask("regions", "add", "LATAM", {"name": "Latin America", "tax_bp": "1600"},
                     "Add the Latin America tax region",
                     "Sales opened a Latin America region. Add region LATAM, named Latin America, taxed at 1600 "
                     "basis points."),
        RegistryTask("regions", "change", "UK", {"tax_bp": "2250"},
                     "UK tax rate change",
                     "The UK region is still taxed at 2000 basis points. From this quarter it must be 2250 basis "
                     "points."),
        RegistryTask("currencies", "change", "JPY", {"name": "Yen"},
                     "Rename the Japanese currency",
                     "Finance wants the JPY currency shown as just Yen on invoices, not Japanese Yen."),
        RegistryTask("regions", "add", "MEA", {"name": "Middle East and Africa", "tax_bp": "500"},
                     "Add the Middle East and Africa tax region",
                     "Add region MEA, named Middle East and Africa, taxed at 500 basis points."),
    ),
    prompts=("Pick up {id} and get it done.", "Please work on {id}.", "Can you take care of {id}?"),
    areas=("invoicing", "accounts", "payments", "catalog"),
    reporters=("finance-ops", "support", "sales-ops", "collections"),
    decoy_items=(
        ("Dunning emails sent twice", "closed", "Resolved by the mail provider; no code change was needed."),
        ("Export to the old ledger", "wontfix", "The old ledger was switched off, nothing to export to."),
        ("Invoice numbering gaps", "duplicate", "Same report as an earlier item that is already closed."),
        ("Slow account search", "closed", "Fixed by an index on the hosted database."),
    ),
    decoys={"unused_target": "bench", "lookalike_regen": "proto", "lookalike_verify": "lint",
            "legacy_script": "legacy-export"},
)

LOGISTICS = Skin(
    name="logistics",
    repo="portside-dispatch",
    pkg="portside",
    scope="@portside/dispatch",
    description="Berth planning, cargo handling and vessel schedules for the Portside terminal.",
    tracker_phrase="the ops queue",
    id_prefix="OPS",
    id_digits=(2, 3),
    tool="bin/fetch-ticket",
    store=".ops/queue.pack",
    magic="PSQ1",
    env="PORTSIDE_ENV",
    modules=("berths", "cargo", "schedule"),
    registry_module="registry_lookup",
    fields={"sum_all": "teu", "sort_desc": "priority"},
    lookup_const="CRANE_RATES",
    lookup_table={"GANTRY": 30, "MOBILE": 18, "RUBBER": 12},
    lookup_lc="mobile",
    noun="vessel name",
    functions={
        "pct_round": ("surcharge_amount", "demurrage surcharges"),
        "after_boundary": ("exceeds_draft", "draft checks"),
        "ci_lookup": ("crane_rate", "crane rate"),
        "clamp_hi": ("clamp_moves", "crane move counts"),
        "sum_all": ("total_teu", "manifest TEU"),
        "avg_empty": ("average_dwell", "average dwell time"),
        "require_name": ("vessel_label", "vessel name"),
        "sort_desc": ("urgent_first", "urgent cargo lists"),
        "unit_convert": ("pilot_buffer_seconds", "pilot buffers"),
        "dedupe_order": ("unique_port_calls", "port call lists"),
        "inclusive_range": ("berth_window_days", "berth windows"),
        "percent_valid": ("load_fraction", "load percentages"),
    },
    tables=(
        Table("ports", "port", ("code", "name", "max_draft_dm"),
              (("DEHAM", "Hamburg", "152"), ("NLRTM", "Rotterdam", "230"), ("SGSIN", "Singapore", "160"),
               ("USLAX", "Los Angeles", "160"))),
        Table("vessels", "vessel_class", ("code", "name", "teu"),
              (("FEEDER", "Feeder", "1000"), ("PANAMAX", "Panamax", "5000"), ("ULCV", "Ultra Large", "20000"))),
    ),
    registry_tasks=(
        RegistryTask("ports", "add", "PLGDN", {"name": "Gdansk", "max_draft_dm": "150"},
                     "Add the port of Gdansk",
                     "We start calling at Gdansk next month and the planner rejects it. Add port PLGDN, Gdansk, with "
                     "a maximum draft of 150 decimetres."),
        RegistryTask("ports", "change", "NLRTM", {"max_draft_dm": "240"},
                     "Rotterdam maximum draft is now 240 decimetres",
                     "The harbour master raised the Rotterdam limit. NLRTM must allow a maximum draft of 240 "
                     "decimetres."),
        RegistryTask("vessels", "add", "NEOPMX", {"name": "Neopanamax", "teu": "14500"},
                     "Add the Neopanamax vessel class",
                     "Add vessel class NEOPMX, named Neopanamax, with a capacity of 14500 TEU."),
        RegistryTask("vessels", "add", "FEEDMX", {"name": "Feedermax", "teu": "3000"},
                     "Add the Feedermax vessel class",
                     "Add vessel class FEEDMX, named Feedermax, with a capacity of 3000 TEU."),
        RegistryTask("ports", "add", "ESVLC", {"name": "Valencia", "max_draft_dm": "160"},
                     "Add the port of Valencia",
                     "Add port ESVLC, Valencia, with a maximum draft of 160 decimetres."),
        RegistryTask("vessels", "change", "PANAMAX", {"teu": "5100"},
                     "Panamax capacity is understated",
                     "Our Panamax class is recorded at 5000 TEU. The chartered ships carry 5100 TEU; update it."),
    ),
    prompts=("{id} is assigned to you. Please resolve it.", "Could you fix {id}?", "Take {id} through to done, please."),
    areas=("berths", "cargo", "schedule", "registry"),
    reporters=("berth-planning", "yard-planning", "vessel-operations", "harbour-master"),
    decoy_items=(
        ("Crane telemetry gaps", "closed", "Hardware fault on one crane, replaced by the vendor."),
        ("Legacy EDI feed", "wontfix", "The EDI partner moved to the new API, the feed is gone."),
        ("Berth plan printout", "duplicate", "Same request as an earlier ticket that is already closed."),
        ("Night shift roster", "closed", "Handled by the HR system, not this service."),
    ),
    decoys={"unused_target": "bench", "lookalike_regen": "proto", "lookalike_verify": "lint",
            "legacy_script": "legacy-sync"},
)

CLINIC = Skin(
    name="clinic",
    repo="wardline-clinic",
    pkg="wardline",
    scope="@wardline/core",
    description="Dosing checks, patient records and staff rotas for Wardline clinics.",
    tracker_phrase="the case system",
    id_prefix="CARE",
    id_digits=(3, 3),
    tool="tools/case",
    store=".cases/store.dat",
    magic="WLC1",
    env="WARDLINE_ENV",
    modules=("dosing", "patients", "rota"),
    registry_module="formulary",
    fields={"sum_all": "doses", "sort_desc": "acuity"},
    lookup_const="SHIFT_HOURS",
    lookup_table={"DAY": 8, "LONG": 12, "NIGHT": 10},
    lookup_lc="night",
    noun="patient name",
    functions={
        "pct_round": ("dose_reduction", "dose reductions"),
        "after_boundary": ("over_daily_limit", "daily limit checks"),
        "ci_lookup": ("shift_hours", "shift length"),
        "clamp_hi": ("clamp_infusion_rate", "infusion rates"),
        "sum_all": ("total_doses", "daily dose"),
        "avg_empty": ("average_wait", "average wait"),
        "require_name": ("patient_label", "patient name"),
        "sort_desc": ("triage_order", "triage lists"),
        "unit_convert": ("handover_seconds", "handover windows"),
        "dedupe_order": ("unique_bed_codes", "bed code lists"),
        "inclusive_range": ("admission_days", "admission periods"),
        "percent_valid": ("occupancy_fraction", "occupancy percentages"),
    },
    tables=(
        Table("wards", "ward", ("code", "name", "beds"),
              (("CARD", "Cardiology", "16"), ("ICU", "Intensive Care", "12"), ("MAT", "Maternity", "20"))),
        Table("drugs", "drug", ("code", "name", "max_daily_mg"),
              (("AMOX", "Amoxicillin", "3000"), ("IBU", "Ibuprofen", "1200"), ("PCM", "Paracetamol", "4000"))),
    ),
    registry_tasks=(
        RegistryTask("wards", "add", "ONC", {"name": "Oncology", "beds": "14"},
                     "Add the oncology ward",
                     "The new oncology ward opens on Monday. Add ward ONC, Oncology, with 14 beds."),
        RegistryTask("drugs", "change", "IBU", {"max_daily_mg": "2400"},
                     "Ibuprofen daily maximum is too low",
                     "Under the new protocol the ibuprofen daily maximum is 2400 mg, not 1200 mg. Update IBU."),
        RegistryTask("drugs", "add", "NAPX", {"name": "Naproxen", "max_daily_mg": "1000"},
                     "Add naproxen to the formulary",
                     "Add drug NAPX, Naproxen, with a daily maximum of 1000 mg."),
        RegistryTask("wards", "change", "ICU", {"beds": "18"},
                     "Intensive care bed count",
                     "Intensive care expanded. The ICU ward now has 18 beds."),
        RegistryTask("wards", "add", "PAED", {"name": "Paediatrics", "beds": "22"},
                     "Add the paediatrics ward",
                     "Add ward PAED, Paediatrics, with 22 beds."),
        RegistryTask("drugs", "add", "CEPH", {"name": "Cefalexin", "max_daily_mg": "4000"},
                     "Add cefalexin to the formulary",
                     "Add drug CEPH, Cefalexin, with a daily maximum of 4000 mg."),
    ),
    prompts=("Case {id} is yours, please handle it.", "Can you resolve {id}?", "Please close out {id}."),
    areas=("dosing", "patients", "rota", "formulary"),
    reporters=("pharmacy", "ward-sister", "clinical-safety", "bed-management"),
    decoy_items=(
        ("Pager integration flaky", "closed", "The paging vendor fixed an outage on their side."),
        ("Old appointment export", "wontfix", "Appointments moved to the regional system."),
        ("Duplicate allergy alert", "duplicate", "Same report as an earlier case that is already closed."),
        ("Rota printout margins", "closed", "Printer driver issue on one workstation."),
    ),
    decoys={"unused_target": "bench", "lookalike_regen": "proto", "lookalike_verify": "lint",
            "legacy_script": "legacy-import"},
)

SKINS = {s.name: s for s in (BILLING, LOGISTICS, CLINIC)}
