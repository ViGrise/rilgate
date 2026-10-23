# rilgate-pow-hashwx

The HashWX implementation of `rilgate_pow::Algorithm`. It issues 32 random
challenge bytes, derives each seed as SHA-256(challenge || little-endian u64
batch), and verifies one interpreted hash on a blocking task. It accepts
canonical decimal u64 nonces and bounded signed work parameters.

The constructor receives only `HashWxConfig`. Configuration loading and token
signatures remain in their existing layers. The challenge service validates
the token before dispatch and validates lifetime before and after verification.

Use `pow.algorithms.hashwx` as described in
[configuration](../../docs/configuration.md#hashwx-configuration).
The difficulty defaults remain placeholders until a mid-range phone is measured.
The browser solver and raw ABI share the specification implementation in
`hashwx-core`. See the [license review](../../docs/hashwx-license-review.md).

```sh
cargo test -p rilgate-pow-hashwx --locked
cargo test -p rilgate-gateway --test hashwx --locked
```

HashWX reserves one verification slot per available CPU. When all slots are
reserved, verification returns the capacity error (HTTP 503). A slot stays
reserved until its blocking task completes, including after request cancellation.
