(() => {
  const algorithms = new Map();
  window.PowClient.register = (id, adapter) => {
    if (algorithms.has(id)) { throw new Error(`Duplicate solver adapter: ${id}`); }
    algorithms.set(id, adapter);
  };
  window.PowClient.algorithm = (id) => {
    const adapter = algorithms.get(id);
    if (!adapter) { throw new Error(`Unsupported PoW algorithm: ${id}`); }
    return adapter;
  };
})();
