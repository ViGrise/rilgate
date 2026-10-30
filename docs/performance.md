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

## Gateway with an HTTP null upstream, 2026-10-24

The performance gate uses the `null-upstream` Rust example. It runs on a
multithreaded Tokio runtime, consumes and discards incoming body frames, and
returns HTTP 200 with an empty body. These are `/dev/null` semantics through
HTTP; `/dev/null` itself cannot serve HTTP. An identifying response header lets
the gate check forwarding even when the response body is empty. The example
uses the existing Axum dependency and also handles streamed uploads.

The previous single-threaded Node.js upstream returned 1 KiB per request. The
new fixture changes both server implementation and response size, so these
results do not establish a gateway speedup over the historical measurements.

Measured on an Apple M4 Pro with 12 logical CPUs, macOS 26.4.1, Rust 1.98.0,
and the release profile. Each path uses `wrk` with four client threads,
two seconds of warmup and eight seconds of measurement. Gateway, upstream and
client share this machine and communicate over loopback HTTP/1.1. There were
no concurrent builds or profilers during the retained measurements. Proof API
quotas are disabled. The authenticated path uses a real Fast difficulty 1
cookie obtained through the proof protocol. The initial 12 / 12 runs at 32 and
128 connections used the default disabled metrics endpoint. Subsequent runs
enabled metrics collection, as the current harness does.

| Gateway / upstream workers | Connections | Allowed req/s | Authenticated req/s | Allowed p50 / p99, us | Gateway CPU, us/allowed request |
| --- | ---: | ---: | ---: | ---: | ---: |
| 12 / 12, first run | 32 | 66040 | 64395 | 461 / 690 | 73.38 |
| 12 / 12, repeat | 32 | 65334 | 65313 | 467 / 702 | 74.62 |
| 12 / 12 | 128 | 65373 | 65254 | 1900 / 2530 | 80.80 |
| 4 / 4 | 32 | 69332 | 68182 | 451 / 687 | 54.05 |
| 4 / 4 | 128 | 68926 | 69757 | 1830 / 3080 | 54.92 |
| 2 / 2 | 128 | 54983 | 51298 | 2300 / 3680 | 36.19 |
| 4 / 12 | 128 | 70439 | 70484 | 1800 / 2730 | 54.47 |

CPU cost is cumulative process user plus system time, divided by
completed requests. RSS snapshots are observations at measurement boundaries,
not peak memory measurements. For the 4 / 12 worker run:

| Path | Req/s | p50, us | p99, us |
| --- | ---: | ---: | ---: |
| Null upstream directly | 128263 | 910 | 1500 |
| Health | 136107 | 900 | 1740 |
| Challenge page | 138077 | 910 | 1680 |
| Authenticated proxy | 70484 | 1800 | 2960 |
| Allowed proxy | 70439 | 1800 | 2730 |

All seven retained runs passed the gate without measured HTTP or socket errors.
Six wrote zero log bytes during load. The repeated 12-worker, 32-connection run
wrote 94 bytes: one Pingora `failed to acquire reusable stream` event. Pingora
falls back to creating a connection on this path; that run still passed response
and error checks. In the 4 / 12 run, gateway RSS after allowed traffic was
41.23 MiB and upstream RSS was 9.30 MiB. Gateway CPU was about 384% of one core,
upstream CPU 190%, and client CPU 157% during that measurement.

### Bottleneck evidence

- **Transport and scheduling are the leading measured costs.** Three separate
  five-second `sample` profiles covered allowed traffic at 12 and 2 gateway
  workers and challenge traffic at 12 workers, with 128 connections. The leading
  non-waiting leaf stacks include `__sendto` and `__recvfrom`; runtime parking,
  wakeups, allocation and Pingora connection reuse are also visible. These are
  wall-clock samples, including idle threads, not CPU percentage attribution.
  Keeping the upstream at 12 workers and using four gateway workers reduced
  allowed CPU/request from 80.80 to 54.47 us, with throughput about 8% higher.
  This supports measuring runtime and transport changes before changing token
  validation. It does not prove a particular lock is responsible.
