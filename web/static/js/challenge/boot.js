(() => {
  if (window.POW_BOOTED === true) {
    const error = new Error("Challenge script is already running");
    error.name = "PowDuplicateLoad";
    throw error;
  }
  window.POW_BOOTED = true;

  function absoluteUrl(value) {
    const anchor = document.createElement("a");
    anchor.href = value;
    return anchor.href;
  }

  // The static base comes from the page bootstrap, or from the bundle tag when the
  // bootstrap is unreadable.
  function staticBase() {
    let base = window.POW_STATIC_URL;
    if (!base) {
      try {
        base = JSON.parse(document.querySelector("#pow-bootstrap").textContent).static_url;
      } catch (_) {
        base = "";
      }
    }
    if (!base) {
      const tag = document.getElementById("challenge-bundle");
      const source = tag && tag.getAttribute("src");
      if (!source) return "";
      base = source.replace(/\/js\/[^/]*$/, "");
    }
    return absoluteUrl(base);
  }

  function isDuplicateLoad(event) {
    return (
      (event.error && event.error.name === "PowDuplicateLoad") ||
      /Challenge script is already running/.test(event.message || "")
    );
  }

  window.addEventListener("error", (event) => {
    const base = staticBase();
    if (!base || String(event.filename || "").indexOf(base) !== 0) return;
    if (isDuplicateLoad(event)) return;
    const client = window.PowClient;
    if (client) {
      if (client.currentState() !== "success") client.state("failed");
    } else {
      document.documentElement.className += " pow-load-failed";
    }
  });
})();
