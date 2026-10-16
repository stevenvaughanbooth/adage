//! The `Stop` arm of [`crate::decide`]: the Stop gate asks the agent to continue for the first
//! unmet finish-line obligation not yet asked for, within the session's bound on requests.

use crate::{render, Arm, Ctx, Decision, SessionState};

pub(super) fn on_stop(ctx: &Ctx, st: &mut SessionState, d: &mut Decision, stop_hook_active: bool) {
    if !stop_hook_active && st.stop_requests < ctx.params.max_stop_requests {
        if let Some(o) = st.obligations.iter_mut().find(|o| !o.met && !o.stop_requested) {
            o.stop_requested = true;
            st.stop_requests += 1;
            let reason = render::stop_reason(&o.paths, &o.command);
            d.would_continue = Some(reason.clone());
            if st.arm != Some(Arm::Control) {
                d.continue_reason = Some(reason);
            }
        }
    }
}
