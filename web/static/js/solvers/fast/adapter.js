(() => {
  const { state, verify, requestChallenge, progressEl } = window.PowClient;
  async function fastVerify() {
    state("preparing");

    const challenge = await requestChallenge();

    if (challenge.algorithm !== "fast") {
      throw new Error(
        `Expected fast challenge, got ${challenge.algorithm}`,
      );
    }

    state("fast");
    if (progressEl) progressEl.removeAttribute("value");

    const solution = await window.PowClient.solveFast(challenge);

    state("verifying");

    await verify(challenge.challenge_token, solution.nonce);
  }


  window.PowClient.register("fast", {run: fastVerify});
})();
