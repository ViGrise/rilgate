(() => {
  const { state, endpoint, exchange, progressEl, once, reloadAllowed } = window.PowClient;
  async function stratumPoolAccepted(solver) {
    const protocol = location.protocol === "https:" ? "wss:" : "ws:";
    const presentation = encodeURIComponent(window.POW_PRESENTATION_TOKEN);
    const delays = [1, 2, 4, 8, 15];
    let attempt = 0;
    let failuresWithoutJob = 0;
    // Workers keep their cache across jobs with the same seed.
    let started = false;
    let seed = null;

    try {
      for (;;) {
        state("connecting");
        let ws;
        let generation = 0;
        let receivedJob = false;
        let accepted = false;
        try {
          ws = new WebSocket(
            `${protocol}//${location.host}` +
              `${endpoint("/api/ws")}` +
              `?presentation_token=${presentation}`,
          );
          await new Promise((resolve, reject) => {
            once(ws, "open", resolve);
            once(ws, "error", () => reject(new Error("WebSocket connection failed")));
            once(ws, "close", () => reject(new Error("WebSocket connection closed")));
          });

          await new Promise((resolve, reject) => {
            let completed = false;
            ws.addEventListener("message", async (event) => {
              if (completed) return;
              try {
                const message = JSON.parse(event.data);
                if (message.type === "job") {
                  receivedJob = true;
                  failuresWithoutJob = 0;
                  const myGeneration = ++generation;
                  window.PowClient.job({
                    id: message.job.job_id,
                    height: message.job.height,
                    seed: message.job.seed_hash,
                    reseeded: started && message.job.seed_hash !== seed,
                  });
                  if (started && message.job.seed_hash === seed) {
                    state("randomx");
                  } else {
                    started = true;
                    state("initializing");
                    if (progressEl) progressEl.removeAttribute("value");
                  }
                  seed = message.job.seed_hash;
                  const solution = await window.PowClient.solveRandomX(
                    solver,
                    window.PowClient.randomxJob(
                      message.job,
                      message.difficulty,
                      message.nonce_offset,
                    ),
                  );
                  if (!solution || myGeneration !== generation || completed) return;
                  ws.send(
                    JSON.stringify({
                      type: "share",
                      job_id: message.job.job_id,
                      nonce: solution.nonce,
                      result: solution.result,
                    }),
                  );
                  state("pool");
                  return;
                }
                if (message.type === "accepted") {
                  accepted = true;
                  completed = true;
                  state("verifying");
                  await exchange(message.receipt);
                  try {
                    sessionStorage.removeItem("rilgate-work-unavailable-reload");
                    sessionStorage.removeItem("rilgate-pool-resume-reload");
                  } catch (_) {
                    // Storage can be unavailable; the one-time reload is skipped then.
                  }
                  ws.close();
                  resolve();
                  return;
                }
                if (message.type === "rejected") {
                  state("pool");
                  return;
                }
                if (message.type === "error") {
                  const error = new Error(message.message);
                  if (message.code === "work_unavailable") {
                    error.status = 503;
                    error.code = "work_unavailable";
                  }
                  completed = true;
                  reject(error);
                }
              } catch (error) {
                completed = true;
                reject(error);
              }
            });
            once(ws, "close", () => {
              if (!completed) {
                completed = true;
                reject(new Error("Challenge connection closed before acceptance"));
              }
            });
            once(ws, "error", () => {
              if (!completed) {
                completed = true;
                reject(new Error("WebSocket connection failed"));
              }
            });
          });
          return;
        } catch (error) {
          if (
            accepted ||
            error.code === "work_unavailable" ||
            error.code === "unsupported_browser" ||
            error.code === "asset_blocked"
          )
            throw error;
        } finally {
          generation += 1;
          ws?.close();
        }
        if (!receivedJob) failuresWithoutJob += 1;
        if (failuresWithoutJob >= 5) {
          if (reloadAllowed("rilgate-pool-resume-reload")) {
            throw window.PowClient.reloadPage();
          }
          throw new Error("RandomX pool session could not resume");
        }
        await new Promise((resolve) =>
          setTimeout(resolve, delays[Math.min(attempt++, delays.length - 1)] * 1000),
        );
      }
    } finally {
      solver.cancel?.();
    }
  }

  window.PowClient.stratumPoolAccepted = stratumPoolAccepted;
})();
