// The saved theme (system, night or day), applied before first paint. The map sets it later.
try {
  var t = localStorage.getItem("adage-map-theme");
  if (t === "night" || t === "day") document.documentElement.setAttribute("data-theme", t);
} catch (e) {}
