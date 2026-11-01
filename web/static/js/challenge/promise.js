// Browser Promise compatibility implementation v1, Apache-2.0.
(function (scope) {
  // biome-ignore lint/suspicious/noRedundantUseStrict: this file is a classic script, not a module
  "use strict";
  if (typeof scope.Promise === "function") return;

  function settle(promise, state, value) {
    if (promise.state !== 0) return;
    promise.state = state;
    promise.value = value;
    flush(promise);
  }

  function adopt(promise, value) {
    var called;
    var then;
    if (value === promise) {
      settle(promise, 2, new TypeError("A promise cannot resolve itself"));
      return;
    }
    if (value && (typeof value === "object" || typeof value === "function")) {
      called = false;
      try {
        then = value.then;
        if (typeof then === "function") {
          then.call(
            value,
            function (result) {
              if (called) return;
              called = true;
              adopt(promise, result);
            },
            function (error) {
              if (called) return;
              called = true;
              settle(promise, 2, error);
            },
          );
          return;
        }
      } catch (error) {
        if (!called) settle(promise, 2, error);
        return;
      }
    }
    settle(promise, 1, value);
  }

  function flush(promise) {
    if (promise.state === 0 || promise.handlers.length === 0) return;
    var handlers = promise.handlers;
    promise.handlers = [];
    setTimeout(function () {
      handlers.forEach(function (handler) {
        var callback = promise.state === 1 ? handler.success : handler.failure;
        if (typeof callback !== "function") {
          (promise.state === 1 ? handler.resolve : handler.reject)(promise.value);
          return;
        }
        try {
          handler.resolve(callback(promise.value));
        } catch (error) {
          handler.reject(error);
        }
      });
    }, 0);
  }

  // biome-ignore lint/suspicious/noShadowRestrictedNames: the polyfill defines the global Promise
  function Promise(executor) {
    if (!(this instanceof Promise)) throw new TypeError("Promise requires new");
    if (typeof executor !== "function") throw new TypeError("Promise requires an executor");
    this.state = 0;
    this.value = undefined;
    this.handlers = [];
    var promise = this;
    var called = false;
    function resolve(value) {
      if (called) return;
      called = true;
      adopt(promise, value);
    }
    function reject(error) {
      if (called) return;
      called = true;
      settle(promise, 2, error);
    }
    try {
      executor(resolve, reject);
    } catch (error) {
      reject(error);
    }
  }

  // biome-ignore lint/suspicious/noThenProperty: the polyfill implements the Promise then method
  Promise.prototype.then = function (success, failure) {
    var promise = this;
    return new Promise(function (resolve, reject) {
      promise.handlers.push({
        success: success,
        failure: failure,
        resolve: resolve,
        reject: reject,
      });
      flush(promise);
    });
  };
  Promise.prototype.catch = function (failure) {
    return this.then(null, failure);
  };
  Promise.resolve = function (value) {
    if (value instanceof Promise) return value;
    return new Promise(function (resolve) {
      resolve(value);
    });
  };
  Promise.reject = function (error) {
    return new Promise(function (_, reject) {
      reject(error);
    });
  };
  Promise.all = function (values) {
    return new Promise(function (resolve, reject) {
      var remaining = values.length;
      var results = new Array(remaining);
      if (remaining === 0) {
        resolve(results);
        return;
      }
      values.forEach(function (value, index) {
        Promise.resolve(value).then(function (result) {
          results[index] = result;
          remaining -= 1;
          if (remaining === 0) resolve(results);
        }, reject);
      });
    });
  };
  Promise.race = function (values) {
    return new Promise(function (resolve, reject) {
      values.forEach(function (value) {
        Promise.resolve(value).then(resolve, reject);
      });
    });
  };
  scope.Promise = Promise;
})(typeof window !== "undefined" ? window : self);
