# Performance measurements

## Findings

Generated asset responses copied the complete payload on every GET, and their
constructor also cloned the input vector. These assets now use `bytes::Bytes`:
construction takes ownership of the vector, and response bodies share its
immutable storage. The explicit `AssetMetadata::bytes()` API still returns an
owned vector. Embedded assets already used static storage and remain unchanged.

Filesystem overrides are copied into immutable backing files when a web snapshot
is built. Serving those files through `ReaderStream::new` used its default 4 KiB
buffer, producing a separate asynchronous read and body frame per chunk.
The read buffer now scales with file size up to 64 KiB. This reduces blocking
file work and response frame overhead, while keeping file bodies streamed and
independent of later changes to the operator's source files.

The buffer can use up to 60 KiB more per active file response than the previous
default, plus Tokio's file buffering. Small files use their own size, with a
minimum capacity of one byte for empty files. HEAD and conditional 304 responses
do not open a file or allocate this read buffer.

These choices follow the documented behavior of
[Bytes shared storage](https://docs.rs/bytes/latest/bytes/struct.Bytes.html)
and [ReaderStream capacity](https://docs.rs/tokio-util/latest/tokio_util/io/struct.ReaderStream.html).
`AssetSource::Memory` now holds `Bytes` instead of `Vec<u8>`; direct enum
constructors need `.into()`. `AssetMetadata::memory` retains its existing API.

## Local measurements

Measured on macOS arm64 using the release profile on 2026-10-07. Each entry is
the median of five samples, with 200 sequential responses per sample. The
benchmark constructs the response and drains its body without a network stack.
File results use warm filesystem caches. The baseline includes the proxy edits
already present in the working tree, which this benchmark does not exercise.

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

These are local body-drain timings, not gateway throughput or browser load-time
claims. In the current snapshot builder, the generated RandomX solver module
uses memory storage; filesystem overrides use file storage. The larger memory
cases characterize the asset API rather than the shipping solver module size.
Concurrency, network backpressure, cold disks, and deployment hardware can
change the results. Use `scripts/perf-gate.sh` for the existing gateway health
and challenge page load checks, plus allowed and authenticated proxy traffic;
it does not measure static assets.

## Policy and authentication gate

Cookie lookup returns a slice of the request header instead of allocating an
owned value. Policy decisions borrow the matched rule name from the compiled
engine; the default allow name is static. Presentation issuance still owns the
rule name in its signed claims. Cookie parsing order, policy ordering, and token
validation remain the same.

The release gate benchmark uses a valid Fast access token, two Cookie headers,
and the full `gate` function. It warms each path with 1,000 calls and reports the
median of five samples of 200,000 requests. Allocation counts are collected in
a separate 1,000-call sample; the counting flag is disabled for timing.

| Path | Allocations before / after | Before, ns/request | After, ns/request |
| --- | ---: | ---: | ---: |
| Allowed | 1 / 0 | 17.46 | 10.70 |
| Authenticated | 5 / 3 | 953.94 | 930.79 |

These measurements were taken locally on macOS arm64 on 2026-10-07. They exclude
HTTP transport and upstream forwarding. The authenticated timing difference is
small and sensitive to system noise; the confirmed result is removal of two
allocations per authenticated request. The table uses runs without concurrent
builds.

```bash
cargo run --release --locked -p rilgate-gateway --example gate-throughput -- 200000
```

`PolicyDecision` now has a lifetime parameter and a borrowed `rule: &str`.
Code retaining a rule beyond the engine's lifetime must take an owned copy.
`cookie_value` now returns `Option<&str>` tied to the headers; callers needing
ownership can use `.map(str::to_owned)`.

The HTTP performance gate obtains its cookie through presentation, challenge,
verification, and exchange endpoints with Fast difficulty 1. It checks the
authenticated response against the mock upstream before and after load, and
reports authenticated and allowed proxy throughput alongside health and
challenge page metrics. Authentication failures during the run fail the gate.

A local smoke run with 32 connections, one-second warmups, and three-second
measurement windows passed: authenticated proxy throughput was 69,801 req/s
with p50 447 us; allowed proxy throughput was 70,389 req/s with p50 444 us.
There were no HTTP/socket errors and no measured log growth. These numbers
validate the load harness and are not a before/after throughput comparison.

## Client connection writes

The gateway accepts connections itself and hands Pingora streams with
unbuffered writes, so a local response cost one `sendto` for its header and
another for its body. `DownstreamStream` wraps each client connection and
coalesces writes while `send` writes a local response: the header and a body of
up to 8 KiB leave in one write when Pingora flushes at the end of the response.
The buffer exists only during that write and is released on flush, so idle
keep-alive connections hold no extra memory.

Proxied responses are written through unchanged. Buffering them would hold a
fixed-length response header until the upstream sends body bytes, which delays
headers behind a slow upstream. Pingora's own listener accepts that delay; the
gateway requires proxied headers to reach the client first.

Request IDs use UUIDv7 random bits. The `uuid` crate's `fast-rng` feature draws
them from `rand`'s thread-local CSPRNG, which removes a `getentropy` system
call per request. Random UUIDs elsewhere, including SHA-256 challenge values,
use the same generator.

`scripts/perf-gate.sh`, 32 connections and eight-second windows, mean of
two runs on macOS arm64 on 2026-10-07:

| Path | Before, req/s | After, req/s | p50 before / after |
| --- | ---: | ---: | ---: |
| Health | 131,289 | 169,060 | 225 / 165 us |
| Challenge page | 131,675 | 168,619 | 224 / 164 us |
| Authenticated proxy | 69,784 | 69,853 | 448 / 445 us |
| Allowed proxy | 70,556 | 70,884 | 443 / 441 us |

Proxy throughput is bounded by the single-threaded mock upstream and the
added hop's latency; gateway CPU per proxied request changed from 62.8 us to
60.7 us, which is within run-to-run variation.

## Further investigation

The authenticated path still allocates for the normalized request host, decoded
token payload, and deserialized host claim. These need further profiling before
changing their ownership or validation. The rate limiter serializes token
updates under one mutex, preserving its global
quota; measure contention before changing its synchronization. RandomX has its
own throughput benchmark and reference vectors, and requires separate analysis
from HTTP response performance.
