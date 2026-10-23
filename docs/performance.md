# Performance measurements

## Findings

Generated asset responses copied the complete payload on every GET, and their
constructor also cloned the input vector. These assets now use `bytes::Bytes`:
construction takes ownership of the vector, and response bodies share its immutable
storage. The explicit `AssetMetadata::bytes()` API still returns an owned vector. Embedded assets
already used static storage and remain unchanged.

Filesystem overrides are copied into immutable backing files when a web snapshot is
built. `ReaderStream::new` used its default 4 KiB buffer to serve those files. This produced a
separate asynchronous read and body frame per chunk. The read buffer now scales with
file size up to 64 KiB. This reduces blocking file work and response frame overhead, and
keeps file bodies streamed and independent of later changes to the operator's source
files.

The buffer can use up to 60 KiB more per active file response than the previous default,
plus Tokio's file buffering. Small files use their own size, with a minimum capacity of
one byte for empty files. HEAD and conditional 304 responses do not open a file or
allocate this read buffer.

These choices follow the documented behavior of [Bytes shared storage](https://docs.rs/bytes/latest/bytes/struct.Bytes.html) and
[ReaderStream capacity](https://docs.rs/tokio-util/latest/tokio_util/io/struct.ReaderStream.html). `AssetSource::Memory` now holds `Bytes`. It previously
held `Vec<u8>`. Direct enum constructors need `.into()`. `AssetMetadata::memory` retains its
existing API.

## Local measurements

Measured on macOS arm64 using the release profile on 2026-10-07. Each entry is the
median of five samples, with 200 sequential responses per sample. The benchmark
constructs the response and drains its body without a network stack. File results use
warm filesystem caches. The baseline includes the proxy edits already present in the
working tree, which this benchmark does not exercise.

| Source | Size | Before, us/response | After, us/response | Frames before / after |
| --- | ---: | ---: | ---: | ---: |
| Memory | 4 KiB | 0.27 | 0.23 | 1 / 1 |
| Memory | 140 KiB | 1.68 | 0.17 | 1 / 1 |
| Memory | 1 MiB | 12.56 | 0.19 | 1 / 1 |
| File | 4 KiB | 35.92 | 32.26 | 1 / 1 |
| File | 140 KiB | 271.05 | 61.08 | 35 / 3 |
| File | 1 MiB | 1890.62 | 167.25 | 256 / 16 |

Run the same benchmark before and after a candidate change:

```bash
cargo run --release --locked -p rilgate-web --example asset-throughput -- 200
```

These are local body-drain timings, not gateway throughput or browser load-time claims.
In the current snapshot builder, the generated RandomX solver module uses memory
storage. Filesystem overrides use file storage.

The larger memory cases characterize the asset API. They do not characterize the
shipping solver module size. Concurrency, network backpressure, cold disks, and
deployment hardware can change the results. Use `scripts/perf-gate.sh` for the existing gateway
health and challenge page load checks, plus allowed and authenticated proxy traffic. It
does not measure static assets.

## Policy and authentication gate

Cookie lookup returns a slice of the request header. It does not allocate an owned
value. Policy decisions borrow the matched rule name from the compiled engine. The
default allow name is static. Presentation issuance still owns the rule name in its
signed claims. Cookie parsing order, policy ordering, and token validation remain the
same.

The release gate benchmark uses a valid Fast access token, two Cookie headers, and the
full `gate` function. It warms each path with 1,000 calls and reports the median
of five samples of 200,000 requests. Allocation counts are collected in a separate
1,000-call sample. The counting flag is disabled for timing.

| Path | Allocations before / after | Before, ns/request | After, ns/request |
| --- | ---: | ---: | ---: |
| Allowed | 1 / 0 | 17.46 | 10.70 |
| Authenticated | 5 / 3 | 953.94 | 930.79 |

These measurements were taken locally on macOS arm64 on 2026-10-07. They exclude HTTP
transport and upstream forwarding. The authenticated timing difference is small and
sensitive to system noise. The confirmed result is removal of two allocations per
authenticated request. The table uses runs without concurrent builds.

```bash
cargo run --release --locked -p rilgate-gateway --example gate-throughput -- 200000
```

`PolicyDecision` now has a lifetime parameter and a borrowed `rule: &str`. Code that retains
a rule beyond the engine's lifetime must take an owned copy. `cookie_value` now returns
`Option<&str>` tied to the headers. Callers that need ownership can use `.map(str::to_owned)`.

The HTTP performance gate gets its cookie through presentation, challenge, verification,
and exchange endpoints with Fast difficulty 1. It checks the authenticated response
against the mock upstream before and after load. It reports authenticated and allowed
proxy throughput alongside health and challenge page metrics. Authentication failures
during the run fail the gate.

A local smoke run with 32 connections, one-second warmups, and three-second measurement
windows passed: authenticated proxy throughput was 69,801 req/s with p50 447 us. Allowed
proxy throughput was 70,389 req/s with p50 444 us. There were no HTTP or socket errors
and no measured log growth. These numbers validate the load harness and are not a
throughput comparison before and after the change.

## Client connection writes

The gateway accepts connections itself and gives Pingora streams with unbuffered writes.
Thus, a local response cost one `sendto` for its header and another for its body.
`DownstreamStream` wraps each client connection and coalesces writes while `send` writes
a local response. When Pingora flushes at the end of the response, the header and a body
of up to 8 KiB leave in one write. The buffer exists only during that write and is
released on flush, so idle keep-alive connections hold no extra memory.

Proxied responses are written through unchanged. A buffer for these responses would hold
a fixed-length response header until the upstream sends body bytes, which delays headers
behind a slow upstream. Pingora's own listener accepts that delay. The gateway requires
proxied headers to reach the client first.

Request IDs use UUIDv7 random bits. The `uuid` crate's `fast-rng` feature draws
them from `rand`'s thread-local CSPRNG, which removes a `getentropy` system call
per request. Random UUIDs elsewhere, including SHA-256 challenge values, use the same
generator.

`scripts/perf-gate.sh`, 32 connections and eight-second windows, mean of two runs on macOS arm64
on 2026-10-07:

| Path | Before, req/s | After, req/s | p50 before / after |
| --- | ---: | ---: | ---: |
| Health | 131,289 | 169,060 | 225 / 165 us |
| Challenge page | 131,675 | 168,619 | 224 / 164 us |
| Authenticated proxy | 69,784 | 69,853 | 448 / 445 us |
| Allowed proxy | 70,556 | 70,884 | 443 / 441 us |

Proxy throughput is bounded by the single-threaded mock upstream and the added hop's
latency. Gateway CPU per proxied request changed from 62.8 us to 60.7 us, which is
within run-to-run variation.

## Further investigation

The authenticated path still allocates for the normalized request host, decoded token
payload, and deserialized host claim. These need further profiling before changes to
their ownership or validation. The rate limiter serializes token updates under one
mutex. This preserves its global quota. Before you change its synchronization, measure
contention. RandomX has its own throughput benchmark and reference vectors, and requires
separate analysis from HTTP response performance.

## HashWX measurements

Measured on 2026-10-23 with three seeds per row and one worker. The Mac is an
Apple M4 Pro, arm64, running macOS 26.4.1. Chrome 155.0.8059.86 ran headless;
Safari 26.4 ran in its application. Native results use Rust 1.98.0 and the
release profile. The phone is an iPhone 15 Pro Max. Its user agent reports
Safari 17.5, iOS 17.5.1 and hardware concurrency 4.

These rates use the prototype artifact recorded in proposal section 8.
Rates below include per-seed generation and compilation at 65536 nonces per
seed. They exclude initial WASM loading, SHA-256 challenge seed derivation,
network traffic and token verification. The native path uses interpretation.
The browser generated path compiles the generated WASM. A native code generator
was not measured.

| Runtime | Interpreter total H/s | Generated WASM total H/s |
| --- | ---: | ---: |
| M4 Pro, native | 71224 | Not measured |
| M4 Pro, Chrome | 64997 | 323475 |
| M4 Pro, Safari | 69917 | 306721 |
| iPhone 15 Pro Max, Safari | 62814 | 386263 |

Native instance generation plus one hash averaged 25.9 microseconds. This
kernel measurement excludes the proof API, seed derivation, signature checks
and blocking-task scheduling. It does not establish server request latency.
The native interpreter / iPhone generated-WASM rate ratio is 0.184; the
native interpreter / iPhone interpreter ratio is 1.134. These compare different
machines. They do not measure a native JIT or GPU advantage.

The iPhone 15 Pro Max is high-end. It does not satisfy the mid-range phone
requirement. The default base of 50000 and max of 5000000 stay placeholders
until that measurement is available. The native-to-mid-range-phone ratio
remains pending.

Three trials per difficulty give a wide solve-time spread. On the iPhone at
50000 difficulty and 463 nonces per seed, generated trials were 20663, 310 and
589 ms. Hash rates are the reliable figures for these samples. Use more trials
on a mid-range phone before selecting defaults. The prototype uses deterministic
synthetic seeds; the production solver uses the signed SHA-256 seed scheme
and up to two workers. Prototype solve medians are not production medians.

See [proposal section 8](proposals/browser-pow-algorithms.md#8-measurement-before-defaults)
for full rates, individual-result file names, solve medians and benchmark
commands. Result JSON stays in the ignored `target/hashwx-bench/` directory.