- **More concurrent connections add latency after saturation.** At 12 / 12
  workers, increasing connections from 32 to 128 left proxy throughput near
  65,000 req/s and increased allowed p50 from 461 to 1900 us. At two workers,
  gateway CPU reached about 199% of one core and proxy throughput fell. Four
  workers are a useful tuning candidate on this host, not a production default.
- **The upstream application is no longer the earlier single-threaded ceiling.**
  In the 4 / 12 run, the sink alone handled about 1.82 times allowed proxy
  throughput. The extra HTTP hop still consumes CPU and sockets on this host.
  Authenticated and allowed proxy rates were nearly equal. Challenge and health
  rates were also close. Neither authentication nor challenge generation is
  the leading throughput limit in this workload.
- **A 128-connection upstream keepalive pool exhausted ephemeral ports at 512
  connections.** Pingora's default `upstream_keepalive_pool_size` is 128 per
  server thread, and the gateway uses one server thread. With more concurrent
  proxied requests than idle pool slots, Pingora closes the surplus upstream
  connections after each request. Each close leaves a socket in TIME_WAIT for
  30 seconds on macOS, and the default ephemeral range has 16,384 ports. With
  the default pool, 512 connections failed the gate during authenticated proxy
  traffic with 413 client connect errors and 187 non-success responses. The
  gateway logged upstream `AddrNotAvailable` failures, macOS errno 49. Upstream
  TIME_WAIT sockets rose to 15,322 while about 300 upstream connections were
  established. Metrics recorded zero connection and request quota rejections.

### Upstream pool sized to the connection limit

The gateway now sizes the upstream keepalive pool to `server.max_connections`,
default 1024. The earlier tables in this section used Pingora's default pool.
Rerun with default workers (12 / 12), four `wrk` threads, two
seconds of warmup and eight seconds of measurement:

| Connections | Allowed req/s | Authenticated req/s | Allowed p50 / p99, us | Gateway CPU, us/allowed request | Peak upstream TIME_WAIT | Peak upstream established |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 128 | 64394 | 64509 | 1920 / 2880 | 86.22 | 30 | 129 |
| 512 | 63253 | 63206 | 7900 / 13080 | 89.91 | 149 | 513 |

Both runs passed the gate with zero log bytes. At 512 connections the gateway
reused one upstream connection per downstream connection, and proxy throughput
stayed at the 128-connection level. Latency grows with queue depth because
throughput is already saturated. At 128 connections, CPU per request did not
improve against the 80.80 us default-pool run; connection churn is not a
measurable cost at that concurrency on this host. Gateway RSS after allowed
traffic was 31.88 MiB at 128 connections and 56.39 MiB at 512.

Prioritize a benchmark on the deployment Linux host with the generator and sink
on separate hosts, then compare gateway worker counts while holding upstream
workers fixed. Profile Pingora's connection reuse and Tokio task wakeups there
before changing production transport or synchronization. This run does not
measure TLS, HTTP/2, large response bodies, rate limiter contention, or PoW
verification capacity.

Build and reproduce the 4 / 12 configuration:

```bash
scripts/build.sh --release
cargo build --release --locked -p rilgate-gateway --example null-upstream
TOKIO_WORKER_THREADS=4 PERF_GATE_UPSTREAM_WORKERS=12 \
  PERF_GATE_THREADS=4 PERF_GATE_CONNECTIONS=128 \
  PERF_GATE_DURATION=8s PERF_GATE_WARMUP=2s \
  PERF_GATE_RUNDIR=/tmp/rilgate-perf-reproduction scripts/perf-gate.sh
```

The gate keeps raw `wrk` output, client CPU time, cumulative server CPU/RSS
snapshots and metrics counters per measured path in `PERF_GATE_RUNDIR`. These
results are not committed.

## Linux container measurements, 2026-10-24

Measured in the Docker Desktop Linux VM on the same Apple M4 Pro: kernel
7.0.14-linuxkit, arm64, 12 vCPUs and 7.7 GiB of memory. The image is Debian trixie
with Rust 1.99.1 and `wrk` 4.1.0 using epoll. Binaries use the release profile and
default Tokio worker counts. The ephemeral port range is 32768-60999 and
`tcp_tw_reuse` is 2. Gateway, `null-upstream` and `wrk` share the VM and
communicate over loopback HTTP/1.1. Each path uses four `wrk` threads, two seconds
of warmup and eight seconds of measurement. Gateway CPU is user plus system time
read from procfs, divided by completed requests.

