# Stratum pool

Status: proposed.

This proposal lets RandomX challenges use Monero mining pools that speak
Stratum. Today Rilgate connects only to one P2Pool node. The proposal adds a
list of pools with ordered failover, a login template, TLS, and SOCKS5 and HTTP
proxies. It renames the `p2pool` settings to `stratum_pool` in one step.

## 1. Current implementation

| Area | Behavior |
| --- | --- |
| Settings | `pow.p2pool` has `address`, `password`, `agent`, `timeout`, `sessions` and `job_sessions` (`crates/rilgate-config/src/schema/pow.rs`) |
| Pools | One pool. A connection failure makes the challenge or the share submission fail |
| Login | The Stratum client always sends `login: "x+<difficulty>"` (`crates/rilgate-stratum/src/client/mod.rs`). The difficulty is at least `1000` |
| Transport | A direct TCP connection (`TcpStream::connect` in the same file). There is no TLS and no proxy |
| Verification mode | `pow.verification: p2pool_accepted` grants access only after the pool accepts the share |
| Share submission | Only the accepted mode submits shares, over the owning WebSocket (`crates/rilgate-gateway/src/proof/websocket/session.rs`). With `local_verify`, the pool only supplies jobs |
| Tokens | Challenge, presentation and receipt claims contain `enforce`, serialized as `local_verify` or `p2pool_accepted` (`crates/rilgate-protocol/src/claims/mod.rs`) |
| Browser | `web/static/js/challenge/p2pool.js` and the RandomX disclosure text name P2Pool |

P2Pool ignores the wallet that a miner sends. It pays the wallet that the node
operator sets with `--wallet`. Other pools read the payout wallet from the
Stratum login. Thus the fixed `x+<difficulty>` login works only with P2Pool.

Some networks and hosting providers block Stratum traffic or connections to
known pool hosts. A proxy or a tunnel lets the gateway reach a pool from such a
network.

## 2. Goals and non-goals

Goals:

1. One gateway can use P2Pool or a pool that reads the wallet from the login.
2. One gateway can use several pools. It tries them in a configured order.
3. The operator controls the full login and password of each pool. Rilgate
   controls the difficulty value.
4. The connection to a pool can use TLS.
5. The connection to a pool can go through a SOCKS5 or HTTP proxy. One proxy
   list applies to all pools, and each pool can replace it. Rilgate tries the
   proxies of a list in order.
6. All public names describe the domain (a Stratum mining pool). No public name
   describes one pool implementation.

Non-goals:

- Pool-specific code paths or a `kind` setting. All supported pools use the same
  Stratum messages: `login`, `job`, `submit` and `keepalived`.
- Round-robin or weighted pool selection. The pools after the first one are
  backups.
- Proxy chains. A connection goes through one proxy at most.
- TLS between the gateway and the proxy.
- Coins other than Monero.

## 3. Decisions

| Decision | Reason |
| --- | --- |
| Rename `pow.p2pool` to `pow.stratum_pool` | The setting configures a Stratum mining pool. P2Pool is one implementation |
| Rename `p2pool_accepted` to `stratum_pool_accepted` | Same reason. The value reads as "accepted by the Stratum pool" |
| Switch in one step | Rilgate is before version 1.0. The old configuration keys cause a startup error that names the new key |
| A `pools` list with ordered failover | Predictable rewards. The order works like `verify.order` for RandomX hashers |
| A `proxies` list with ordered failover | A backup proxy covers the common failure. A chain adds latency and failure points |
| One default proxy list, replaced for each pool | Most deployments use one proxy for all pools. A local P2Pool node needs a direct connection |
| Login template with a required `{difficulty}` | Pools put the difficulty in different places. A template supports all of them without code for each pool |
| No separate `wallet` setting | P2Pool ignores a login wallet. The operator writes the wallet in the template or in an environment reference |
| Optional TLS with system roots | Public pools offer TLS ports. Without TLS, the wallet and password cross the network as plain text |
| Indexed environment variables for pools and proxies | A list of objects does not fit one variable. Variables such as `RILGATE_POW_STRATUM_POOL_POOLS_0_ADDRESS` set one field of one item, and the same schema validates them |

## 4. Configuration

