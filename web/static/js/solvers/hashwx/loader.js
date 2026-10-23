import('./solver.js').then(
  (module) => window.PowClient.hashwxLoaded?.(module),
  (error) => window.PowClient.hashwxLoaded?.(null, error),
);