### Upstream pool on Linux

The same working tree was built with the upstream pool sized to
`server.max_connections` and with Pingora's default pool of 128. Each run waited
65 seconds before it started, so TIME_WAIT sockets from earlier runs had expired.

| Pool | Connections | Allowed req/s | Authenticated req/s | Allowed p50 / p99, us | Gateway CPU, us/allowed request | Peak upstream TIME_WAIT |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `max_connections` | 128 | 107695 | 115494 | 1150 / 2170 | 73.32 | 40 |
| Default 128 | 128 | 105394 | 115837 | 1180 / 2220 | 74.77 | 59 |
| `max_connections` | 512 | 104385 | 112043 | 4640 / 11160 | 73.61 | 406 |
| Default 128 | 512 | 105605 | 110816 | 4590 / 11120 | 72.75 | 13614 |

All four runs passed the gate. Throughput, latency and CPU per request are
equivalent within run-to-run variation. With the default pool at 512 connections,
upstream TIME_WAIT sockets reached 13,614, about 48% of the ephemeral range,
within one eight-second measurement. The wider Linux range delays the address
exhaustion measured on macOS but does not prevent it under sustained churn. The
sized pool kept the peak at 406. The null upstream alone served 448,000 to 705,000
req/s, so it does not limit these results.

Pingora logs `failed to acquire reusable stream` at ERROR level two or three
times per run with either pool size. The event occurs when a request takes a
pooled connection while its idle watcher still holds the connection. Pingora
then opens a new connection, and the response checks pass. These lines are the
127 to 381 log bytes the gate reported.

### Axum and Pingora transport comparison

Three gateway builds served the same null upstream:

- Axum: commit `d88eaa8^`, the last revision with an Axum server and a Reqwest
  upstream client.
- Pingora at migration: commit `d88eaa8`, which uses the same configuration
  schema as `d88eaa8^`.
- Pingora current: the working tree with the sized upstream pool. It uses the
  current configuration schema with an operations listener.

The order was Axum, migration, current, current, migration, Axum for each
connection count. Values are means of the two runs per build. All 36
measurements completed without socket errors or non-2xx responses. The proxy
path is `/public`, allowed without a challenge. The challenge path is
`/protected`, a local Fast challenge page. Both use a browser User-Agent.

| Connections | Path | Axum req/s | Pingora at migration req/s | Pingora current req/s | Gateway CPU, us/request: Axum / migration / current |
| ---: | --- | ---: | ---: | ---: | --- |
| 32 | proxy | 86016 | 88427 | 94568 | 68.95 / 70.61 / 68.78 |
| 128 | proxy | 114617 | 107239 | 107958 | 68.80 / 72.88 / 72.46 |
| 512 | proxy | 119754 | 105837 | 104155 | 69.09 / 73.80 / 75.32 |
| 32 | challenge | 128246 | 176477 | 184132 | 67.87 / 36.21 / 37.33 |
| 128 | challenge | 130595 | 203833 | 190269 | 71.02 / 36.56 / 39.92 |
| 512 | challenge | 120670 | 247564 | 204464 | 74.23 / 30.97 / 38.73 |

- **Proxy throughput depends on concurrency.** The Axum build forwarded 6% to 7% more
  requests at 128 connections and 13% to 15% more at 512, with 4 to 6 us less gateway
  CPU per request. At 32 connections, both Pingora builds were 3% to 10% faster.
  The 10% to 12% Pingora deficit measured on macOS loopback appears on Linux only
  at higher concurrency. These runs do not separate the server from the upstream
  client, because the Axum build also uses Reqwest.
- **Challenge results do not compare transports.** Commit `d88eaa8` also
  contains web and token optimizations that `d88eaa8^` lacks. The Axum build
  spent about 70 us per challenge page, close to its proxy cost, so application
  work dominates that difference.
- **Current challenge throughput is lower than at migration at 512
  connections.** The current build served 17% fewer challenge pages and used
  25% more CPU per page than `d88eaa8`. Both runs of each build agree. The cause
  is not established: commits after the migration and the different
  configuration schema both change this path. Bisect it with one configuration
  before you attribute it to code.
