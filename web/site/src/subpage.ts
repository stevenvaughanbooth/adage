// The pages with one subject each: /packs, /get-started and /early-access. Each part starts only
// where its page has it: the pack explorer and the early-access form.

import "@fontsource-variable/bricolage-grotesque/wght.css";
import "@fontsource-variable/hanken-grotesk/wght.css";
import "@fontsource-variable/red-hat-mono/wght.css";
import "./styles.css";
import { initPacks } from "./packs";
import { initPage } from "./page";
import { initSignup } from "./signup";
import { initTheme } from "./theme";

initTheme();
initPage();
initPacks();
initSignup();
