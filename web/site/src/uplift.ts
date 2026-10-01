// The uplift calculator on /how: how often agents take a step on their own, how often when told,
// and whether the difference is worth saying. The bar is UPLIFT_MIN in crates/adage-tracks/src/params.rs.

const UPLIFT_MIN = 0.1;

export function initUplift() {
  const self = document.getElementById("us") as HTMLInputElement | null;
  const follow = document.getElementById("uf") as HTMLInputElement | null;
  if (!self || !follow) return;
  const so = document.getElementById("us-o")!, fo = document.getElementById("uf-o")!;
  const bself = document.getElementById("u-self")!, bgain = document.getElementById("u-gain")!, say = document.getElementById("u-say")!;
  const update = () => {
    const s = +self.value / 100, f = +follow.value / 100, u = f - s;
    so.textContent = `${self.value}%`;
    fo.textContent = `${follow.value}%`;
    bself.style.width = `${s * 100}%`;
    bgain.style.left = `${Math.min(s, f) * 100}%`;
    bgain.style.width = `${Math.abs(u) * 100}%`;
    bgain.classList.toggle("neg", u < 0);
    const pts = Math.round(Math.abs(u) * 100);
    say.textContent =
      u >= UPLIFT_MIN ? `Telling adds ${pts} points. Worth saying: the reflex is delivered.`
      : u >= 0 ? `Telling adds only ${pts} point${pts === 1 ? "" : "s"}, under the 10-point bar. Agents do this anyway, so the reflex stays quiet until a person approves it.`
      : `Telling makes it ${pts} point${pts === 1 ? "" : "s"} less likely. The reflex stays quiet.`;
  };
  self.addEventListener("input", update);
  follow.addEventListener("input", update);
  update();
}
