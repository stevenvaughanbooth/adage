// The Wilson calculator on /how: move k and n, see the rate, its 95% lower bound and the fire bar.
// The bound is the same formula as crates/adage-tracks/src/model.rs `wilson_lower`.

const FIRE = 0.8, MIN_SUPPORT = 5;

export function wilson(k: number, n: number, z = 1.959964): number {
  if (!n) return 0;
  const p = k / n, d = 1 + (z * z) / n;
  return (p + (z * z) / (2 * n) - z * Math.sqrt((p * (1 - p)) / n + (z * z) / (4 * n * n))) / d;
}

/** The smallest n at which k = n (every trace took the path) clears the fire bar. */
function tracesToFire(): number {
  let n = 1;
  while (wilson(n, n) < FIRE) n++;
  return n;
}

export function initWilson() {
  const k = document.getElementById("wk") as HTMLInputElement | null;
  const n = document.getElementById("wn") as HTMLInputElement | null;
  if (!k || !n) return;
  const ko = document.getElementById("wk-o")!, no = document.getElementById("wn-o")!;
  const rate = document.getElementById("w-rate")!, lb = document.getElementById("w-lb")!, say = document.getElementById("w-say")!;
  const need = tracesToFire();
  const update = (from: "k" | "n") => {
    let kv = +k.value, nv = +n.value;
    if (kv > nv) { if (from === "k") { nv = kv; n.value = String(nv); } else { kv = nv; k.value = String(kv); } }
    ko.textContent = String(kv); no.textContent = String(nv);
    const r = kv / nv, b = wilson(kv, nv);
    rate.style.width = `${r * 100}%`;
    lb.style.left = `${b * 100}%`;
    const fires = b >= FIRE && kv >= MIN_SUPPORT;
    say.textContent =
      `Rate ${r.toFixed(2)}, lower bound ${b.toFixed(2)}. ` +
      (fires ? "It clears the 0.80 fire bar with enough support, so it may fire."
        : kv < MIN_SUPPORT ? `Under the minimum support of ${MIN_SUPPORT} traces, so no reflex forms yet.`
        : r >= 0.5 ? "Under the fire bar: it primes, and firms up as traces arrive."
        : "Too rare to prime.") +
      ` Even a perfect record needs ${need} traces to fire.`;
  };
  k.addEventListener("input", () => update("k"));
  n.addEventListener("input", () => update("n"));
  update("n");
}