- **Memory at 512 connections.** Gateway RSS after the challenge measurement was
  53.1 to 56.6 MiB for Axum, 40.5 to 42.2 MiB for Pingora at migration and 44.8
  to 44.9 MiB for current Pingora.
- p99 latency varied between repeated runs, for example 2.1 ms and 9.7 ms for the
  same build. Use p50 and throughput from repeated runs for comparison in this
  shared VM.

The authenticated path is not part of the comparison, because the proof protocol
used to get a cookie changed after the migration. These results come from one
VM that runs the generator, gateway and upstream together. They do not replace
a benchmark on the deployment host with separate generator and upstream hosts.

## Global allocator, 2026-10-24

The `rilgate` binary uses mimalloc as its global allocator. mimalloc keeps
per-thread heaps, so request handling on many Tokio workers does not contend on
one allocator lock. The comparison uses the same source and the Linux container
described above. Each build ran in the order A, B, C, C, B, A for each
connection count. Runs that failed the gate because `wrk` reported timeouts are
excluded. Those timeouts occurred with every build and on every path, including
health, and come from the shared VM.

| Connections | Metric | glibc malloc | glibc and mimalloc | Static musl and mimalloc |
| ---: | --- | ---: | ---: | ---: |
| 128 | Allowed proxy req/s | 134514 | 199837 | 201462 |
| 128 | Authenticated proxy req/s | 139186 | 193972 | 195282 |
| 128 | Gateway CPU, us/allowed request | 56.90 | 37.87 | 37.39 |
| 512 | Allowed proxy req/s | 124055 | 223017 | 224476 |
| 512 | Gateway CPU, us/allowed request | 60.01 | 35.96 | 35.50 |
| 512 | Challenge page req/s | 434647 | 492020 | - |
| 128 / 512 | Gateway RSS after allowed traffic, MiB | 31 / 47 | 55 / 67 | 52 / 64 |

The 512-connection musl values come from an earlier session of the same
comparison, because both musl runs in this session failed with `wrk` timeouts.

- mimalloc increased allowed proxy throughput by 49% at 128 connections and 80%
  at 512, and reduced gateway CPU per allowed request by 33% to 40%. Allocation
  on the proxy path was the largest measured cost in this workload. It is larger
  than the transport difference between Axum and Pingora above, which used the
  system allocator.
- glibc and musl builds with mimalloc have the same throughput and CPU cost
  within run-to-run variation. A static musl build that keeps the musl
  allocator served about 13,500 allowed proxy req/s at 128 connections, about
  one eighth of the glibc build in the same session. Static musl builds need
  mimalloc.
- Health and challenge throughput changed by -4% to +13%. These paths allocate
  less per request than proxying.
- On Linux, mimalloc increases RSS by about 20 to 24 MiB at these connection
  counts.

On macOS with the system allocator as baseline, in the order A, B, B, A:

| Connections | Allocator | Allowed proxy req/s | Gateway CPU, us/allowed request | Gateway RSS after allowed traffic, MiB |
| ---: | --- | ---: | ---: | ---: |
| 128 | System | 80272 | 72.86 | 37.6 |
| 128 | mimalloc | 81089 | 64.73 | 51.8 |
| 512 | System | 79006 | 75.30 | 73.9 |
| 512 | mimalloc | 78910 | 70.21 | 63.5 |

On macOS, mimalloc reduced CPU per allowed request by 7% to 11% without a
throughput change. RSS increased by 14 MiB at 128 connections and decreased by
10 MiB at 512. Loopback throughput on macOS stays near 80,000 req/s with
both allocators.

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

## RandomX browser measurements, 2026-10-30

The desktop is an Apple M4 Pro with 12 logical CPUs, running Chrome 156.0.8078.76
on macOS. Chrome reports `hardwareConcurrency = 12` and `deviceMemory = 16` GiB.
The mid-range phone is a Samsung Galaxy A17 with Chrome 156.0.8078.47, reporting
8 logical CPUs and 8 GiB. These results describe these devices and browser
versions; they are not capacity promises for other devices.