```yaml
pow:
  verification: stratum_pool_accepted
  default_algorithm: randomx
  stratum_pool:
    proxies:
      - name: office-socks
        url: socks5h://10.0.0.2:1080
      - name: backup-http
        url: http://proxy.example:3128
        username: rilgate
        password: ${RILGATE_STRATUM_PROXY_PASSWORD}
    pools:
      - name: home-p2pool
        address: 10.0.0.5:3333
        login: {user: "x+{difficulty}"}
        proxies: []
      - name: supportxmr
        address: pool.supportxmr.com:443
        login:
          user: "${RILGATE_STRATUM_POOL_WALLET}.rilgate+{difficulty}"
          password: rilgate
        tls: {enabled: true}
    failover:
      cooldown: 30s
    agent: rilgate/<version>
    timeout: {connect: 5s, submit: 20s}
    sessions: {max: 64, ttl: 15m}
    job_sessions: {mode: pooled, max: 16, idle: 1m}
```

Pool settings:

| Setting | Default | Rules |
| --- | --- | --- |
| `pools` | one pool, `127.0.0.1:3333` | 1 to 8 entries |
| `pools[].name` | required | 1 to 64 bytes from `a-z`, `0-9`, `-` and `_`. Unique in the list |
| `pools[].address` | required | `HOST:PORT` or `[IPv6]:PORT`, without a scheme, credentials or path |
| `pools[].login.user` | `x+{difficulty}` | 1 to 256 bytes after substitution. It contains `{difficulty}` exactly once. It contains no other `{...}` placeholder |
| `pools[].login.password` | `x` | A secret |
| `pools[].tls.enabled` | `false` | Required in production for a host outside loopback and private networks (section 10) |
| `pools[].tls.server_name` | the `address` host | A DNS name or an IP address |
| `pools[].tls.ca` | none | A PEM file. Without it, the client uses the `webpki-roots` trust store that the workspace already has |
| `pools[].proxies` | `stratum_pool.proxies` | A proxy list for this pool. `[]` means a direct connection |

Proxy settings, for `stratum_pool.proxies` and `pools[].proxies`:

| Setting | Default | Rules |
| --- | --- | --- |
| `proxies` | `[]` | 0 to 8 entries. An empty list means a direct connection |
| `name` | required | The same rules as a pool name. Unique in its list |
| `url` | required | `socks5://HOST:PORT`, `socks5h://HOST:PORT` or `http://HOST:PORT`. No credentials, path, query or fragment |
| `username`, `password` | none | Set both or neither. `password` is a secret |

- `socks5` resolves the pool host on the gateway. `socks5h` sends the host name
  to the proxy, so the proxy resolves it.
- `http` uses the `CONNECT` method.

Shared settings:

| Setting | Default | Rules |
| --- | --- | --- |
| `failover.cooldown` | `30s` | `1s` to `10m`. The time that Rilgate skips a pool or a proxy after a connection failure |
| `agent`, `timeout`, `sessions`, `job_sessions` | current values | The rules do not change |

The gateway replaces `{difficulty}` with the challenge difficulty in decimal.
The minimum difficulty of `1000` does not change. Environment references such as
`${RILGATE_STRATUM_POOL_WALLET}` resolve before the template check.

The shared settings keep their CLI flags and environment variables, for example
`--pow-stratum-pool-timeout-connect` and `RILGATE_POW_STRATUM_POOL_TIMEOUT_CONNECT`.
`.env.example` is generated again from the settings.

### Indexed environment variables

One variable sets one field of one list item:

```bash
RILGATE_POW_STRATUM_POOL_POOLS_0_NAME=home-p2pool
RILGATE_POW_STRATUM_POOL_POOLS_0_ADDRESS=10.0.0.5:3333
RILGATE_POW_STRATUM_POOL_POOLS_0_PROXIES=
RILGATE_POW_STRATUM_POOL_POOLS_1_NAME=supportxmr
RILGATE_POW_STRATUM_POOL_POOLS_1_ADDRESS=pool.supportxmr.com:443
RILGATE_POW_STRATUM_POOL_POOLS_1_LOGIN_USER=4Abc...rilgate+{difficulty}
RILGATE_POW_STRATUM_POOL_POOLS_1_TLS_ENABLED=true
RILGATE_POW_STRATUM_POOL_PROXIES_0_NAME=office-socks
RILGATE_POW_STRATUM_POOL_PROXIES_0_URL=socks5h://10.0.0.2:1080
```

