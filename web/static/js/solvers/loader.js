const parameters = new URL(import.meta.url).searchParams;
const request = parameters.get('request');
import(parameters.get('module')).then(
  module => window.PowClient.solverLoaded(request, module),
  error => window.PowClient.solverLoaded(request, null, error),
);
