(() => {
  const { t, status, jsonFetch, endpoint, exchange, requestChallenge, progressEl } = window.PowClient;
  async function loadRandomXSolver() {
    const moduleUrl = window.POW_SOLVER_MODULE_URL;
    if (!moduleUrl) {
      throw new Error("RandomX solver module URL is empty");
    }

    const module = await import(moduleUrl);
    if (typeof module.solve !== "function") {
      throw new Error(
        "RandomX solver module must export async solve(job, options)",
      );
    }

    return module;
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

  // The solver abandons an in-flight attempt when P2Pool publishes a newer job,
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
      description.textContent = config.enforce === "p2pool_accepted"
        ? t("pow.disclosure.mining", "Your browser will perform RandomX mining work submitted to the gateway's P2Pool mining setup. This uses about 256 MiB of memory and CPU time, and can take a long time. Start only if you agree to contribute this work.")
        : t("pow.disclosure.randomx", "Your browser will calculate a RandomX proof using about 256 MiB of memory and CPU time. This can take a long time.");
      status(t("challenge.status.waiting", "Waiting for you to start RandomX work."));
      const startButton = document.querySelector("#randomx-start");
      startButton.hidden = false;
      await new Promise((resolve) => startButton.addEventListener("click", resolve, { once: true }));
      startButton.hidden = true;
      const solver = await loadRandomXSolver();
      window.addEventListener("pagehide", () => solver.cancel?.(), { once: true });
      try {
        if (config.enforce === "local_verify") {
          await randomxLocalVerify(solver);
        } else if (config.enforce === "p2pool_accepted") {
          await window.PowClient.p2poolAccepted(solver);
        } else {
          throw new Error(
            `Unsupported enforcement mode: ${config.enforce}`,
          );
        }
      } finally {
        solver.cancel?.();
      }
  }
  window.PowClient.randomxJob = randomxJob;
  window.PowClient.solveRandomX = solveRandomX;
  window.PowClient.register("randomx", { run });
})();
