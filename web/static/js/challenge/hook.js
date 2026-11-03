(() => {
  const adapters = Object.create(null);
  const client = window.PowClient;
  let loadedCount = 0;

  client.registerHook = (name, adapter) => {
    // biome-ignore lint/suspicious/noPrototypeBuiltins: Object.hasOwn is unavailable in legacy browsers
    if (Object.prototype.hasOwnProperty.call(adapters, name)) {
      throw new Error(`Duplicate hook adapter: ${name}`);
    }
    adapters[name] = adapter;
  };

  function resolveCall(path) {
    const parts = path.split(".");
    let owner = window;
    for (let i = 0; i < parts.length; i++) {
      // biome-ignore lint/suspicious/noPrototypeBuiltins: Object.hasOwn is unavailable in legacy browsers
      if (owner == null || !Object.prototype.hasOwnProperty.call(owner, parts[i])) return null;
      const value = owner[parts[i]];
      if (i === parts.length - 1) {
        return typeof value === "function" ? { owner, fn: value } : null;
      }
      owner = value;
    }
    return null;
  }

  function loadAttempt(hook) {
    return new Promise((resolve, reject) => {
      const widget = hook.widget;
      const settings = widget || hook.script;
      const script = document.createElement("script");
      let callback;
      let poll;
      let finished = false;
      const timer = setTimeout(() => finish(new Error("Challenge hook load timed out")), 10000);
      function finish(error) {
        if (finished) return;
        finished = true;
        clearTimeout(timer);
        clearTimeout(poll);
        if (callback) delete window[callback];
        script.onerror = null;
        if (error) {
          if (script.parentNode) script.parentNode.removeChild(script);
          reject(error);
        } else resolve();
      }
      script.async = true;
      script.src = widget ? widget.script : settings.src;
      if (settings.integrity != null) {
        script.integrity = settings.integrity;
        script.crossOrigin = "anonymous";
      }
      if (widget && widget.onload) {
        callback = "__powHookLoaded" + ++loadedCount;
        window[callback] = () => finish();
        script.src += (script.src.indexOf("?") === -1 ? "?" : "&") + widget.onload + "=" + callback;
      } else {
        const check = () => {
          if (widget ? resolveCall(widget.call) : adapters[hook.name]) finish();
          else poll = setTimeout(check, 50);
        };
        poll = setTimeout(check, 50);
      }
      script.onerror = () => finish(new Error("Challenge hook script failed to load"));
      document.head.appendChild(script);
    });
  }

  async function load(hook) {
    for (let attempt = 1; attempt <= 3; attempt++) {
      try {
        await loadAttempt(hook);
        return;
      } catch (_) {
        await new Promise((resolve) =>
          setTimeout(resolve, Math.random() * Math.min(10000, 750 * attempt * attempt)),
        );
      }
    }
    const error = new Error("Challenge hook script failed to load");
    error.code = "asset_blocked";
    throw error;
  }

  function childContainer(container) {
    container.textContent = "";
    const child = document.createElement("div");
    container.appendChild(child);
    return child;
  }

  function widgetResult(widget, container) {
    return new Promise((resolve, reject) => {
      let renders = 0;
      let generation = 0;
      let settled = false;
      function render() {
        if (settled) return;
        const current = ++generation;
        if (++renders > 5) {
          settled = true;
          reject(new Error("Challenge hook failed"));
          return;
        }
        const child = childContainer(container);
        let completed = false;
        const retry = () => {
          if (settled || completed || current !== generation) return;
          completed = true;
          render();
        };
        const result = (value) => {
          if (settled || completed || current !== generation) return;
          if (typeof value !== "string" || value.length === 0) {
            retry();
            return;
          }
          completed = true;
          settled = true;
          resolve(value);
        };
        function argument(value) {
          if (value === "$container") return child;
          if (value === "$result") return result;
          if (value === "$error" || value === "$expired") return retry;
          if (Array.isArray(value)) return value.map(argument);
          if (value && typeof value === "object") {
            const copy = {};
            Object.keys(value).forEach((key) => {
              Object.defineProperty(copy, key, {
                value: argument(value[key]),
                enumerable: true,
                writable: true,
                configurable: true,
              });
            });
            return copy;
          }
          return value;
        }
        try {
          const call = resolveCall(widget.call);
          const returned = call.fn.apply(call.owner, widget.args.map(argument));
          if (widget.returns === "promise") Promise.resolve(returned).then(result, retry);
        } catch (error) {
          settled = true;
          reject(error);
        }
      }
      render();
    });
  }

  client.runHook = async (hook) => {
    document.documentElement.classList.add("pow-hook");
    client.state("hook");
    let container = document.querySelector("#hook-container");
    if (!container) {
      container = document.createElement("div");
      container.id = "hook-container";
      document.querySelector("main").appendChild(container);
    }
    container.hidden = false;
    await load(hook);
    const controller = hook.widget ? null : new AbortController();
    if (controller) client.once(window, "pagehide", () => controller.abort());
    for (;;) {
      const result = hook.widget
        ? await widgetResult(hook.widget, container)
        : await adapters[hook.name].mount({
            container: childContainer(container),
            params: hook.params,
            challengeId: hook.challenge_id,
            locale: document.documentElement.lang,
            signal: controller.signal,
          });
      if (typeof result !== "string" || result.length === 0) {
        throw new Error("Challenge hook failed");
      }
      client.state("verifying");
      try {
        const body = await client.jsonFetch(client.endpoint("/api/hook/verify"), {
          method: "POST",
          retries: 0,
          body: JSON.stringify({ presentation_token: window.POW_PRESENTATION_TOKEN, result }),
        });
        client.setNext(body.next);
        return;
      } catch (error) {
        if (error.code === "hook_rejected") client.state("rejected");
        else if (error.code === "hook_unavailable" && error.body.next) {
          client.setNext(error.body.next);
          return;
        } else throw error;
      }
    }
  };
})();
