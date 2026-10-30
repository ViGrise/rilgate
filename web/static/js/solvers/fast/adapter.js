(() => {
  const { t, status, verify, requestChallenge, progressEl } = window.PowClient;
  async function fastVerify() {
    status(t("challenge.status.requesting", "Requesting {algorithm} proof-of-work challenge…", { algorithm: "lightweight" }));

    const challenge = await requestChallenge();

    if (challenge.algorithm !== "fast") {
      throw new Error(
        `Expected fast challenge, got ${challenge.algorithm}`,
      );
    }

    status(
      t("challenge.status.fast_difficulty", "Calculating SHA-256 proof ({difficulty} leading zero hex digits)…", { difficulty: challenge.difficulty }),
    );
    progressEl.removeAttribute("value");

    const solution = await window.PowClient.solveFast(challenge);

    status(t("challenge.status.verifying", "Verifying {algorithm} proof…", { algorithm: "lightweight" }));

    await verify(challenge.challenge_token, solution.nonce);
  }


  window.PowClient.register("fast", {run: fastVerify});
})();
