(() => {
  const { t, status, verify, requestChallenge, progressEl, once } = window.PowClient;
  function loadRandomXSolver() {
    const moduleUrl = window.POW_SOLVER_MODULE_URL;
    if (typeof Worker !== "function" || typeof WebAssembly !== "object" ||
        typeof WebAssembly.validate !== "function" || typeof BigInt !== "function") {
      return Promise.reject(new Error(t("challenge.randomx_unsupported", "This browser cannot run RandomX. A browser with WebAssembly, BigInt, and workers is required.")));
    }
    if (!moduleUrl) return Promise.reject(new Error("RandomX solver module URL is empty"));
    return window.PowClient.loadSolver(window.POW_SOLVER_MODULE_URL);
  }

  function randomxJob(job, difficulty, nonceOffset, expiresIn, receivedAt) {
    return {
      ...job,
      difficulty,
      nonceOffset,
      expiresIn,
      receivedAt,
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

    const reloadKey = "rilgate-work-unavailable-reload";
    let challenge;
    try {
      challenge = await requestChallenge();
      sessionStorage.removeItem(reloadKey);
    } catch (error) {
      if (error.status === 503 && error.code === "work_unavailable" &&
          sessionStorage.getItem(reloadKey) !== window.location.href) {
        sessionStorage.setItem(reloadKey, window.location.href);
        window.location.reload();
        return;
      }
      throw error;
    }

    if (challenge.algorithm !== "randomx") {
      throw new Error(
        `Expected randomx challenge, got ${challenge.algorithm}`,
      );
    }

    status(t("challenge.status.difficulty", "Calculating {algorithm} proof (difficulty {difficulty})…", { algorithm: "RandomX", difficulty: challenge.difficulty }));
    progressEl.removeAttribute("value");

    const solution = await solver.solve(
      randomxJob(challenge.job, challenge.difficulty, challenge.nonce_offset, challenge.expires_in, challenge.received_at),
    );

    status(t("challenge.status.verifying", "Verifying {algorithm} proof…", { algorithm: "RandomX" }));

    await verify(challenge.challenge_token, solution.nonce);
  }


  async function run(config) {
      const description = document.querySelector("#description");
      description.textContent = config.enforce === "stratum_pool_accepted"
        ? t("pow.disclosure.mining", "Your browser will perform RandomX mining work submitted to the gateway's mining pool. This uses CPU time and about 256 MiB of memory per worker, and can take a long time. Start only if you agree to contribute this work.")
        : t("pow.disclosure.randomx", "Your browser will calculate a RandomX proof using CPU time and about 256 MiB of memory per worker. This can take a long time.");
      status(t("challenge.status.waiting", "Waiting for you to start RandomX work."));
      const startButton = document.querySelector("#randomx-start");
      startButton.hidden = false;
      await new Promise((resolve) => once(startButton, "click", resolve));
      startButton.hidden = true;
      const module = await loadRandomXSolver();
      const options = config.algorithms.find((entry) => entry.id === "randomx")?.browser;
      const solver = {
        solve: (job) => module.solve(job, options),
        cancel: () => module.cancel?.(),
      };
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
