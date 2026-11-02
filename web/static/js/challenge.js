function markLoadFailed() {
  document.documentElement.className += " pow-load-failed";
}

async function main() {
  if (!window.PowClient) {
    markLoadFailed();
    return;
  }
  const { state, jsonFetch, endpoint } = window.PowClient;
  window.PowClient.once(document.querySelector("#retry"), "click", () => location.reload());
  if (navigator.cookieEnabled === false) {
    state("cookies");
    return;
  }
  state("preparing");
  const config = await jsonFetch(endpoint("/api/client-config"), { retries: 2 });
  const algorithm = window.POW_ALGORITHM;
  if (config.algorithms && !config.algorithms.some((entry) => entry.id === algorithm)) {
    throw new Error(`PoW algorithm is not enabled: ${algorithm}`);
  }
  await window.PowClient.algorithm(algorithm).run(config);
  const session = await jsonFetch(endpoint("/api/session"), { retries: 2 });
  if (session.authenticated !== true) {
    state("cookies");
    return;
  }
  state("success");
  if (window.POW_EMBEDDED) {
    const message = { type: "rilgate:verified" };
    if (window.parent !== window) window.parent.postMessage(message, location.origin);
    if (window.opener) window.opener.postMessage(message, location.origin);
  } else {
    // The delay lets the browser show the success state before the reload.
    setTimeout(() => location.reload(), 300);
  }
}

main().catch((error) => {
  if (error && error.name === "PageReload") return;
  console.error(error);
  const client = window.PowClient;
  if (!client) {
    markLoadFailed();
    return;
  }
  let name = "failed";
  if (/deadline exceeded|^(RandomX|HashWX) challenge expired/i.test(error.message))
    name = "expired";
  else if (error instanceof TypeError) name = "network";
  else if (error.status === 429) name = "limited";
  else if (error.status === 503) name = "unavailable";
  else if (error.code === "unsupported_browser") name = "unsupported";
  else if (error.code === "asset_blocked") name = "blocked";
  client.state(name);
  if (name === "limited") {
    const seconds = error.retryAfterSeconds;
    // biome-ignore lint/suspicious/noGlobalIsFinite: Number.isFinite is unavailable in legacy browsers
    if (typeof seconds === "number" && isFinite(seconds) && seconds > 0) {
      client.status(
        client.t("challenge.state.limited.status", "Too many requests (429)") + error.retryAfter,
      );
      // The status text promises another attempt after the server's delay.
      setTimeout(() => location.reload(), seconds * 1000);
    } else {
      const retry = document.querySelector("#retry");
      if (retry) retry.hidden = false;
    }
  }
});

window.addEventListener("pageshow", (event) => {
  if (event.persisted) location.reload();
});
