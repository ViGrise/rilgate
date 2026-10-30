(() => {
  const statusEl = document.querySelector("#status");
  const progressEl = document.querySelector("#progress");
  const prefix = window.POW_INTERNAL_PREFIX;
  const translationData = document.querySelector("#pow-i18n")?.textContent;
  const translations = translationData ? JSON.parse(translationData) : {};

  function t(key, fallback, args = {}) {
    return (translations[key] ?? fallback).replace(/\{([a-zA-Z0-9_]+)\}/g,
      (token, name) => Object.prototype.hasOwnProperty.call(args, name) ? String(args[name]) : token);
  }

  function once(target, type, listener) {
    function handle(event) {
      target.removeEventListener(type, handle);
      listener(event);
    }
    target.addEventListener(type, handle);
    return () => target.removeEventListener(type, handle);
  }

  function status(message) {
    statusEl.textContent = message;
  }

  function endpoint(path) {
    return `${prefix}${path}`;
  }

  function responseBody(text, code, retryHeader) {
    let body;
    try { body = JSON.parse(text); } catch (_) { body = {}; }
    if (code < 200 || code >= 300) {
      const retryAfter = Number(retryHeader);
      const retry = code === 429 && isFinite(retryAfter) && retryAfter > 0
        ? t("challenge.retry_after", " Try again in {seconds} seconds.", { seconds: Math.ceil(retryAfter) })
        : "";
      throw new Error((body?.message || `HTTP ${code}`) + retry);
    }
    return body;
  }

  function xhrFetch(url, options) {
    return new Promise((resolve, reject) => {
      const request = new XMLHttpRequest();
      const signal = options.signal;
      function cleanup() {
        if (signal) signal.removeEventListener("abort", abort);
      }
      function abort() {
        request.abort();
        cleanup();
        reject(new Error("Request canceled"));
      }
      request.open(options.method || "GET", url, true);
      request.timeout = 30000;
      request.setRequestHeader("content-type", "application/json");
      Object.keys(options.headers || {}).forEach((name) => request.setRequestHeader(name, options.headers[name]));
      request.onload = () => {
        cleanup();
        try {
          resolve(responseBody(request.responseText, request.status, request.getResponseHeader("retry-after")));
        } catch (error) { reject(error); }
      };
      request.onerror = () => { cleanup(); reject(new Error("Network request failed")); };
      request.ontimeout = () => { cleanup(); reject(new Error("Request timed out")); };
      request.onabort = () => { cleanup(); reject(new Error("Request canceled")); };
      if (signal) {
        if (signal.aborted) { abort(); return; }
        signal.addEventListener("abort", abort);
      }
      request.send(options.body === undefined ? null : options.body);
    });
  }

  async function jsonFetch(url, options = {}) {
    if (typeof fetch !== "function" || typeof AbortController !== "function") {
      return xhrFetch(url, options);
    }
    const controller = new AbortController();
    const signal = options.signal;
    const abort = () => controller.abort();
    const timer = setTimeout(abort, 30000);
    if (signal) {
      if (signal.aborted) abort();
      else signal.addEventListener("abort", abort);
    }
    const headers = { "content-type": "application/json" };
    Object.keys(options.headers || {}).forEach((name) => { headers[name] = options.headers[name]; });
    try {
      const response = await fetch(url, {
        method: options.method || "GET",
        body: options.body,
        credentials: "same-origin",
        cache: "no-store",
        signal: controller.signal,
        headers,
      });
      return responseBody(await response.text(), response.status, response.headers.get("retry-after"));
    } finally {
      clearTimeout(timer);
      if (signal) signal.removeEventListener("abort", abort);
    }
  }

  async function exchange(receipt) {
    await jsonFetch(endpoint("/api/exchange"), {
      method: "POST",
      body: JSON.stringify({ receipt }),
    });
  }

  async function verify(challenge_token, nonce) {
    const verified = await jsonFetch(endpoint("/api/verify"), {
      method: "POST",
      body: JSON.stringify({ challenge_token, nonce, set_cookie: true }),
    });
    if (verified.cookie_set !== true) await exchange(verified.receipt);
  }

  async function requestChallenge() {
    const challenge = await jsonFetch(endpoint("/api/challenge"), {
      method: "POST",
      body: JSON.stringify({ presentation_token: window.POW_PRESENTATION_TOKEN }),
    });
    challenge.received_at = Date.now();
    return challenge;
  }

  let nextSolverRequest = 0;
  const solverRequests = {};
  function solverLoaded(request, module, error) {
    solverRequests[request]?.(module, error);
  }
  function loadSolver(moduleUrl) {
    if (!moduleUrl) return Promise.reject(new Error("Solver module URL is empty"));
    return new Promise((resolve, reject) => {
      const request = String(++nextSolverRequest);
      const script = document.createElement("script");
      const timer = setTimeout(() => finish(null, new Error("Solver module load timed out")), 30000);
      function finish(module, error) {
        clearTimeout(timer);
        delete solverRequests[request];
        script.onerror = null;
        if (script.parentNode) script.parentNode.removeChild(script);
        if (error) reject(error);
        else if (!module || typeof module.solve !== "function") reject(new Error("Solver module must export solve"));
        else resolve(module);
      }
      solverRequests[request] = finish;
      script.type = "module";
      script.src = endpoint("/static/js/solvers/loader.js") + "?request=" + request + "&module=" + encodeURIComponent(moduleUrl);
      script.onerror = () => finish(null, new Error("Solver module failed to load"));
      try { document.head.appendChild(script); } catch (error) { finish(null, error); }
    });
  }

  window.PowClient = { loadSolver, solverLoaded, t, status, endpoint, jsonFetch, exchange, verify, requestChallenge, progressEl, once };
})();
