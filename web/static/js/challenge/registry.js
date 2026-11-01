(() => {
  const algorithms = Object.create(null);
  window.PowClient.register = (id, adapter) => {
    // biome-ignore lint/suspicious/noPrototypeBuiltins: Object.hasOwn is unavailable in legacy browsers
    if (Object.prototype.hasOwnProperty.call(algorithms, id)) {
      throw new Error(`Duplicate solver adapter: ${id}`);
    }
    algorithms[id] = adapter;
  };
  window.PowClient.algorithm = (id) => {
    const adapter = algorithms[id];
    if (!adapter) {
      throw new Error(`Unsupported PoW algorithm: ${id}`);
    }
    return adapter;
  };
})();
