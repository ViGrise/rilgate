(() => {
  const statusEl = document.querySelector("#status");
  const progressEl = document.querySelector("#progress");
  const prefix = window.POW_INTERNAL_PREFIX;
  const translationData = document.querySelector("#pow-i18n")?.textContent;
  const translations = translationData ? JSON.parse(translationData) : {};

  function t(key, fallback, args = {}) {
    return (translations[key] ?? fallback).replace(/\{([a-zA-Z0-9_]+)\}/g,
      (token, name) => Object.hasOwn(args, name) ? String(args[name]) : token);
  }

  function status(message) {
    statusEl.textContent = message;
  }

  function endpoint(path) {
    return `${prefix}${path}`;
  }

  async function jsonFetch(url, options = {}) {
    const response = await fetch(url, {
      ...options,
      credentials: "same-origin",
      cache: "no-store",
      signal: options.signal || AbortSignal.timeout(30_000),
      headers: {
        "content-type": "application/json",
        ...(options.headers || {}),
      },
    });

    const body = await response.json().catch(() => ({}));
    if (!response.ok) {
      const retryAfter = Number(response.headers?.get("retry-after"));
      const retry = response.status === 429 && Number.isFinite(retryAfter) && retryAfter > 0
        ? t("challenge.retry_after", " Try again in {seconds} seconds.", { seconds: Math.ceil(retryAfter) })
        : "";
      throw new Error((body.message || `HTTP ${response.status}`) + retry);
    }

    return body;
  }

  async function exchange(receipt) {
    await jsonFetch(endpoint("/api/exchange"), {
      method: "POST",
      body: JSON.stringify({ receipt }),
    });
  }

  async function requestChallenge() {
    return jsonFetch(endpoint("/api/challenge"), {
      method: "POST",
      body: JSON.stringify({
        presentation_token: window.POW_PRESENTATION_TOKEN,
      }),
    });
  }


  window.PowClient = {t,status,endpoint,jsonFetch,exchange,requestChallenge,progressEl};
})();
