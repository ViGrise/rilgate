# Web runtime and browser RandomX adapter

The `rilgate-web` crate implements the embedded bundle and filesystem overrides,
MiniJinja rendering, key-level translations, locale negotiation, safe seeding,
immutable snapshots, optional watching, manual reloads, and web CLI commands.
See the [web resource guide](../../docs/web.md)
for configuration and deployment instructions.

Gateway APIs and assets live below the configurable `web.base_path`.
Challenge, denied, and upstream error pages are rendered at the application URL.

Default:

```text
/.well-known/rilgate
```

The default solver module therefore resolves to:

```text
/.well-known/rilgate/static/js/solvers/randomx/solver.js
```

Configure the relative path in `config/rilgate.yml`:

```yaml
web:
  base_path: "/.well-known/rilgate"

pow:
  browser:
    solver_module_path: "/static/js/solvers/randomx/solver.js"
```

## Modules

```text
static/js/solvers/randomx/solver.js          ES module loaded by challenge.js
static/js/solvers/randomx/randomx-worker.js  classic Web Worker that owns the WASM instance
static/js/solvers/randomx/randomx_wasm.wasm  randomx-core compiled to wasm32
```

`solver.js` resolves the worker and the WASM module relative to its own URL, so
the trio stays together regardless of `web.base_path`.

`randomx_wasm.wasm` is produced by `cargo xtask build-wasm` and embedded with
the rest of `web/` through `include_dir!`, keeping deployment to a single
binary. Rebuild the module and gateway after changing `crates/randomx-core`
or `crates/randomx-wasm`:

```bash
rustup target add wasm32-unknown-unknown
cargo xtask build-wasm
cargo build --release
```

## Module contract

```js
export async function solve(job) {
  // job.blob        : P2Pool hashing blob, hex
  // job.seedHash    : RandomX seed, hex
  // job.difficulty  : selected fixed difficulty
  // job.nonceOffset : normally 39
  // job.expiresAt   : local challenge expiry, Unix seconds
  // job.onProgress  : optional progress callback
  return {
    nonce: "00000000", // Numeric hexadecimal nonce.
    result: "00".repeat(32),
  };
}
```

`p2pool_accepted` needs both `nonce` and `result` because the gateway forwards
the share through the active P2Pool Stratum session.

`local_verify` only requires the nonce for trust purposes: the Rust gateway
reconstructs the blob and recomputes RandomX natively with the same
`randomx-core` code the browser runs.

Starting a new `solve()` while one is in flight cancels the previous attempt and
rejects it with an error named `SupersededError`; `challenge.js` treats that as
a normal job rotation rather than a challenge failure.

## WASM ABI

```text
rx_create(seed_ptr, seed_len) -> handle   handle >= 0, or -1 on failure
rx_destroy(handle) -> status
rx_hash(handle, input_ptr, input_len, out_ptr) -> status
rx_hash_pair(handle, first_ptr, second_ptr, input_len, out_ptr) -> status
rx_hash_triple(handle, input_ptr, input_len, out_ptr) -> status
rx_dataset_module_ptr(handle) -> pointer
rx_dataset_module_len(handle) -> byte length
rx_set_dataset_function(handle, table_index) -> status
rx_set_dataset_pair_function(handle, table_index) -> status
rx_set_dataset_triple_function(handle, table_index) -> status
rx_dataset_item(handle, item_u64, out_ptr) -> status   interpreted oracle
```

`cargo xtask build-wasm` exports a growable `__indirect_function_table`. The worker
compiles the Rust-generated dataset module with the core's `memory`, installs its
`dataset_item`, `dataset_pair` and `dataset_triple` functions in that table, and registers their
indices with the handle. Three table slots are reused across seed changes.
Index zero restores interpretation. The generated module belongs to one cache
and must be replaced when the seed changes. Its byte buffer is valid until the
handle is destroyed; callers copy it before asynchronous compilation. Dataset
output is eight u64 words at an eight-byte-aligned writable address. The paired
kernel receives two item indices and two output addresses. `rx_hash_pair` receives
two equally sized inputs and writes their hashes consecutively to 64 bytes.
The triple kernel receives an address containing three u64 indices and an address
for three consecutive eight-word outputs. `rx_hash_triple` reads equally sized
inputs at `input_ptr`, `input_ptr + 256` and `input_ptr + 512`, then writes 96 output
bytes. The maximum input length is 256 bytes.

The worker checks all three consecutive nonce results in order and yields between
batches. One cache is shared by three independent VMs, adding about 4 MiB of
scratchpad memory relative to a single VM. The final nonce is hashed singly,
keeping every nonce within the 32-bit range. Failed kernel installation retains single-hash execution.
The built WASM artifact requires SIMD128 support.

The wrapper uses Rust's wasm32 standard library and exposes a raw C-style ABI.
Callers pass pointers into the exported linear memory; no `wasm-bindgen`
JavaScript glue is required. Fallible status exports return `0` on success
and `-1` on failure.

`cancel()` terminates the worker, releases its WASM memory, and rejects pending
work with `AbortError`. The challenge page calls it when work finishes or the
page leaves. The page waits for a click before allocating the RandomX cache,
and P2Pool mode discloses that browser work is submitted as mining work.

Local RandomX jobs carry `expires_at` in Unix seconds. The challenge page passes
it as `expiresAt` to the solver, which refuses expired work before cache setup
and before and after hashing. Difficulty may be a number or a decimal string;
strings preserve values above JavaScript's safe integer range. Local blobs
include a unique challenge suffix and are not valid P2Pool mining submissions.

The Rust WASM core uses exact directed SIMD rounding, public-state AES lookup
tables and inlined SuperscalarHash kernels. Browser throughput depends on the
device, browser and workload. See [RandomX throughput](../../docs/pow.md#randomx-throughput)
for arithmetic validation and native throughput commands.
