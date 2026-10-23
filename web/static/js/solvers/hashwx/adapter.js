(() => {
  const { t, status, endpoint, jsonFetch, exchange, requestChallenge, progressEl, once } = window.PowClient;
  function loadSolver() {
    if (typeof Worker !== 'function' || typeof WebAssembly !== 'object' ||
        typeof WebAssembly.instantiate !== 'function') {
      return Promise.reject(new Error(t('challenge.hashwx_unsupported', 'This browser cannot run HashWX. WebAssembly and workers are required.')));
    }
    return new Promise((resolve, reject) => {
      const script = document.createElement('script');
      const timer = setTimeout(() => finish(new Error('HashWX module load timed out')), 30000);
      function finish(error, module) {
        clearTimeout(timer);
        script.onerror = null;
        if (script.parentNode) script.parentNode.removeChild(script);
        delete window.PowClient.hashwxLoaded;
        if (error) reject(error);
        else if (!module || typeof module.solve !== 'function') reject(new Error('Invalid HashWX solver module'));
        else resolve(module);
      }
      window.PowClient.hashwxLoaded = (module, error) => finish(error, module);
      script.type = 'module';
      script.src = endpoint('/static/js/solvers/hashwx/loader.js');
      script.onerror = () => finish(new Error('HashWX solver module failed to load'));
      document.head.appendChild(script);
    });
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
