(() => {
  const { t, state, stats, verify, requestChallenge, progressEl, once } = window.PowClient;
  function loadSolver() {
    if (
      typeof Worker !== "function" ||
      typeof WebAssembly !== "object" ||
      typeof WebAssembly.instantiate !== "function"
    ) {
      const error = new Error(
        t(
          "challenge.hashwx_unsupported",
          "This browser cannot run HashWX. WebAssembly and workers are required.",
        ),
      );
      error.code = "unsupported_browser";
      return Promise.reject(error);
    }
    return window.PowClient.loadSolver(window.POW_SOLVER_MODULE_URL);
  }
  async function run() {
    const solver = await loadSolver();
    const removePagehide = once(window, "pagehide", () => solver.cancel());
    try {
      state("preparing");
      const challenge = await requestChallenge();
      if (challenge.algorithm !== "hashwx") throw new Error("Expected HashWX challenge");
      if (progressEl) progressEl.removeAttribute("value");
      state("hashwx");
      const solution = await solver.solve(challenge, {
        onProgress: ({ attempts, elapsedMs }) => {
          stats(attempts, (attempts * 1000) / Math.max(1, elapsedMs));
        },
      });
      state("verifying");
      await verify(challenge.challenge_token, solution.nonce);
    } finally {
      removePagehide();
      solver.cancel();
    }
  }
  window.PowClient.register("hashwx", { run });
})();
