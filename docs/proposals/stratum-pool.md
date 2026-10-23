# Stratum pool

Status: proposed.

This proposal lets RandomX challenges use any Monero mining pool that speaks
Stratum. Today Rilgate connects only to a P2Pool node. The proposal adds a login
template and TLS, and it renames the `p2pool` settings to `stratum_pool` in one
step.

## 1. Current implementation

| Area | Behavior |
| --- | --- |
| Settings | `pow.p2pool` has `address`, `password`, `agent`, `timeout`, `sessions` and `job_sessions` (`crates/rilgate-config/src/schema/pow.rs`) |
| Login | The Stratum client always sends `login: "x+<difficulty>"` (`crates/rilgate-stratum/src/client/mod.rs:62`). The difficulty is at least `1000` |
| Transport | Plain TCP (`TcpStream::connect` in the same file). There is no TLS |
| Verification mode | `pow.verification: p2pool_accepted` grants access only after the pool accepts the share |
| Share submission | Only the accepted mode submits shares, over the owning WebSocket (`crates/rilgate-gateway/src/proof/websocket/session.rs`). With `local_verify`, the pool only supplies jobs |
| Tokens | Challenge, presentation and receipt claims contain `enforce`, serialized as `local_verify` or `p2pool_accepted` (`crates/rilgate-protocol/src/claims/mod.rs`) |
| Browser | `web/static/js/challenge/p2pool.js` and the RandomX disclosure text name P2Pool |

P2Pool ignores the wallet that a miner sends. It pays the wallet that the node
operator sets with `--wallet`. Other pools read the payout wallet from the
Stratum login. Thus the fixed `x+<difficulty>` login works only with P2Pool.

## 2. Goals and non-goals

Goals:

1. One gateway can use P2Pool or a pool that reads the wallet from the login.
2. The operator controls the full login and password. Rilgate controls the
   difficulty value.
3. The connection to a pool on a public network can use TLS.
4. All public names describe the domain (a Stratum mining pool). No public name
   describes one pool implementation.

Non-goals:

- Pool-specific code paths or a `kind` setting. All supported pools use the same
  Stratum messages: `login`, `job`, `submit` and `keepalived`.
- Several pools with failover. One gateway uses one pool.
- Coins other than Monero.

## 3. Decisions

| Decision | Reason |
| --- | --- |
| Rename `pow.p2pool` to `pow.stratum_pool` | The setting configures a Stratum mining pool. P2Pool is one implementation |
| Rename `p2pool_accepted` to `stratum_pool_accepted` | Same reason. The value reads as "accepted by the Stratum pool" |
| Switch in one step | Rilgate is before version 1.0. The old configuration keys cause a startup error that names the new key |
| Login template with a required `{difficulty}` | Pools put the difficulty in different places. A template supports all of them without code for each pool |
| No separate `wallet` setting | P2Pool ignores a login wallet. The operator writes the wallet in the template or in an environment reference |
| Optional TLS with system roots | Public pools offer TLS ports. Without TLS, the wallet and password cross the network as plain text |

## 4. Configuration

```yaml
pow:
  verification: stratum_pool_accepted
  default_algorithm: randomx
  stratum_pool:
    address: 127.0.0.1:3333
    login:
      user: "x+{difficulty}"
      password: x
    tls:
      enabled: false
      server_name: null
      ca: null
    agent: rilgate/<version>
    timeout: {connect: 5s, submit: 20s}
    sessions: {max: 64, ttl: 15m}
    job_sessions: {mode: pooled, max: 16, idle: 1m}
```

| Setting | Default | Rules |
| --- | --- | --- |
| `address` | `127.0.0.1:3333` | `HOST:PORT` or `[IPv6]:PORT`, without a scheme, credentials or path. The rules do not change |
| `login.user` | `x+{difficulty}` | 1 to 256 bytes after substitution. It contains `{difficulty}` exactly once. It contains no other `{...}` placeholder |
| `login.password` | `x` | A secret. The rules of the current `password` apply |
| `tls.enabled` | `false` | Required in production when the address host is outside loopback and private networks (section 8) |
| `tls.server_name` | the `address` host | A DNS name or an IP address |
| `tls.ca` | none | A PEM file. Without it, the client uses the `webpki-roots` trust store that the workspace already has |
| `agent`, `timeout`, `sessions`, `job_sessions` | current values | The rules do not change |

