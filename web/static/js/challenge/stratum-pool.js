(() => {
  const { t, status, endpoint, exchange, progressEl, once } = window.PowClient;
  async function stratumPoolAccepted(solver) {
    status(t("challenge.status.connecting", "Connecting to the mining pool proof-of-work service…"));

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

              status(
                t("challenge.status.difficulty", "Calculating {algorithm} proof (difficulty {difficulty})…", { algorithm: "RandomX", difficulty: message.difficulty }),
              );
              progressEl.removeAttribute("value");

              const solution = await window.PowClient.solveRandomX(
                solver,
                window.PowClient.randomxJob(message.job, message.difficulty, message.nonce_offset),
              );

              if (!solution || myGeneration !== generation) {
                return;
              }

              ws.send(JSON.stringify({
                type: "share",
                job_id: message.job.job_id,
                nonce: solution.nonce,
                result: solution.result,
              }));

              status(t("challenge.status.submitted", "Proof submitted; waiting for mining pool acceptance…"));
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
              status(
                t("challenge.status.rejected", "The mining pool rejected the proof: {reason}\nWaiting for another job…", { reason: message.reason }),
              );
              return;
            }

            if (message.type === "error") {
              reject(new Error(message.message));
            }
          } catch (error) {
            reject(error);
          }
        });

        once(
          ws, "close",
          () => {
            if (!completed) {
              reject(
                new Error(
                  "Challenge connection closed before acceptance",
                ),
              );
            }
          },
        );
      });
    } finally {
      generation += 1;
      ws.close();
      solver.cancel?.();
    }
  }


  window.PowClient.stratumPoolAccepted = stratumPoolAccepted;
})();
