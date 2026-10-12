//! The repository scope Live, Reflexes and Map share: one repository at a time (the one the app
//! was opened in, else the most recently active one), or every repository. The scope itself is
//! [`super::view::Live`]'s (`all_repos`, `repo`), so `adage watch` and the app read it alike; this
//! module names repositories in plain words and lists the ones a person can switch to.
//!
//! A repository is known by its key (`adage_core::repo::repo_key`: the normalised `origin` URL,
//! else `<name>@<root commit>`), which is what spool records, `learn.log` lines and a track's
//! scope carry. People see the key's readable part: `github.com/acme/billing`, or `billing`.

use super::feed::FeedItem;
use super::routes::RouteRow;
use std::collections::BTreeMap;

/// A repository key in plain words: the `origin` URL as it is, the name of one without `origin`
/// (`billing@5ab83fcedeea` and `billing@path-…` are `billing`).
pub fn friendly(key: &str) -> String {
    if key.is_empty() {
        return "(no repository)".into();
    }
    match key.rsplit_once('@') {
        // `user@host` is not a key shape (origins are normalised without it), so an `@` with a
        // hex or `path-` suffix is the root-commit form.
        Some((name, tail))
            if !name.is_empty()
                && (tail.starts_with("path-") || (!tail.is_empty() && tail.chars().all(|c| c.is_ascii_hexdigit()))) =>
        {
            name.to_string()
        }
        _ => key.to_string(),
    }
}

/// The shortest readable name: the last part of [`friendly`] (`github.com/acme/billing` is
/// `billing`), for a column beside each feed row.
pub fn short(key: &str) -> String {
    let f = friendly(key);
    f.rsplit('/').next().unwrap_or(&f).to_string()
}

/// A repository a person can scope to.
#[derive(Debug, Clone, PartialEq)]
pub struct Known {
    pub key: String,
    /// [`friendly`], with the key's tail when two repositories would read alike.
    pub name: String,
    /// The repository the app was opened in.
    pub here: bool,
    /// The newest hook call or conditioning run seen for it (ms since the epoch).
    pub last: Option<u64>,
    /// Events in the live feed.
    pub events: usize,
    /// Reflexes installed for it.
    pub reflexes: usize,
}

/// Every repository the screens know of: the one opened here, those in the live feed and the
/// conditioning ticker, and those with reflexes; most recently active first, then by name.
pub fn known(
    feed: &[&FeedItem],
    ticks: &[(&str, u64)],
    rows: &[RouteRow],
    here: Option<&str>,
    current: Option<&str>,
) -> Vec<Known> {
    let mut by: BTreeMap<String, Known> = BTreeMap::new();
    fn entry<'a>(by: &'a mut BTreeMap<String, Known>, key: &str) -> Option<&'a mut Known> {
        if key.is_empty() {
            return None;
        }
        Some(by.entry(key.to_string()).or_insert_with(|| Known {
            key: key.to_string(),
            name: friendly(key),
            here: false,
            last: None,
            events: 0,
            reflexes: 0,
        }))
    }
    for i in feed {
        if let Some(k) = entry(&mut by, &i.repo) {
            k.events += 1;
            k.last = k.last.max(Some(i.ts));
        }
    }
    for (repo, ts) in ticks {
        if let Some(k) = entry(&mut by, repo) {
            k.last = k.last.max(Some(*ts));
        }
    }
    for r in rows {
        if let Some(k) = entry(&mut by, &r.repo) {
            k.reflexes += 1;
        }
    }
    for key in [here, current].into_iter().flatten() {
        entry(&mut by, key);
    }
    if let Some(k) = here.and_then(|h| by.get_mut(h)) {
        k.here = true;
    }
    let mut out: Vec<Known> = by.into_values().collect();
    // Two repositories that read alike (`app@1a2b…`, `app@9f8e…`) keep their keys' tails apart.
    let names: Vec<String> = out.iter().map(|k| k.name.clone()).collect();
    for k in &mut out {
        if names.iter().filter(|n| **n == k.name).count() > 1 && k.name != k.key {
            k.name = format!("{} ({})", k.name, k.key.rsplit_once('@').map_or("", |(_, t)| t));
        }
    }
    out.sort_by(|a, b| b.last.cmp(&a.last).then(b.here.cmp(&a.here)).then(a.name.cmp(&b.name)));
    out
}

/// A track's group in the Reflexes tab, in plain words: `Learned here`, `From pack · work-items`,
/// or the track's own name; with every repository shown, the repository too. The version is said
/// beside it; the track's directory (what `adage review` takes) only in its details.
pub fn track_title(r: &RouteRow, scoped: bool) -> String {
    collection(&r.track, &r.repo, r.imported, scoped)
}

/// [`track_title`] for a track named `track` of repository `repo`: the conditioning card names
/// the sets a run wrote the same way.
pub fn collection(track: &str, repo_key: &str, imported: bool, scoped: bool) -> String {
    let name = track.rsplit('/').next().unwrap_or(track);
    let repo = short(repo_key);
    let learned = name == adage_learn::TRACK && !imported;
    match (learned, imported, scoped || repo_key.is_empty()) {
        (true, _, true) => "Learned here".into(),
        (true, _, false) => format!("Learned in {repo}"),
        (false, true, true) => format!("From pack · {name}"),
        (false, true, false) => format!("From pack · {name} · {repo}"),
        (false, false, true) => name.to_string(),
        (false, false, false) => format!("{name} · {repo}"),
    }
}
