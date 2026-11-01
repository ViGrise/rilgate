(() => {
  const bootstrap = JSON.parse(document.querySelector("#pow-bootstrap").textContent);
  Object.keys(bootstrap).forEach((name) => {
    window["POW_" + name.toUpperCase()] = bootstrap[name];
  });
  const statusEl = document.querySelector("#status");
  const progressEl = document.querySelector("#progress");
  const prefix = window.POW_INTERNAL_PREFIX;
  const translationData = document.querySelector("#pow-i18n")?.textContent;
  const translations = translationData ? JSON.parse(translationData) : {};

  function t(key, fallback, args = {}) {
    return (translations[key] ?? fallback).replace(/\{([a-zA-Z0-9_]+)\}/g, (token, name) =>
      // biome-ignore lint/suspicious/noPrototypeBuiltins: Object.hasOwn is unavailable in legacy browsers
      Object.prototype.hasOwnProperty.call(args, name) ? String(args[name]) : token,
    );
  }

  function once(target, type, listener) {
    if (!target) return () => {};
    function handle(event) {
      target.removeEventListener(type, handle);
      listener(event);
    }
    target.addEventListener(type, handle);
    return () => target.removeEventListener(type, handle);
  }

  function status(message) {
    if (statusEl) statusEl.textContent = message;
  }

  const modes = {
    cookies: "retry",
    preparing: "run",
    fast: "run",
    hashwx: "run",
    consent: "start",
    initializing: "run",
    randomx: "run",
    pool: "run",
    verifying: "run",
    success: "done",
    expired: "retry",
    failed: "retry",
    network: "retry",
    unsupported: "none",
    blocked: "retry",
    limited: "none",
    unavailable: "retry",
  };
  let previousMode = "none";
  let currentState = "preparing";
  let attemptsCount = 0;
  function element(id) {
    return document.querySelector("#" + id);
  }
  function hidden(id, value) {
    const target = element(id);
    if (target) target.hidden = value;
  }
  function updateBar(mode) {
    const shimmer = element("bar-shimmer");
    const complete = element("bar-complete");
    const wasRunning = previousMode === "run";
    previousMode = mode;
    hidden("bar", mode !== "run" && mode !== "done");
    hidden("bar-shimmer", mode !== "run");
    if (mode === "run") {
      if (complete) {
        complete.style.transition = "none";
        complete.style.transform = "scaleX(0)";
      }
      if (!wasRunning && shimmer) {
        shimmer.style.animation = "none";
        void shimmer.offsetWidth;
        shimmer.style.animation = "";
      }
    } else if (mode === "done" && complete) {
      complete.style.transition = "none";
      complete.style.transform = "scaleX(0)";
      const reduced =
        typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
      if (reduced || typeof requestAnimationFrame !== "function") {
        complete.style.transform = "scaleX(1)";
      } else {
        requestAnimationFrame(() =>
          requestAnimationFrame(() => {
            if (previousMode !== "done") return;
            complete.style.transition = "transform .55s ease-out";
            complete.style.transform = "scaleX(1)";
          }),
        );
      }
    }
  }
  function showStats() {
    const solving = ["fast", "hashwx", "randomx"].indexOf(currentState) !== -1;
    const retaining =
      ["verifying", "pool", "success"].indexOf(currentState) !== -1 && attemptsCount > 0;
    hidden("stats", !solving && !retaining);
  }
  function state(name, values) {
    currentState = name;
    ["title", "description", "status"].forEach((id) => {
      const target = element(id);
      if (target) target.textContent = t("challenge.state." + name + "." + id, "", values);
    });
    const mode = modes[name];
    updateBar(mode);
    hidden("randomx-start", mode !== "start");
    hidden("retry", mode !== "retry");
    if (progressEl) {
      if (mode === "done") {
        progressEl.max = 1;
        progressEl.value = 1;
      } else progressEl.removeAttribute("value");
      progressEl.hidden = mode !== "run" && mode !== "done";
    }
    if (["fast", "hashwx", "randomx"].indexOf(name) === -1) {
      const rate = element("hashrate");
      if (rate) rate.textContent = "0";
    }
    showStats();
  }
  function currentStateName() {
    return currentState;
  }
  function stats(attempts, rate) {
    attemptsCount = Math.max(0, Number(attempts) || 0);
    const locale = (document.documentElement && document.documentElement.lang) || "en";
    const formatter =
      typeof Intl === "object" && Intl.NumberFormat
        ? new Intl.NumberFormat(locale, { maximumFractionDigits: 0 })
        : null;
    [
      ["attempts", attemptsCount],
      ["hashrate", Math.max(0, Number(rate) || 0)],
    ].forEach((entry) => {
      const target = element(entry[0]);
      if (target)
        target.textContent = formatter ? formatter.format(entry[1]) : String(Math.round(entry[1]));
    });
    hidden("stats", false);
  }

  function endpoint(path) {
    return `${prefix}${path}`;
  }

  function asset(path) {
    return window.POW_STATIC_URL ? window.POW_STATIC_URL + path : endpoint("/static" + path);
  }

  function responseBody(text, code, retryHeader) {
    let body;
    try {
      body = JSON.parse(text);
    } catch (_) {
      body = {};
    }
    if (code < 200 || code >= 300) {
      const retryAfter = Number(retryHeader);
      const retry =
        // biome-ignore lint/suspicious/noGlobalIsFinite: Number.isFinite is unavailable in legacy browsers
        code === 429 && isFinite(retryAfter) && retryAfter > 0
          ? t("challenge.retry_after", " Try again in {seconds} seconds.", {
              seconds: Math.ceil(retryAfter),
            })
          : "";
      const error = new Error((body?.message || `HTTP ${code}`) + retry);
      error.status = code;
      error.code = body?.code;
      error.retryAfter = retry;
      error.retryAfterSeconds =
        retryHeader !== null && retryHeader !== undefined && retryHeader !== ""
          ? retryAfter
          : undefined;
      throw error;
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
      Object.keys(options.headers || {}).forEach((name) => {
        request.setRequestHeader(name, options.headers[name]);
      });
      request.onload = () => {
        cleanup();
        try {
          resolve(
            responseBody(
              request.responseText,
              request.status,
              request.getResponseHeader("retry-after"),
            ),
          );
        } catch (error) {
          reject(error);
        }
      };
      request.onerror = () => {
        cleanup();
        reject(new TypeError("Network request failed"));
      };
      request.ontimeout = () => {
        cleanup();
        reject(new TypeError("Request timed out"));
      };
      request.onabort = () => {
        cleanup();
        reject(new Error("Request canceled"));
      };
      if (signal) {
        if (signal.aborted) {
          abort();
          return;
        }
        signal.addEventListener("abort", abort);
      }
      request.send(options.body === undefined ? null : options.body);
    });
  }

  async function fetchOnce(url, options) {
    if (typeof fetch !== "function" || typeof AbortController !== "function") {
      return xhrFetch(url, options);
    }
    const controller = new AbortController();
    const signal = options.signal;
    const abort = () => controller.abort();
    let timedOut = false;
    const timer = setTimeout(() => {
      timedOut = true;
      abort();
    }, 30000);
    if (signal) {
      if (signal.aborted) abort();
      else signal.addEventListener("abort", abort);
    }
    const headers = { "content-type": "application/json" };
    Object.keys(options.headers || {}).forEach((name) => {
      headers[name] = options.headers[name];
    });
    try {
      const response = await fetch(url, {
        method: options.method || "GET",
        body: options.body,
        credentials: "same-origin",
        cache: "no-store",
        signal: controller.signal,
        headers,
      });
      return responseBody(
        await response.text(),
        response.status,
        response.headers.get("retry-after"),
      );
    } catch (error) {
      // A timeout reports as a network failure, the same as the XHR path.
      if (timedOut) throw new TypeError("Request timed out");
      throw error;
    } finally {
      clearTimeout(timer);
      if (signal) signal.removeEventListener("abort", abort);
    }
  }

  const MAX_RETRY_AFTER_SECONDS = 10;

  // Milliseconds to wait before another attempt, or -1 when the failure is final.
  function retryDelay(error, attempt) {
    if (
      error instanceof TypeError ||
      error.status === 502 ||
      error.status === 504 ||
      (error.status === 503 && !error.code)
    ) {
      return Math.random() * Math.min(10000, 750 * attempt * attempt);
    }
    const seconds = error.retryAfterSeconds;
    if (
      error.status === 429 &&
      typeof seconds === "number" &&
      seconds >= 0 &&
      seconds <= MAX_RETRY_AFTER_SECONDS
    ) {
      return seconds * 1000;
    }
    return -1;
  }

  function sleep(milliseconds) {
    return new Promise((resolve) => setTimeout(resolve, milliseconds));
  }

  // options.retries is the number of extra attempts for requests that are safe to repeat.
  async function jsonFetch(url, options = {}) {
    const retries = options.retries || 0;
    for (let attempt = 1; ; attempt += 1) {
      try {
        return await fetchOnce(url, options);
      } catch (error) {
        const delay =
          attempt <= retries && !(options.signal && options.signal.aborted)
            ? retryDelay(error, attempt)
            : -1;
        if (delay < 0) throw error;
        await sleep(delay);
      }
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
      retries: 2,
      method: "POST",
      body: JSON.stringify({ presentation_token: window.POW_PRESENTATION_TOKEN }),
    });
    challenge.received_at = Date.now();
    return challenge;
  }

  function supportsModules() {
    return typeof HTMLScriptElement === "function" && "noModule" in HTMLScriptElement.prototype;
  }

  // The 8-byte empty module checks that this browser can compile and instantiate WebAssembly.
  function supportsWasm() {
    if (typeof WebAssembly !== "object") return false;
    try {
      const bytes = new Uint8Array([0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]);
      return (
        new WebAssembly.Instance(new WebAssembly.Module(bytes)) instanceof WebAssembly.Instance
      );
    } catch (_) {
      return false;
    }
  }

  function absoluteUrl(value) {
    const anchor = document.createElement("a");
    anchor.href = value;
    return anchor.href;
  }

  function codedError(message, code) {
    const error = new Error(message);
    error.code = code;
    return error;
  }

  let nextSolverRequest = 0;
  const solverRequests = {};
  function solverLoaded(request, module, error) {
    solverRequests[request]?.(module, error);
  }
  function loadSolver(moduleUrl) {
    if (!moduleUrl) return Promise.reject(new Error("Solver module URL is empty"));
    if (!supportsModules()) {
      return Promise.reject(
        codedError("This browser cannot load module scripts", "unsupported_browser"),
      );
    }
    return new Promise((resolve, reject) => {
      const request = String(++nextSolverRequest);
      const solverBase = absoluteUrl(asset("/js/solvers/"));
      // A module that fails to parse reports the SyntaxError through the window error event.
      function onWindowError(event) {
        if (String(event.filename || "").indexOf(solverBase) !== 0) return;
        if (
          (event.error && event.error instanceof SyntaxError) ||
          /SyntaxError/.test(event.message || "")
        ) {
          finish(null, codedError("Solver module uses unsupported syntax", "unsupported_browser"));
        }
      }
      window.addEventListener("error", onWindowError);
      const script = document.createElement("script");
      const timer = setTimeout(
        () => finish(null, new Error("Solver module load timed out")),
        30000,
      );
      function finish(module, error) {
        clearTimeout(timer);
        delete solverRequests[request];
        window.removeEventListener("error", onWindowError);
        script.onerror = null;
        if (script.parentNode) script.parentNode.removeChild(script);
        if (error) reject(error);
        else if (!module || typeof module.solve !== "function")
          reject(new Error("Solver module must export solve"));
        else resolve(module);
      }
      solverRequests[request] = finish;
      script.type = "module";
      script.src =
        asset("/js/solvers/loader.js") +
        "?request=" +
        request +
        "&module=" +
        encodeURIComponent(moduleUrl);
      script.onerror = () =>
        finish(null, codedError("Solver module failed to load", "asset_blocked"));
      try {
        document.head.appendChild(script);
      } catch (error) {
        finish(null, error);
      }
    });
  }

  window.PowClient = {
    loadSolver,
    supportsModules,
    supportsWasm,
    solverLoaded,
    t,
    status,
    state,
    currentState: currentStateName,
    stats,
    endpoint,
    asset,
    jsonFetch,
    exchange,
    verify,
    requestChallenge,
    progressEl,
    once,
  };
})();
