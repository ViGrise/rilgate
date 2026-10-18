async function main() {
  if (!window.PowClient?.ready) {
    for (const path of [
      "challenge/runtime.js", "challenge/registry.js", "challenge/p2pool.js",
      "solvers/fast/solver.js", "solvers/fast/adapter.js", "solvers/randomx/adapter.js",
    ]) {
      await import(`${window.POW_INTERNAL_PREFIX}/static/js/${path}`);
    }
    window.PowClient.ready = true;
  }
  const { t, status, jsonFetch, endpoint, progressEl } = window.PowClient;
  const config = await jsonFetch(endpoint("/api/client-config"));
  const algorithm = window.POW_ALGORITHM;
  if (config.algorithms && !config.algorithms.some((entry) => entry.id === algorithm)) {
    throw new Error(`PoW algorithm is not enabled: ${algorithm}`);
  }
  await window.PowClient.algorithm(algorithm).run(config);
  progressEl.value = 1;
  progressEl.max = 1;
  status(t("challenge.success", "Verification successful. Continuing…"));
  if (window.POW_EMBEDDED) {
    const message = { type: "rilgate:verified" };
    if (window.parent !== window) window.parent.postMessage(message, location.origin);
    if (window.opener) window.opener.postMessage(message, location.origin);
  } else {
    location.reload();
  }
}

main().catch((error) => {
  console.error(error);
  document.querySelector("#progress").removeAttribute("value");
  document.querySelector("#status").textContent = window.PowClient
    ? window.PowClient.t("challenge.failed", "Challenge failed: {message}", { message: error.message })
    : `Challenge failed: ${error.message}`;
  const retry = document.querySelector("#retry");
  retry.hidden = false;
  retry.addEventListener("click", () => location.reload(), { once: true });
});