| List | Variable prefix | Fields |
| --- | --- | --- |
| Pools | `RILGATE_POW_STRATUM_POOL_POOLS_<n>_` | `NAME`, `ADDRESS`, `LOGIN_USER`, `LOGIN_PASSWORD`, `TLS_ENABLED`, `TLS_SERVER_NAME`, `TLS_CA`, `PROXIES` |
| Default proxies | `RILGATE_POW_STRATUM_POOL_PROXIES_<n>_` | `NAME`, `URL`, `USERNAME`, `PASSWORD` |
| Proxies of a pool | `RILGATE_POW_STRATUM_POOL_POOLS_<n>_PROXIES_<m>_` | The proxy fields |

- The loader changes each variable into a setting path, for example
  `pow.stratum_pool.pools.1.login.user`. The settings then go through the same
  schema, resolution and environment checks as YAML.
- A variable sets one field of the YAML item with the same index. An index
  after the last YAML item adds a new item at the end of the list.
- The indexes start at `0` and have no gaps. The YAML items count.
- An unknown field, for example `POOLS_0_ADRESS`, stops startup. The error lists
  the valid fields.
- `POOLS_<n>_PROXIES` with an empty value sets `proxies: []`.
- An error names the variable that supplied the value.
- Indexed settings have no CLI flags. A flag set is fixed when the program
  starts, and the number of pools is not.

### Pool examples

The login formats below come from the pool source code or the pool
documentation, as given. The hosts and ports are examples. Copy the current
values from the setup page of the pool.