The gateway replaces `{difficulty}` with the challenge difficulty in decimal.
The minimum difficulty of `1000` does not change. Environment references such as
`${RILGATE_STRATUM_POOL_WALLET}` resolve before the template check.

The CLI flags and environment variables follow the new keys. For example,
`--pow-stratum-pool-address` and `RILGATE_POW_STRATUM_POOL_ADDRESS` replace
`--pow-p2pool-address` and `RILGATE_POW_P2POOL_ADDRESS`. `.env.example` is
generated again from the settings.

### Pool examples

The login formats below come from the pool source code or the pool
documentation, as given. The hosts and ports are examples. Copy the current
values from the setup page of the pool.

```yaml
# P2Pool. The node sets the wallet with --wallet. The login sets only the difficulty.
stratum_pool:
  address: 127.0.0.1:3333
  login: {user: "x+{difficulty}"}

# nodejs-pool, for example SupportXMR: WALLET[.worker][+difficulty], worker in the password.
stratum_pool:
  address: pool.supportxmr.com:443
  login:
    user: "${RILGATE_STRATUM_POOL_WALLET}.rilgate+{difficulty}"
    password: rilgate
  tls: {enabled: true}

# MoneroOcean: WALLET[+difficulty], worker and algorithm in the password.
stratum_pool:
  address: gulf.moneroocean.stream:20128
  login:
    user: "${RILGATE_STRATUM_POOL_WALLET}+{difficulty}"
    password: "rilgate~rx/0"
  tls: {enabled: true}
```

Sources for the formats:

