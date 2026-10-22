import(window.POW_SOLVER_MODULE_URL).then(
  (module) => window.PowClient.randomxLoaded?.(module),
  (error) => window.PowClient.randomxLoaded?.(null, error),
);