```yaml
pools:
  # P2Pool. The node sets the wallet with --wallet. The login sets only the difficulty.
  - name: p2pool
    address: 127.0.0.1:3333
    login: {user: "x+{difficulty}"}

  # nodejs-pool, for example SupportXMR: WALLET[.worker][+difficulty], worker in the password.
  - name: supportxmr
    address: pool.supportxmr.com:443
    login:
      user: "${RILGATE_STRATUM_POOL_WALLET}.rilgate+{difficulty}"
      password: rilgate
    tls: {enabled: true}

  # MoneroOcean: WALLET[+difficulty], worker and algorithm in the password.
  - name: moneroocean
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

## 5. Connection order

For each new Stratum session, the gateway does these steps:

1. It takes the pools in the configured order and skips a pool in its cooldown.
2. For a pool, it takes the proxy list of the pool and skips a proxy in its
   cooldown. With an empty list, the gateway tries one direct connection.
3. It connects through the next proxy, then does the TLS handshake if TLS is on,
   then sends the Stratum `login`.
4. If the TCP connection, proxy handshake or TLS handshake fails, the proxy
   starts its cooldown. The gateway tries the next proxy.
5. If all proxies fail or the pool login fails, the pool starts its cooldown.
   The gateway tries the next pool. A login failure does not change proxy health.
6. If all pools fail, the challenge or the share submission fails, as with one
   pool today.
7. If all pools are in their cooldown, the gateway tries them again in order and
   ignores the cooldown. A cooldown never blocks the last chance to connect.

`timeout.connect` limits the transport phase and the login round trip separately.
The transport phase includes TCP, proxy and TLS handshakes. A session stays on
the pool that it connected to:

- A pooled job session belongs to one pool. Its jobs come only from that pool.
- In `stratum_pool_accepted` mode, the WebSocket session submits the share to the
  pool that sent the job.
- With `local_verify`, a challenge does not record the pool. Verification does
  not contact the pool.

A pool login error, for example a wrong wallet, also starts the cooldown of the
pool. The pool error stays in the log.

## 6. Difficulty from the pool

A pool can change the requested difficulty. For example, nodejs-pool limits a
fixed difficulty to its configured minimum and maximum. The challenge keeps the
target of the job that the pool sent, so verification stays correct. A higher
target makes the browser work longer.

- The gateway compares the job target with the requested difficulty.
- If they are different, the gateway writes one warning for each connection. The
  warning contains the pool name and the requested and received difficulty. It
  contains no login data.
- The counter `rilgate_stratum_pool_difficulty_mismatch_total{pool}` records each
  case.

## 7. TLS and proxies

- The Stratum client connects with `tokio-rustls` when `tls.enabled` is `true`.
  `tokio-rustls` is already in `Cargo.lock`.
- Certificate verification is always on. No setting can stop it.
- Through a proxy, TLS runs from the gateway to the pool. The proxy cannot read
  the Stratum messages.
- SOCKS5 uses the `tokio-socks` crate (MIT). The HTTP `CONNECT` handshake is a
  small local implementation: one request, a status line and headers of at most
  8 KiB.
- `cargo deny check` must pass for each new dependency.

## 8. Rename

The rename covers all public names in one release:

| Area | Old name | New name |
| --- | --- | --- |
| YAML section | `pow.p2pool` | `pow.stratum_pool` |
| YAML address | `pow.p2pool.address` | `pow.stratum_pool.pools[].address` |
| YAML password | `pow.p2pool.password` | `pow.stratum_pool.pools[].login.password` |
| Verification value | `p2pool_accepted` | `stratum_pool_accepted` |
| CLI flags | `--pow-p2pool-*` | `--pow-stratum-pool-*`, shared settings only |
| Environment variables | `RILGATE_POW_P2POOL_ADDRESS`, `RILGATE_POW_P2POOL_PASSWORD` | `RILGATE_POW_STRATUM_POOL_POOLS_<n>_ADDRESS`, `RILGATE_POW_STRATUM_POOL_POOLS_<n>_LOGIN_PASSWORD` |
| Other environment variables | `RILGATE_POW_P2POOL_*` | `RILGATE_POW_STRATUM_POOL_*` |
| Browser client value | `enforce: "p2pool_accepted"` | `enforce: "stratum_pool_accepted"` |
| Browser module | `js/challenge/p2pool.js` | `js/challenge/stratum-pool.js` |
| User text | "P2Pool" in status and disclosure strings | "the mining pool" |

Old names:

- `pow.p2pool` and `p2pool_accepted` in YAML cause a startup error. The error
  names the new key, for example `pow.p2pool: renamed to pow.stratum_pool`.
- An old `RILGATE_POW_P2POOL_*` variable causes the same type of error.
- An old `--pow-p2pool-*` flag is an unknown argument.

Tokens in use:

- Challenge, presentation and receipt tokens carry `enforce`. A token that the
  previous version issued can contain `p2pool_accepted`.
- Token decoding accepts `p2pool_accepted` as an alias of
  `stratum_pool_accepted`. Token encoding writes only the new value.
- Remove the alias after one release. The longest token lifetime is
  `pow.algorithms.randomx.challenge_ttl` (default `1h`), so one release is enough.

Internal names, such as `P2PoolProvider` and `PowEnforcement::P2poolAccepted`,
change to `StratumPool*` names in the same change.

## 9. Security

- A public pool is a third party. With `stratum_pool_accepted`, access depends on
  the answer of that pool. A pool that accepts every share grants access to every
  client. Use pools that you trust with this decision, or use `local_verify`.
- With `local_verify`, the gateway checks the hash locally and submits no share.
  The pool only supplies jobs, and the operator gets no reward.
- A failover pool can pay a different wallet. Each pool has its own login.
- The login, the pool password, the proxy credentials and the proxy URL can
  identify the operator. Logs, metrics and readiness responses show only pool
  and proxy names. `config show` masks each secret, as it masks secrets today.
- A proxy sees the pool host and the traffic volume. Without TLS to the pool, it
  also sees the login and the shares.
- An `http` proxy and a `socks5` proxy send their credentials as plain text.
  Use them only on a private network or a tunnel.
- Public pools can limit or block an IP address that opens many connections. Use
  `job_sessions.mode: pooled` with a public pool. The `per_challenge` mode opens
  one connection for each job.

## 10. Operations

Readiness:

- `/api/ready` is ready when at least one pool accepts a connection through its
  proxies. All pools are probed concurrently. Each pool probe uses
  `server.readiness_timeout`. Probes do not change cooldowns or connection and
  failover counters.
- The response names each pool and its result. It does not show addresses or
  credentials.

New metrics. Labels contain only configured pool and proxy names:

| Metric | Type | Labels |
| --- | --- | --- |
| `rilgate_stratum_pool_connections_total` | counter | `pool`, `outcome` (`connected`, `failed`) |
| `rilgate_stratum_pool_failover_total` | counter | `from` (pool name) |
| `rilgate_stratum_proxy_connections_total` | counter | `proxy`, `outcome` (`connected`, `failed`) |
| `rilgate_stratum_pool_difficulty_mismatch_total` | counter | `pool` |

The current pool metric names, for example `rilgate_pool_sessions_active`,
contain no `p2pool` part and do not change.

## 11. Environment checks

| Identifier | Environment | Level | Condition |
| --- | --- | --- | --- |
| `stratum-pool-plain-tcp` | production | error | A pool has `tls.enabled: false` and an `address` host outside loopback and private networks |
| `stratum-pool-plain-tcp` | development | warning | The same condition |
| `stratum-proxy-plain-credentials` | production | error | A proxy with credentials has a host outside loopback and private networks |
| `stratum-proxy-plain-credentials` | development | warning | The same condition |
| `stratum-pool-per-challenge-public` | both | warning | `job_sessions.mode` is `per_challenge` and a pool host is outside loopback and private networks |

The rules in section 4 are validation errors, not environment checks. A
configuration that breaks them does not load.

## 12. Compatibility

| Area | Result |
| --- | --- |
| Configuration files | Must change. The startup error names each new key |
| Environment variables and flags | Must change. The pool address and password use indexed variables and have no flag |
| Challenge, presentation and receipt tokens | Tokens from the previous version stay valid until they expire |
| Auth cookies | No change |
| Browser solvers | Ship with the gateway. The new `enforce` value and module path change together |
| Default behavior | No change. The default pool list has one pool at `127.0.0.1:3333` with the login `x+{difficulty}` and the password `x`. It sends the same Stratum login as today |

## 13. Verification

| Area | Test |
| --- | --- |
| Template | `{difficulty}` is required exactly once. An unknown placeholder fails. The substituted login reaches the Stratum `login` message |
| Pool order | A failed first pool gives a session on the second pool and one failover count. A pool in its cooldown is skipped. When all pools are in their cooldown, the first pool is tried |
| Affinity | A share goes to the pool of its job |
| Proxy inheritance | A pool without `proxies` uses the default list. `proxies: []` connects directly. A pool list replaces the default list |
| Indexed variables | A variable sets one field of a YAML item and adds an item at the end. A gap, an unknown field and an index over the limit fail. An invalid value fails with the variable name. An empty `PROXIES` value connects directly |
| Proxy order | A failed first proxy gives a connection through the second proxy |
| SOCKS5 | `socks5` sends an address, and `socks5h` sends the host name. Credentials work. A refused request fails over |
| HTTP CONNECT | A `200` response connects. Another status, a response over 8 KiB and a closed connection fail over |
| TLS | A handshake through a proxy verifies the pool certificate. An untrusted certificate and a wrong `server_name` fail |
| Rename | Each old YAML key and environment variable fails with an error that names the new name |
| Tokens | A token with `p2pool_accepted` decodes. A new token contains `stratum_pool_accepted` |
| Difficulty | A job with a higher target than requested produces one warning and one counter increment. The challenge uses the job target |
| Secrets | Logs, metrics, readiness and `config show` contain no login, password, proxy URL or proxy credentials |
| Environment checks | Each identifier in section 11 appears only under its condition |
| Live | An opt-in test runs the full flow against a pool that a test environment variable names. Nothing in the repository contains a pool host |

## 14. Delivery sequence

1. Rename the settings, the verification value, the flags, the environment
   variables, the browser module and the user text. Add the token alias and the
   startup errors for old names.
2. Replace `address` and `password` with the `pools` list and its login template.
   Add ordered failover with the cooldown, pool affinity, the pool metrics and the
   indexed environment variables for pools.
3. Add TLS for the Stratum client and the readiness probe.
4. Add the proxy lists with SOCKS5, SOCKS5h and HTTP `CONNECT`, inheritance and
   ordered failover. Add the proxy metrics and the indexed environment variables
   for proxies.
5. Add the difficulty mismatch warning and counter.
6. Add the environment checks.
7. Update `docs/pow.md`, `docs/configuration.md`, `docs/production.md`,
   `docs/architecture.md`, `docs/api-integration.md`, `docs/algorithms.md`,
   `config/rilgate.example.yml`, `config/rilgate.randomx.example.yml` and
   `.env.example`. `docs/production.md` also describes Stratum blocking and the
   tunnel, proxy and separate-host options.
