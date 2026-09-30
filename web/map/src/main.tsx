import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { applyTheme, useTheme } from "./state/theme";
import "./styles.css";

// Screenshots and tests turn motion off with ?motion=off (the OS setting is honoured anyway).
if (new URLSearchParams(location.search).get("motion") === "off") {
  document.documentElement.setAttribute("data-motion", "off");
}

function Root() {
  applyTheme(useTheme());
  return <App />;
}

const el = document.getElementById("root");
if (el) {
  createRoot(el).render(
    <StrictMode>
      <Root />
    </StrictMode>,
  );
}
