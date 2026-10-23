(() => {
  const { t, status, jsonFetch, endpoint, exchange, requestChallenge, progressEl, once } = window.PowClient;
  function loadRandomXSolver() {
    const moduleUrl = window.POW_SOLVER_MODULE_URL;
    const simd = [0, 97, 115, 109, 1, 0, 0, 0, 1, 4, 1, 96, 0, 0, 3, 2, 1, 0, 10, 9, 1, 7, 0, 65, 0, 253, 15, 26, 11];
    if (typeof Worker !== "function" || typeof WebAssembly !== "object" ||
        typeof WebAssembly.validate !== "function" || typeof BigInt !== "function" ||
        !WebAssembly.validate(new Uint8Array(simd))) {
      return Promise.reject(new Error(t("challenge.randomx_unsupported", "This browser cannot run RandomX. A browser with WebAssembly SIMD, BigInt, and workers is required.")));
    }
    if (!moduleUrl) return Promise.reject(new Error("RandomX solver module URL is empty"));
    return new Promise((resolve, reject) => {
      const script = document.createElement("script");
      const timer = setTimeout(() => finish(new Error("RandomX solver module load timed out")), 30000);
      function finish(error, module) {
        clearTimeout(timer);
        script.onerror = null;
        if (script.parentNode) script.parentNode.removeChild(script);
        delete window.PowClient.randomxLoaded;
        if (error) reject(error);
        else if (!module || typeof module.solve !== "function") reject(new Error("RandomX solver module must export async solve(job, options)"));
        else resolve(module);
      }
      window.PowClient.randomxLoaded = (module, error) => finish(error, module);
      script.type = "module";
      script.src = endpoint("/static/js/solvers/randomx/loader.js");
      script.onerror = () => finish(new Error("RandomX solver module failed to load"));
      document.head.appendChild(script);
    });
  }

  function randomxJob(job, difficulty, nonceOffset, expiresAt) {
    return {
      ...job,
      difficulty,
      nonceOffset,
      expiresAt,
      onProgress: reportRandomxProgress,
    };
  }

  function reportRandomxProgress({ attempts, elapsedMs }) {
    const seconds = Math.max(elapsedMs / 1000, 0.001);
    const rate = (attempts / seconds).toFixed(2);

    status(
      t("challenge.status.solving", "Calculating {algorithm} proof…", { algorithm: "RandomX" }) + "\n" +
        t("challenge.status.progress", "{attempts} attempts · {rate} H/s", { attempts: attempts.toLocaleString(), rate }),
    );
  }

  // The solver abandons an in-flight attempt when the pool publishes a newer job,
  // so a superseded solve is not a challenge failure.
  async function solveRandomX(solver, job) {
    try {
      return await solver.solve(job);
    } catch (error) {
      if (error && error.name === "SupersededError") {
        return null;
      }

      throw error;
    }
  }

  async function randomxLocalVerify(solver) {
    status(t("challenge.status.requesting", "Requesting {algorithm} proof-of-work challenge…", { algorithm: "RandomX" }));

    const challenge = await requestChallenge();

    if (challenge.algorithm !== "randomx") {
      throw new Error(
        `Expected randomx challenge, got ${challenge.algorithm}`,
      );
    }

    status(t("challenge.status.difficulty", "Calculating {algorithm} proof (difficulty {difficulty})…", { algorithm: "RandomX", difficulty: challenge.difficulty }));
    progressEl.removeAttribute("value");

    const solution = await solver.solve(
      randomxJob(challenge.job, challenge.difficulty, challenge.nonce_offset, challenge.expires_at),
    );

    status(t("challenge.status.verifying", "Verifying {algorithm} proof…", { algorithm: "RandomX" }));

    const verified = await jsonFetch(endpoint("/api/verify"), {
      method: "POST",
      body: JSON.stringify({
        challenge_token: challenge.challenge_token,
        nonce: solution.nonce,
      }),
    });

    await exchange(verified.receipt);
  }


  async function run(config) {
      const description = document.querySelector("#description");
      description.textContent = config.enforce === "stratum_pool_accepted"
        ? t("pow.disclosure.mining", "Your browser will perform RandomX mining work submitted to the gateway's mining pool. This uses about 256 MiB of memory and CPU time, and can take a long time. Start only if you agree to contribute this work.")
        : t("pow.disclosure.randomx", "Your browser will calculate a RandomX proof using about 256 MiB of memory and CPU time. This can take a long time.");
      status(t("challenge.status.waiting", "Waiting for you to start RandomX work."));
      const startButton = document.querySelector("#randomx-start");
      startButton.hidden = false;
      await new Promise((resolve) => once(startButton, "click", resolve));
      startButton.hidden = true;
      const solver = await loadRandomXSolver();
      const removePagehide = once(window, "pagehide", () => solver.cancel?.());
      try {
        if (config.enforce === "local_verify") {
          await randomxLocalVerify(solver);
        } else if (config.enforce === "stratum_pool_accepted") {
          await window.PowClient.stratumPoolAccepted(solver);
        } else {
          throw new Error(
            `Unsupported enforcement mode: ${config.enforce}`,
          );
        }
      } finally {
        removePagehide();
        solver.cancel?.();
      }
  }
  window.PowClient.randomxJob = randomxJob;
  window.PowClient.solveRandomX = solveRandomX;
  window.PowClient.register("randomx", { run });
})();
