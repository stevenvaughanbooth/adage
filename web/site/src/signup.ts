// Ask for an address only after the server says registration is configured.
export function initSignup() {
  const form = document.getElementById("signup") as HTMLFormElement | null;
  if (!form) return;
  const fields = document.getElementById("signup-fields") as HTMLFieldSetElement;
  const status = document.getElementById("signup-status")!;
  const retry = document.getElementById("signup-retry") as HTMLButtonElement;
  const email = form.elements.namedItem("email") as HTMLInputElement;
  const submit = form.querySelector<HTMLButtonElement>('button[type="submit"]')!;
  let available = false;
  let busy = false;
  const say = (message: string, kind: "" | "ok" | "err" = "") => {
    status.textContent = message;
    status.className = "status " + kind;
  };
  const setAvailable = (value: boolean) => {
    available = value;
    fields.hidden = !value;
    fields.disabled = !value;
    retry.hidden = value;
  };
  const setBusy = (value: boolean) => {
    busy = value;
    form.setAttribute("aria-busy", String(value));
    submit.disabled = value;
    retry.disabled = value;
  };
  const closed = () => say("Sign-ups aren't open yet. Explore the interactive walkthrough below while you wait.");

  async function check(focus = false) {
    if (busy) return;
    setBusy(true);
    say("Checking registration availability…");
    const controller = new AbortController();
    const timeout = window.setTimeout(() => controller.abort(), 10_000);
    try {
      const response = await fetch(form!.action, { cache: "no-store", signal: controller.signal });
      if (!response.ok) throw new Error("Availability check failed");
      const data: unknown = await response.json();
      if (!data || typeof data !== "object" || !("available" in data) || typeof data.available !== "boolean") {
        throw new Error("Invalid availability response");
      }
      setAvailable(data.available);
      if (available) say(""); else closed();
    } catch {
      setAvailable(false);
      say("Couldn't check registration. Try again, or explore the walkthrough below.", "err");
    } finally {
      clearTimeout(timeout);
      setBusy(false);
      if (focus) (available ? email : retry).focus();
    }
  }
  retry.addEventListener("click", () => { void check(true); });
  void check();

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (!available || busy) return;
    if (!email.value.trim() || !email.validity.valid) {
      say("Enter an email address, like you@company.com.", "err");
      email.focus();
      return;
    }
    setBusy(true);
    say("Adding you…");
    const controller = new AbortController();
    const timeout = window.setTimeout(() => controller.abort(), 10_000);
    try {
      const response = await fetch(form.action, {
        method: "POST",
        headers: { "content-type": "application/json" },
        signal: controller.signal,
        body: JSON.stringify({ email: email.value.trim(), hp_x7: (form.elements.namedItem("hp_x7") as HTMLInputElement).value }),
      });
      if (response.ok) { form.reset(); say("You're on the list. We'll email you once, when the first release is ready.", "ok"); }
      else if (response.status === 400 || response.status === 413) say("That address doesn't look right. Check it and try again.", "err");
      else if (response.status === 503) { setAvailable(false); closed(); }
      else say("Something went wrong on our side. Please try again in a minute.", "err");
    } catch {
      say(controller.signal.aborted
        ? "The request timed out. Your signup may have arrived; you can retry with the address still here."
        : "Couldn't reach the server. Check your connection and try again.", "err");
    } finally {
      clearTimeout(timeout);
      setBusy(false);
    }
  });
}