The single-worker baseline uses commit `8bb4481afcb91b2a61d705f3960f97dac2a35316`.
Parallel measurements use `f86309f`; `01ed64c` changes documentation and memory
disclosure wording without changing hashing. The release confirmation uses this
change's working tree, based on `f822bd9` with the parallel changes and automatic
cap 8. Its browser hashing code matches the release commit containing this section.

The parallel rate trials use a development-only RandomX policy at difficulty
1000000. Each configuration has two runs. Each run clears cookies and origin
storage, starts after explicit consent, and reads the cumulative summed rate at
least 45 seconds after the first progress status. Polling is approximately
100 ms. Setup runs measure the interval from the difficulty status to the first
progress status. Cold setup clears storage; warm setup preserves the stored entry
and creates new workers. Builds and tests complete before rate measurements.
The earlier desktop and phone series ran concurrently; release confirmation runs
only on the desktop. The phone was not re-measured for the cap change.

The earlier single-worker baseline was 28.76 H/s on desktop and 5.71 H/s on the
phone, with approximately 1.0 s and 4.6 s setup. Baseline desktop proof trials
reached an upstream error page; the rates are observations before that error,
not successful end-to-end solve times. The following parallel tables use their
own single-worker runs for speedup comparisons.

### Desktop worker counts

| Configuration | Run 1 H/s | Run 2 H/s | Mean H/s | Speedup vs 1 worker |
| --- | ---: | ---: | ---: | ---: |
| 1 worker | 29.33 | 29.25 | 29.290 | 1.00x |
| 2 workers | 61.51 | 61.17 | 61.340 | 2.09x |
| 4 workers | 120.21 | 120.57 | 120.390 | 4.11x |
| 8 workers | 202.10 | 200.68 | 201.390 | 6.88x |
| Automatic, earlier cap 4 | 118.12 | 117.56 | 117.840 | 4.02x |
| Scalar, automatic with cap 4 | 52.82 | 53.01 | 52.915 | 1.81x |
| Automatic, release cap 8 | 200.17 | 201.27 | 200.720 | 6.85x |

Both release runs used automatic N = 8. A CDP target inventory during run 2
confirmed 8 live workers. Their hashing windows were
45.101 s and 45.060 s. No worker startup or allocation
failures were observed in the earlier or release desktop trials.

### Galaxy A17 worker counts

| Configuration | Run 1 H/s | Run 2 H/s | Mean H/s | Speedup vs 1 worker |
| --- | ---: | ---: | ---: | ---: |
| 1 worker | 6.32 | 6.48 | 6.400 | 1.00x |
| 2 workers | 12.41 | 12.55 | 12.480 | 1.95x |
| 4 workers | 16.10 | 16.05 | 16.075 | 2.51x |
| Automatic, 4 workers | 16.22 | 15.92 | 16.070 | 2.51x |
| Scalar, automatic, 4 workers | 10.86 | 10.91 | 10.885 | 1.70x |

The Galaxy A17 gains 1.95x with 2 workers and 2.51x with 4 workers.
Its automatic worker count stays 4 with cap 8. No worker startup or allocation
failures were observed. Battery temperature before the trials ranged from
32.4 to 33.0 degrees C.

### Stored cache setup and solve estimates

These setup measurements use the earlier automatic configuration: 4 workers on
both devices. Each cold and warm setup measurement is one run, with one stored
IndexedDB entry. The scalar rate rows also use the earlier automatic count of 4;
scalar throughput with 8 automatic desktop workers was not measured.

| Device | Cold setup, s | Warm setup, s |
| --- | ---: | ---: |
| M4 Pro | 1.0156 | 0.8135 |
| Galaxy A17 | 7.1469 | 4.9551 |

At difficulty 1000, expected hashing time is difficulty divided by measured
throughput. The release desktop automatic estimate is `1000 / 200.720 = 4.98 s`; the phone automatic estimate is `1000 / 16.07 = 62.23 s`.
These estimates exclude setup, networking and verification. Individual solve
times vary because finding a valid nonce is probabilistic.

See [RandomX throughput](pow.md#randomx-throughput) for worker selection, cache
storage and browser requirements. Raw results remain outside the repository in
`/tmp/rilgate-randomx-parallel/`, with release confirmation in its `release/`
subdirectory. Generated benchmark secrets are deleted during cleanup.
