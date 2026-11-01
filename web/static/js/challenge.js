async function main() {
  const { state, jsonFetch, endpoint } = window.PowClient;
  window.PowClient.once(document.querySelector("#retry"), "click", () => location.reload());
  if (navigator.cookieEnabled === false) {
    state("cookies");
    return;
  }
  state("preparing");
  const config = await jsonFetch(endpoint("/api/client-config"));
  const algorithm = window.POW_ALGORITHM;
  if (config.algorithms && !config.algorithms.some((entry) => entry.id === algorithm)) {
    throw new Error(`PoW algorithm is not enabled: ${algorithm}`);
  }
  await window.PowClient.algorithm(algorithm).run(config);
  const session = await jsonFetch(endpoint("/api/session"));
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
    location.reload();
  }
}

main().catch((error) => {
  console.error(error);
  const client = window.PowClient;
  let name = "failed";
  if (/deadline exceeded|^(RandomX|HashWX) challenge expired/i.test(error.message))
    name = "expired";
  else if (error instanceof TypeError) name = "network";
  else if (error.status === 429) name = "limited";
  else if (error.status === 503) name = "unavailable";
  else if (error.code === "unsupported_browser") name = "unsupported";
  client.state(name);
  if (name === "limited" && error.retryAfter) {
    client.status(
      client.t("challenge.state.limited.status", "Too many requests (429)") + error.retryAfter,
    );
  }
});
