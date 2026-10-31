(() => {
  const { stats, asset } = window.PowClient;
  function solveFast(challenge) {
    return new Promise((resolve, reject) => {
      let worker = null;
      let cancel = null;

      const abort = () => {
        finish();
        const error = new Error("Proof calculation canceled");
        error.name = "AbortError";
        reject(error);
      };
      function finish() {
        if (worker) {
          worker.terminate();
          worker.onmessage = null;
          worker.onerror = null;
        }
        if (cancel) cancel();
        window.removeEventListener("pagehide", abort);
      }
      window.addEventListener("pagehide", abort);

      const onmessage = (event) => {
        const message = event.data;

        if (message.type === "solved") {
          finish();
          resolve({ nonce: message.nonce });
          return;
        }

        if (message.type === "progress") {
          const seconds = Math.max(message.elapsed_ms / 1000, 0.001);
          const rate = Math.round(message.attempts / seconds);

          stats(message.attempts, rate);
          return;
        }

        if (message.type === "error") {
          finish();
          reject(new Error(message.message));
        }
      };

      const onerror = (event) => {
        finish();
        reject(new Error(event.message || "SHA-256 worker failed"));
      };

      try {
        const data = { challenge: challenge.challenge, difficulty: challenge.difficulty, start: 0 };
        if (typeof Worker === "function") {
          worker = new Worker(asset("/js/solvers/fast/worker.js"));
          worker.onmessage = onmessage;
          worker.onerror = onerror;
          worker.postMessage(data);
        } else {
          cancel = window.PowClient.startFastWork(data, (message) => onmessage({ data: message }));
        }
      } catch (error) {
        finish();
        reject(error);
      }
    });
  }


  window.PowClient.solveFast = solveFast;
})();
