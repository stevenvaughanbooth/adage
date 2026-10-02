//! `adage savings`: the estimate (`adage_app::savings`, docs/zero-config.md §4.3) in the output
//! grammar.

pub use adage_app::savings::*;
use adage_learn::ledger::Ledger;
use adage_store::Paths;

/// The verdict's mark (docs/design/style.md, savings): `saved` a `✓` in the fire role (success),
/// `adds cost` a `✗` in the inhibit role (failure), measuring and no difference a `○` in ink 3.
/// The amounts stay in the default ink; under `NO_COLOR` the verdict is the words alone.
pub fn mark(v: Verdict) -> (adage_term::Glyph, adage_term::Role) {
    use adage_term::{Glyph, Role};
    match v {
        Verdict::Saved => (Glyph::Ok, Role::Fire),
        Verdict::AddsCost => (Glyph::Fail, Role::Inhibit),
        Verdict::Measuring | Verdict::NoDifference => (Glyph::Off, Role::Ink3),
    }
}

/// `adage savings`, in the output grammar: the estimate, each outcome's interval, the guardrail,
/// and the counts, then what to do next.
pub fn render<W: std::io::Write>(o: &mut adage_term::Out<W>, s: &Summary) -> std::io::Result<()> {
    use adage_term::Line;
    o.header("savings", &[&s.repo, verdict_word(s.verdict)])?;
    o.blank()?;
    let (g, role) = mark(s.verdict);
    let lead = if o.theme.color() { Line::new().glyph(role, g).text(" ") } else { Line::new() };
    o.kv("estimate", lead.text(brief(s)))?;
    if !s.gate_met {
        o.kv(
            "gate",
            Line::new().quiet(format!(
                "dollars after {NEED_CONTROL} control and {NEED_TREATED} treated traces; Adage stays quiet in 1 of 10 traces so its effect can be measured"
            )),
        )?;
    }
    // In the direction the words say: `20.1 more model turns (95% CI 20.0–20.2 …)`.
    let outcome = |e: &Estimate, what: &str| {
        let (lo, hi) = if e.delta >= 0.0 { (e.lo, e.hi) } else { (-e.hi, -e.lo) };
        Line::new().text(format!("{:.1} {what}", e.delta.abs())).quiet(format!(
            " (95% CI {lo:.1}–{hi:.1}; {:.1} with Adage, {:.1} without)",
            e.mean_treated, e.mean_control
        ))
    };
    if let Some(t) = &s.model_turns {
        o.kv("turns", outcome(t, if t.delta >= 0.0 { "fewer model turns" } else { "more model turns" }))?;
    }
    if let Some(t) = &s.turns_to_first_edit {
        o.kv("first edit", outcome(t, if t.delta >= 0.0 { "turns sooner" } else { "turns later" }))?;
    }
    if let (Verdict::Saved, Some(c)) = (s.verdict, &s.cost_usd) {
        o.kv(
            "over",
            Line::new()
                .text(format!("≈ {} over {}", usd(c.delta * s.n_treated as f64), traces(s.n_treated)))
                .sep()
                .quiet(format!("priced as of {}", s.pricing_as_of)),
        )?;
    } else if s.gate_met {
        o.kv("priced", Line::new().quiet(format!("API-list prices as of {}", s.pricing_as_of)))?;
    }
    if let Some(g) = guardrail(s) {
        o.kv("help", Line::new().quiet(g))?;
    }
    if s.unpriced > 0 {
        o.kv(
            "unpriced",
            Line::new().quiet(format!("{} on a model with no listed price: turns only", traces(s.unpriced))),
        )?;
    }
    let c = &s.counts;
    let n = |k: usize, one: &str, many: &str| format!("{k} {}", if k == 1 { one } else { many });
    let mut counts = Line::new()
        .text(n(c.sessions, "trace", "traces"))
        .sep()
        .text(n(c.directives, "directive", "directives"))
        .sep()
        .text(format!("{} followed", c.followed))
        .sep()
        .text(format!("{} headed off", n(c.dead_ends_headed_off, "dead end", "dead ends")))
        .sep()
        .text(format!("{} met", n(c.finish_lines_met, "finish line", "finish lines")));
    if c.rewritten > 0 {
        counts = counts.sep().text(n(c.rewritten, "rewrite", "rewrites"));
    }
    o.kv("recent", counts.faint(format!(" (last {} days)", c.days)))?;
    o.blank()?;
    match s.verdict {
        Verdict::AddsCost => o.next("adage review", "weaken or extinguish what does not help"),
        Verdict::Measuring => o.next("adage watch", "see each trace as it happens"),
        _ => o.next("adage savings --json", "every figure, with the caps and counts"),
    }
}

