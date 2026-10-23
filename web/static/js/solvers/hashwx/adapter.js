(() => {
  const { t, status, endpoint, jsonFetch, exchange, requestChallenge, progressEl, once } = window.PowClient;
  function loadSolver() {
    if (typeof Worker !== 'function' || typeof WebAssembly !== 'object' ||
        typeof WebAssembly.instantiate !== 'function') {
      return Promise.reject(new Error(t('challenge.hashwx_unsupported', 'This browser cannot run HashWX. WebAssembly and workers are required.')));
    }
    return window.PowClient.loadSolver(window.POW_SOLVER_MODULE_URL);
  }
  async function run() {
    const solver = await loadSolver();
    const removePagehide = once(window, 'pagehide', () => solver.cancel());
    try {
      status(t('challenge.status.requesting', 'Requesting {algorithm} proof-of-work challenge…', { algorithm: 'HashWX' }));
      const challenge = await requestChallenge();
      if (challenge.algorithm !== 'hashwx') throw new Error('Expected HashWX challenge');
      progressEl.removeAttribute('value');
      status(t('challenge.status.solving', 'Calculating {algorithm} proof…', { algorithm: 'HashWX' }));
      const solution = await solver.solve(challenge, { onProgress: ({ attempts, elapsedMs }) => {
        status(t('challenge.status.progress', '{attempts} attempts · {rate} H/s', { attempts: attempts.toLocaleString(), rate: Math.round(attempts * 1000 / Math.max(1, elapsedMs)) }));
      } });
      status(t('challenge.status.verifying', 'Verifying {algorithm} proof…', { algorithm: 'HashWX' }));
      const verified = await jsonFetch(endpoint('/api/verify'), { method: 'POST', body: JSON.stringify({ challenge_token: challenge.challenge_token, nonce: solution.nonce }) });
      await exchange(verified.receipt);
    } finally { removePagehide(); solver.cancel(); }
  }
  window.PowClient.register('hashwx', { run });
})();