- P2Pool: the [README](https://github.com/SChernykh/p2pool/blob/master/README.md)
  says that XMRig ignores a wallet address, and it shows `-u x+10000`.
- nodejs-pool: [`lib/pool.js`](https://github.com/Snipa22/nodejs-pool/blob/master/lib/pool.js)
  splits the login on `+` and `.`, and the password on `:`.
- MoneroOcean: [`lib/pool/miners.js`](https://github.com/MoneroOcean/nodejs-pool/blob/master/lib/pool/miners.js)
  gives the format `monero_address[(%N%monero_address_95char)+][+difficulty_number]`.

## 5. Difficulty from the pool

A pool can change the requested difficulty. For example, nodejs-pool limits a
fixed difficulty to its configured minimum and maximum. The challenge keeps the
target of the job that the pool sent, so verification stays correct. A higher
target makes the browser work longer.

- The gateway compares the job target with the requested difficulty.
- If they are different, the gateway writes one warning for each connection. The
  warning contains the requested and the received difficulty, and no login data.
- A new counter `rilgate_stratum_pool_difficulty_mismatch_total` records each
  case.

## 6. TLS

- The Stratum client connects with `tokio-rustls` when `tls.enabled` is `true`.
  `tokio-rustls` is already in `Cargo.lock`.
- Certificate verification is always on. No setting can stop it.
- The readiness probe uses the same TLS settings. A handshake failure makes
  `/api/ready` return `503`, like a refused connection today.

## 7. Rename

The rename covers all public names in one release:

| Area | Old name | New name |
| --- | --- | --- |
| YAML section | `pow.p2pool` | `pow.stratum_pool` |
| YAML password | `pow.p2pool.password` | `pow.stratum_pool.login.password` |
| Verification value | `p2pool_accepted` | `stratum_pool_accepted` |
| CLI flags | `--pow-p2pool-*` | `--pow-stratum-pool-*` |
| Environment variables | `RILGATE_POW_P2POOL_*` | `RILGATE_POW_STRATUM_POOL_*` |
| Browser client value | `enforce: "p2pool_accepted"` | `enforce: "stratum_pool_accepted"` |
| Browser module | `js/challenge/p2pool.js` | `js/challenge/stratum-pool.js` |
| User text | "P2Pool" in status and disclosure strings | "the mining pool" |

Old configuration keys:

- `pow.p2pool` and `p2pool_accepted` in YAML cause a startup error. The error
  names the new key, for example `pow.p2pool: renamed to pow.stratum_pool`.
- An old `RILGATE_POW_P2POOL_*` variable or `--pow-p2pool-*` flag causes the
  same type of error.

Tokens in use:

- Challenge, presentation and receipt tokens carry `enforce`. A token that the
  previous version issued can contain `p2pool_accepted`.
- Token decoding accepts `p2pool_accepted` as an alias of
  `stratum_pool_accepted`. Token encoding writes only the new value.
- Remove the alias after one release. The longest token lifetime is
  `pow.algorithms.randomx.challenge_ttl` (default `1h`), so one release is enough.

Internal names, such as `P2PoolProvider` and `PowEnforcement::P2poolAccepted`,
change to `StratumPool*` names in the same change. These names are not public.
The change keeps code and configuration in agreement.

## 8. Security

- A public pool is a third party. With `stratum_pool_accepted`, access depends on
  the answer of that pool. A pool that accepts every share grants access to every
  client. Use a pool that you trust with this decision, or use `local_verify`.
- With `local_verify`, the gateway checks the hash locally and submits no share.
  The pool only supplies jobs, and the operator gets no reward.
- The login and password can contain a wallet address and an email address. Logs,
  metrics, readiness responses and `config show` never show them. `config show`
  shows the template with `{difficulty}` and masks the password, as it masks
  secrets today.
- Public pools can limit or block an IP address that opens many connections. Use
  `job_sessions.mode: pooled` with a public pool. The `per_challenge` mode opens
  one connection for each job.

## 9. Operations

- Readiness keeps the current probe and adds the TLS handshake when TLS is on.
- The metric `rilgate_stratum_pool_difficulty_mismatch_total` is new (section 5).
- The current pool metric names, for example `rilgate_pool_sessions_active`,
  contain no `p2pool` part and do not change.

## 10. Environment checks

| Identifier | Environment | Level | Condition |
| --- | --- | --- | --- |
| `stratum-pool-plain-tcp` | production | error | `tls.enabled` is `false` and the `address` host is outside loopback and private networks |
| `stratum-pool-plain-tcp` | development | warning | The same condition |
| `stratum-pool-per-challenge-public` | both | warning | `job_sessions.mode` is `per_challenge` and the `address` host is outside loopback and private networks |

The template rules in section 4 are validation errors, not environment checks.
A configuration that breaks them does not load.

## 11. Compatibility

| Area | Result |
| --- | --- |
| Configuration files | Must change. The startup error names each new key |
| Environment variables and flags | Must change |
| Challenge, presentation and receipt tokens | Tokens from the previous version stay valid until they expire |
| Auth cookies | No change |
| Browser solvers | Ship with the gateway. The new `enforce` value and module path change together |
| Default behavior | No change. The default login `x+{difficulty}` and the default password `x` send the same Stratum login as today |

## 12. Verification

| Area | Test |
| --- | --- |
| Template | `{difficulty}` is required exactly once. An unknown placeholder fails. The substituted login reaches the Stratum `login` message |
| Rename | Each old YAML key, flag and environment variable fails with an error that names the new key |
| Tokens | A token with `p2pool_accepted` decodes. A new token contains `stratum_pool_accepted` |
| TLS | A handshake succeeds with `ca`. A handshake fails with an untrusted certificate and with a wrong `server_name`. Readiness reports both failures |
| Difficulty | A job with a higher target than requested produces one warning and one counter increment. The challenge uses the job target |
| Secrets | Logs, metrics and `config show` contain no login and no password |
| Environment checks | Each identifier in section 10 appears only under its condition |
| Live | An opt-in test runs the full flow against a pool that a test environment variable names. Nothing in the repository contains a pool host |

## 13. Delivery sequence

1. Rename the settings, the verification value, the flags, the environment
   variables, the browser module and the user text. Add the token alias and the
   startup errors for old names.
2. Add `login.user` and `login.password` with the template rules.
3. Add TLS for the Stratum client and the readiness probe.
4. Add the difficulty mismatch warning and counter.
5. Add the environment checks.
6. Update `docs/pow.md`, `docs/configuration.md`, `docs/production.md`,
   `docs/architecture.md`, `docs/api-integration.md`, `docs/algorithms.md`,
   `config/rilgate.example.yml`, `config/rilgate.randomx.example.yml` and
   `.env.example`.

The branch `feat/randomx-remote-verification` also changes `pow` settings and
the RandomX code. Merge it before step 1, or rebase step 1 on it, to keep the
rename in one place.
