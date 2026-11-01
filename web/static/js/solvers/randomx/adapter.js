(() => {
  const reloadKey = "rilgate-work-unavailable-reload";
  const { t, state, stats, verify, requestChallenge, progressEl, once, reloadAllowed } =
    window.PowClient;
  function loadRandomXSolver() {
    const moduleUrl = window.POW_SOLVER_MODULE_URL;
    if (
      typeof Worker !== "function" ||
      !window.PowClient.supportsWasm() ||
      typeof BigInt !== "function"
    ) {
      const error = new Error(
        t(
          "challenge.randomx_unsupported",
          "This browser cannot run RandomX. A browser with WebAssembly, BigInt, and workers is required.",
        ),
      );
      error.code = "unsupported_browser";
      return Promise.reject(error);
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

    state("randomx");
    stats(attempts, rate);
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
    state("preparing");

    let challenge;
    try {
      challenge = await requestChallenge();
      try {
        sessionStorage.removeItem(reloadKey);
      } catch (_) {
        // Storage can be unavailable; the one-time reload is skipped then.
      }
    } catch (error) {
      if (error.status === 503 && error.code === "work_unavailable" && reloadAllowed(reloadKey)) {
        throw window.PowClient.reloadPage();
      }
      throw error;
    }

    if (challenge.algorithm !== "randomx") {
      throw new Error(`Expected randomx challenge, got ${challenge.algorithm}`);
    }

    state("initializing");
    if (progressEl) progressEl.removeAttribute("value");

    const solution = await solver.solve(
      randomxJob(
        challenge.job,
        challenge.difficulty,
        challenge.nonce_offset,
        challenge.expires_in,
        challenge.received_at,
      ),
    );

    state("verifying");

    await verify(challenge.challenge_token, solution.nonce);
  }

  async function run(config) {
    state("consent");
    const startButton = document.querySelector("#randomx-start");
    await new Promise((resolve) => once(startButton, "click", resolve));
    state("initializing");
    const module = await loadRandomXSolver();
    const options = config.algorithms.find((entry) => entry.id === "randomx")?.browser;
    const solver = {
      solve: (job) => module.solve(job, options),
      cancel: () => module.cancel?.(),
    };
    const removePagehide = once(window, "pagehide", () => solver.cancel?.());
    const verification = config.algorithms.find((entry) => entry.id === "randomx")?.verification;
    try {
      if (verification === "gateway") {
        await randomxLocalVerify(solver);
      } else if (verification === "pool_share") {
        try {
          await window.PowClient.stratumPoolAccepted(solver);
        } catch (error) {
          if (
            error.status === 503 &&
            error.code === "work_unavailable" &&
            reloadAllowed(reloadKey)
          ) {
            throw window.PowClient.reloadPage();
          }
          throw error;
        }
      } else {
        throw new Error(`Unsupported verification mode: ${verification}`);
      }
    } finally {
      removePagehide();
      if (verification !== "pool_share") solver.cancel?.();
    }
  }
  window.PowClient.randomxJob = randomxJob;
  window.PowClient.solveRandomX = solveRandomX;
  window.PowClient.register("randomx", { run });
})();
