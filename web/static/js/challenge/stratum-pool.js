(() => {
  const { state, endpoint, exchange, progressEl, once } = window.PowClient;
  async function stratumPoolAccepted(solver) {
    state("pool");

    const protocol = location.protocol === "https:" ? "wss:" : "ws:";
    const presentation = encodeURIComponent(window.POW_PRESENTATION_TOKEN);

    const ws = new WebSocket(
      `${protocol}//${location.host}` +
        `${endpoint("/api/ws")}` +
        `?presentation_token=${presentation}`,
    );

    await new Promise((resolve, reject) => {
      once(ws, "open", resolve);
      once(ws, "error", () => reject(new Error("WebSocket connection failed")));
      once(ws, "close", () => reject(new Error("WebSocket connection closed")));
    }).catch((error) => {
      ws.close();
      throw error;
    });

    let generation = 0;

    try {
      await new Promise((resolve, reject) => {
        let completed = false;

        ws.addEventListener("message", async (event) => {
          try {
            const message = JSON.parse(event.data);

            if (message.type === "job") {
              const myGeneration = ++generation;

              state("initializing");
              if (progressEl) progressEl.removeAttribute("value");

              const solution = await window.PowClient.solveRandomX(
                solver,
                window.PowClient.randomxJob(message.job, message.difficulty, message.nonce_offset),
              );

              if (!solution || myGeneration !== generation) {
                return;
              }

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
              completed = true;
              await exchange(message.receipt);
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
              reject(error);
            }
          } catch (error) {
            reject(error);
          }
        });

        once(ws, "close", () => {
          if (!completed) {
            reject(new Error("Challenge connection closed before acceptance"));
          }
        });
      });
    } finally {
      generation += 1;
      ws.close();
      solver.cancel?.();
    }
  }

  window.PowClient.stratumPoolAccepted = stratumPoolAccepted;
})();
