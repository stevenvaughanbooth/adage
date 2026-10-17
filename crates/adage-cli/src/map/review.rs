//! A terminal-owned map can review reflexes. This capability is not execution permission.
use super::http::{decode_component, json_response, text, Request, Response};
use super::Shared;
use adage_app::{person::Person, review};
use rand_core::{OsRng, RngCore};
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

pub(super) struct Access {
    person: Option<Person>,
    token: Option<String>,
    reason: Option<String>,
}

impl Access {
    pub(super) fn new(demo: bool) -> Self {
        if demo {
            return Self::read_only(
                "The demo is read-only. Open adage map from your terminal to review your own reflexes.",
            );
        }
        let person = match adage_app::person::require("review") {
            Ok(person) => person,
            Err(_) => return Self::read_only("This map is read-only. Run adage map from your own terminal to enable review; an agent-launched map cannot change trust."),
        };
        let mut bytes = [0u8; 32];
        if OsRng.try_fill_bytes(&mut bytes).is_err() {
            return Self::read_only("Review could not be enabled securely. Restart adage map from your terminal.");
        }
        Self { person: Some(person), token: Some(bytes.iter().map(|b| format!("{b:02x}")).collect()), reason: None }
    }
    pub(super) fn read_only(reason: &str) -> Self {
        Self { person: None, token: None, reason: Some(reason.into()) }
    }
    pub(super) fn enabled(&self) -> bool {
        self.person.is_some()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Decision {
    reference: String,
    reflex: String,
    verb: String,
    revision: String,
}

pub(super) fn respond(req: &Request, shared: &Shared) -> Response {
    let access = &shared.review;
    if req.method == "GET" {
        if req.query.is_empty() {
            return json_response(
                &json!({ "enabled": access.enabled(), "token": access.token, "reason": access.reason }),
            );
        }
        let value = |key: &str| {
            req.query
                .split('&')
                .filter_map(|v| v.split_once('='))
                .find(|(k, _)| *k == key)
                .map(|(_, v)| decode_component(&v.replace('+', " ")))
        };
        let (Some(reference), Some(reflex)) = (value("reference"), value("reflex")) else {
            return text("400 Bad Request", "reference and reflex are required");
        };
        return match review::standings(&shared.paths, shared.repo_root.as_deref()) {
            Ok(mut standings) => match standings.remove(&(reference, reflex)) {
                Some(standing) => match standing.preview {
                    Some(p) => {
                        json_response(&json!({ "revision": p.revision, "current_directive": p.current_directive,
                        "verbs": standing.verbs, "reasons": standing.reasons, "proposal": p.proposal.map(|p| json!({ "on": p.on,
                            "reason": p.reason, "diff": p.diff, "directive": p.directive })) }))
                    }
                    None => text(
                        "409 Conflict",
                        "The proposal cannot be inspected safely. Reload after conditioning completes.",
                    ),
                },
                None => text("404 Not Found", "This installed reflex is no longer available."),
            },
            Err(e) => text("503 Service Unavailable", &format!("{e:#}")),
        };
    }
    if req.method != "POST" {
        return text("405 Method Not Allowed", "review supports GET and POST only");
    }
    let Some(person) = &access.person else {
        return text("403 Forbidden", access.reason.as_deref().unwrap_or("Review is disabled."));
    };
    // Exact origin, JSON, and an unguessable per-server capability. No CORS response headers.
    let origin = format!("http://{}", req.host.as_deref().unwrap_or_default());
    if req.origin.as_deref() != Some(&origin) || req.csrf.as_ref() != access.token.as_ref() {
        return text("403 Forbidden", "Review requires this map's origin and review token.");
    }
    if req.content_type.as_deref() != Some("application/json") {
        return text("415 Unsupported Media Type", "Review requires application/json.");
    }
    let Ok(d) = serde_json::from_slice::<Decision>(&req.body) else {
        return text("400 Bad Request", "Invalid review decision.");
    };
    let Some(verb) = review::Verb::parse(&d.verb) else {
        return text("400 Bad Request", "Unknown review decision. Execution grants are not available here.");
    };
    if d.revision.len() != 64 || !d.revision.bytes().all(|b| b.is_ascii_hexdigit()) {
        return text("400 Bad Request", "An inspected revision is required.");
    }
    match review::decide_reviewed_within(
        person,
        verb,
        &d.reference,
        &d.reflex,
        &d.revision,
        &shared.paths,
        Duration::from_millis(250),
    ) {
        Ok(o) => json_response(
            &json!({ "saved": true, "verb": o.verb.as_str(), "reflex": o.route, "reference": o.reference, "repo": o.repo, "warnings": o.warnings,
            "state": o.after.state, "strength": o.after.strength, "recorded": o.recorded,
            "proposal_applied": o.proposal_applied, "already": o.already.is_some(), "pack_approved": o.pack_approved }),
        ),
        Err(e) => text("409 Conflict", &format!("{e:#}")),
    }
}
