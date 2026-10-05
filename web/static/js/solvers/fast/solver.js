(() => {
  const { t, status, endpoint } = window.PowClient;
  function solveFast(challenge) {
    return new Promise((resolve, reject) => {
      const worker = new Worker(endpoint("/static/js/solvers/fast/worker.js"), {
        type: "classic",
      });

      const abort = () => {
        finish();
        reject(new DOMException("Proof calculation canceled", "AbortError"));
      };
      function finish() {
        worker.terminate();
        worker.onmessage = null;
        worker.onerror = null;
        window.removeEventListener("pagehide", abort);
      }
      window.addEventListener("pagehide", abort, { once: true });

      worker.onmessage = (event) => {
        const message = event.data;

        if (message.type === "solved") {
          finish();
          resolve({ nonce: message.nonce });
          return;
        }

        if (message.type === "progress") {
          const seconds = Math.max(message.elapsed_ms / 1000, 0.001);
          const rate = Math.round(message.attempts / seconds);

          status(
            t("challenge.status.solving", "Calculating {algorithm} proof…", { algorithm: "SHA-256" }) + "\n" +
            t("challenge.status.progress", "{attempts} attempts · {rate} H/s", { attempts: message.attempts.toLocaleString(), rate: rate.toLocaleString() }),
          );
          return;
        }

        if (message.type === "error") {
          finish();
          reject(new Error(message.message));
        }
      };

      worker.onerror = (event) => {
        finish();
        reject(new Error(event.message || "SHA-256 worker failed"));
      };

      try {
        worker.postMessage({
          challenge: challenge.challenge,
          difficulty: challenge.difficulty,
          start: 0,
        });
      } catch (error) {
        finish();
        reject(error);
      }
    });
  }


  window.PowClient.solveFast = solveFast;
})();