/// `adage savings [--repo <path>] [--json]`: the estimate for this repository, computed now.
pub fn run(args: &[String], paths: &Paths) -> anyhow::Result<()> {
    let dir = match crate::flag(args, "--repo") {
        Some(d) => std::path::PathBuf::from(d),
        None => std::env::current_dir()?,
    };
    let root = adage_core::repo::find_root(&dir).unwrap_or(dir);
    let index = adage_tracks::Index::load(&paths.index());
    let repo = crate::ui::health::repo_key(paths, &root, &index);
    let ledger = Ledger::new(&paths.ledger(), &repo).read();
    let s = refresh(paths, &repo, &ledger);
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string_pretty(&s)?);
        return Ok(());
    }
    render(&mut adage_term::Out::stdout(), &s)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(i: usize, control: bool, cost: f64) -> Unit {
        Unit {
            id: format!("s{i:03}"),
            control,
            propensity: if control { 0.1 } else { 0.9 },
            cost: Some(cost),
            turns: cost * 100.0,
            first_edit: None,
            asked_for_help: false,
        }
    }

    fn rendered(s: &Summary, theme: adage_term::Theme) -> String {
        let mut o = adage_term::Out::raw(Vec::new(), theme);
        render(&mut o, s).unwrap();
        String::from_utf8(o.into_inner()).unwrap()
    }

    fn counts() -> Counts {
        Counts {
            days: 30,
            sessions: 61,
            directives: 188,
            followed: 152,
            dead_ends_headed_off: 7,
            finish_lines_met: 3,
            rewritten: 2,
            since: 0,
        }
    }

    /// `adage savings` in the output grammar, at each verdict: below the gate no dollars (only
    /// what is being counted); past it the figure with its interval, the guardrail, the counts.
    #[test]
    fn savings_output_at_each_verdict() {
        let below: Vec<Unit> = (0..29).map(|i| unit(i, i < 4, 0.1)).collect();
        let s = summarise("github.com/acme/billing", &below, counts(), 0);
        assert_eq!(brief(&s), "measuring: 4 control / 25 treated traces");
        let text = rendered(&s, adage_term::Theme::plain());
        assert!(!text.contains('$'), "{text}");
        insta::assert_snapshot!("savings-measuring", text);

        // Control traces cost more: a saving.
        let saved: Vec<Unit> = (0..52)
            .map(|i| {
                unit(i, i >= 40, if i >= 40 { 0.3 + (i % 3) as f64 * 0.001 } else { 0.1 + (i % 5) as f64 * 0.001 })
            })
            .collect();
        let mut s = summarise("github.com/acme/billing", &saved, counts(), 0);
        s.pricing_as_of = "2026-06-24".into();
        assert_eq!(s.verdict, Verdict::Saved);
        assert!(brief(&s).starts_with("saved ~$0.199 per trace, API-list equivalent (95% CI $"), "{}", brief(&s));
        insta::assert_snapshot!("savings-saved", rendered(&s, adage_term::Theme::plain()));
        // Without colour (pipes, NO_COLOR) the verdict is the words alone; in colour it carries
        // its mark, a ✓ in the success role.
        let plain = rendered(&s, adage_term::Theme::plain());
        assert!(plain.contains("estimate    saved ~$0.199") && !plain.contains('✓'), "{plain}");
        let caps = adage_term::Caps { level: adage_term::Level::Ansi16, ..adage_term::Caps::plain() };
        let colour = rendered(&s, adage_term::Theme::new(caps));
        assert!(colour.contains("✓ saved ~$0.199"), "{colour:?}");

        let adds: Vec<Unit> = (0..52)
            .map(|i| {
                unit(i, i >= 40, if i >= 40 { 0.1 + (i % 3) as f64 * 0.001 } else { 0.3 + (i % 5) as f64 * 0.001 })
            })
            .collect();
        let mut s = summarise("github.com/acme/billing", &adds, counts(), 0);
        s.pricing_as_of = "2026-06-24".into();
        assert!(brief(&s).starts_with("adds ~$0.20"), "{}", brief(&s));
        insta::assert_snapshot!("savings-adds-cost", rendered(&s, adage_term::Theme::plain()));
    }
}
