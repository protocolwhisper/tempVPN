---
name: tempvpn
description: Set up, preflight, connect, buy, verify, inspect, and disconnect temporary paid WireGuard VPN sessions through Tempo MPP (one-time charge or Session v2 streaming) on Linux or macOS. Use for natural requests such as "Connect 30 mins to Belgium", "use a Belgian VPN for one hour", "get me the fastest VPN in Singapore", "what does TempVPN need", "check whether TempVPN is ready", "show VPN status", or "disconnect the VPN". Normalizes duration and location into structured indexer filters, selects the fastest eligible node from the user's network, keeps private keys local, and pauses unused balance on disconnect.
---

# tempVPN

Turn a plain-language request into a safe, complete temporary VPN workflow:

```text
resolve intent → preflight → query indexer → shortlist available nodes
→ client latency-rank → pay selected node
→ connect native client → verify exit → report
```

## Interpret the request

Extract:

- **Action:** setup, preflight, connect/buy, status, heartbeat, or disconnect.
- **Duration:** convert to seconds (`30 mins` → `1800`). Never purchase when the
  duration is missing or ambiguous.
- **Location:** normalize an unambiguous country to its ISO 3166-1 alpha-2 code;
  retain an optional city or advertised region. Do not maintain or consult an
  exhaustive phrase list in this skill. If location is omitted, select the
  fastest eligible node globally.
- **Selection policy:** words such as `fastest`, `quickest`, or `lowest ping`
  mean `lowest_latency`. This is the default and currently the only policy.

Use this normalized intent contract internally:

```text
action: setup | preflight | connect | buy | status | heartbeat | disconnect
duration_seconds: integer | absent
country_code: ISO alpha-2 | absent
city: string | absent
region: string | absent
selection_policy: lowest_latency
```

Never send, store, or log the user's raw prompt in the indexer or node request.
Only pass the normalized fields supported by the platform client.

Requests such as “connect,” “start,” or “use” include purchase, local
connection, and exit-IP verification. Stop after purchase only when the user
explicitly asks to buy without connecting.

Treat “what does TempVPN need?” or “is TempVPN ready?” as read-only preflight.
Treat “set up TempVPN” as authorization to inspect first and then perform only
the separately approved installation or account-provisioning actions; it never
authorizes a VPN purchase.

Treat “Connect 30 mins to Belgium” as authorization to buy exactly 1,800 seconds
on a Belgian node. Still use any approval mechanism required by the environment
for payment, installation, or administrator access. Do not ask for a redundant
confirmation after a successful payment; continue immediately to connection.

Interpret representative requests as follows:

| Request | Normalized result |
| --- | --- |
| `Connect 30 mins to Belgium` | `connect`, `1800`, `BE`, `lowest_latency` |
| `Use a Belgian VPN for one hour` | `connect`, `3600`, `BE`, `lowest_latency` |
| `Get me the fastest VPN in Singapore` | Ask for duration, retain `SG` and `lowest_latency`; do not pay yet |
| `Connect for 20 minutes to the fastest VPN` | `connect`, `1200`, global `lowest_latency` |
| `What does TempVPN need?` | `preflight`; inspect and report blockers, with no installation or payment |
| `Set up TempVPN` | `setup`; preflight, then request approval for each privileged or account action |
| `Show VPN status` | `status`; no discovery or payment |
| `Disconnect the VPN` | `disconnect`; no discovery or payment |

Ask for clarification before discovery or payment when a place is genuinely
ambiguous, such as `Congo` (`CG` or `CD`) or `Georgia` when country versus US
state is unclear. Reject values that cannot be normalized to one ISO alpha-2
country; never guess or silently choose a nearby country.

## Query the indexer before payment

Treat the registry as the node indexer. Query it before contacting or paying an
exit node. Send structured `country`, `city`, `region`, and `available` query
parameters through the platform client—never the raw prompt. The indexer
returns currently leased nodes matching all supplied filters.

Choose the registry URL in this order:

1. Preserve an explicit `VPN_CLIENT_REGISTRY_URL` or equivalent client option.
2. Otherwise use `https://registry.tempvpn.xyz` only when its `/health`
   endpoint succeeds with valid TLS.
3. If the global registry is unavailable, use a regional HTTPS fallback only
   when the requested location unambiguously belongs to that catalog:
   - Americas: `https://us-east.tempvpn.xyz`
   - Europe/Asia: `https://belgium.tempvpn.xyz`

If global discovery is unavailable and a global-fastest request cannot be
answered from one unambiguous regional catalog, stop before payment and report
the discovery outage. Never silently narrow a global request to one region.

Production node DNS names are:

| Node ID | HTTPS origin |
| --- | --- |
| `us-east` | `https://us-east.tempvpn.xyz` |
| `us-west` | `https://us-west.tempvpn.xyz` |
| `sao-paulo` | `https://sao-paulo.tempvpn.xyz` |
| `belgium` | `https://belgium.tempvpn.xyz` |
| `madrid` | `https://madrid.tempvpn.xyz` |
| `singapore` | `https://singapore.tempvpn.xyz` |

Treat this table as an identity allowlist, not as discovery data. Always retain
the exact `api_url` returned by the live catalog; never construct a node URL
from a node ID, replace it with a bare IP address, or downgrade an advertised
HTTPS URL to HTTP. A rollout-era catalog may temporarily advertise a legacy
HTTP origin; accept it only through an explicit operator override, never as the
production default.

Use the indexer to:

1. Filter for the requested country, city, or region.
2. Remove expired, unavailable, draining, or capacity-exhausted nodes.
3. Produce a small eligible shortlist.

Then health-check and latency-rank the shortlist from the user's machine. Do not
select solely from a ping measured by the indexer: that latency describes the
indexer-to-node path, not the user-to-node path.

Perform one final node health check immediately before payment. The selected
node's session endpoint remains the final authority on availability.

Never guess a location from an IP address and never silently substitute a
different country.

Use only structured country/city metadata for jurisdiction selection. Legacy
nodes without it may remain usable through an explicit URL, but never guess
their country from `region`, name, or IP address. If several eligible nodes
match, let the client choose the lowest median latency. If no node matches, say
that no eligible node is currently available in the requested location and stop
before payment.

## Preflight before payment

Treat preflight as a read-only action. It may inspect commands, application
bundles, signatures, account names, and service health, but it must not create
an account, move funds, install software, invoke `sudo`, or make a paid request.

Detect the platform:

- **Linux:** require `vpn-client`, `wg`, and `wg-quick`, plus permission to
  create the interface.
- **macOS:** require signed `tempvpnctl`, `/Applications/TempVPN.app`, its
  embedded Packet Tunnel extension, and WireGuardKit. Resolve the installed
  `tempvpnctl` from `PATH`; a repository `target/tempvpnctl` is a build artifact,
  not the installed runtime. Verify the CLI, app, and embedded extension have
  valid non-ad-hoc signatures from the expected Apple team. Treat
  `Signature=adhoc`, a missing `TeamIdentifier`, or mismatched team identifiers
  as not installed. If the installed version exposes `tempvpnctl doctor
  --json`, also require it to report ready. Never substitute external `wg` or
  `wg-quick` on macOS.
- **Windows or another platform:** stop before payment.

Require:

- A healthy production global registry, an explicit registry override, or an
  unambiguous regional HTTPS fallback as defined above.
- Installed `mppx`.
- A funded MPPX account named `main`.

Do not require the Tempo Wallet desktop or mobile application. This workflow
does not invoke it. Payment uses `mppx`; a separate wallet may be one way for
the user to transfer funds to the MPPX account, but wallet setup and transfers
remain outside the VPN connection workflow.

Check MPPX readiness in the user's real login and Keychain context. Parse
`mppx account list --format json` locally and report only whether an account
named `main` is present; never echo addresses or raw account output. A Keychain
or sandbox access error means `unknown`, not `missing`. Account listing proves
existence, not sufficient token balance. Verify funding with account tooling
when available, without printing addresses, balances, or keys; otherwise state
that funding is unverified and let only an explicitly authorized VPN purchase
test it. Ensure the account and the selected node's MPP challenge use the same
Tempo network and payment currency.

Account provisioning is separate from connection. Never automatically create,
fund, replace, export, or repair an MPPX account after a failed check or
payment. If the user explicitly requests account setup, perform it as a
separate visible action and do not expose command output that could contain
account or private-key material. `mppx account fund` supplies testnet tokens
only; never present it as a production/mainnet funding method.

On macOS, if installed products are missing and this skill is running from a
TempVPN development checkout, locate the repository root by the presence of
`clients/macos/build-macos-products.sh` and
`clients/macos/install-tempvpnctl.sh`. The checkout may be one directory above
this skill (for example `../`), but never assume that layout outside a verified
checkout. Explain that a usable native build requires:

- Xcode and Go;
- an Apple Developer team and signing identity;
- provisioning for the Packet Tunnel Network Extension and the shared Keychain
  access group;
- both products built by `clients/macos/build-macos-products.sh`; and
- explicit administrator approval to run
  `clients/macos/install-tempvpnctl.sh` and install the products into
  `/Applications` and `/usr/local/bin`.

Unsigned or ad-hoc products under `target/` may be used only to validate that
the source compiles. Never install them or use them for a paid session. After a
properly signed install, expect macOS to request one-time VPN profile approval
on the first connection.

If a required client is missing, explain the installation action and request
permission before downloading, installing, or using `sudo`. Installation and
payment must remain separate visible actions. Summarize preflight as `ready`,
`needs MPPX account/funding`, `needs native client signing/install`, or
`service unavailable`, listing only the unmet prerequisites. Do not proceed to
payment unless preflight is ready or the only unverified condition is balance
and the user's request already authorizes the exact purchase.

## Resolve exactly one VPN client

`mppx` is the separate payment CLI; it is not the VPN client. Select exactly one
native VPN client from the detected operating system and use that same client
for selection, live checks, connection, status, heartbeat, and disconnect:

| Platform | One client to use | If it is absent |
| --- | --- | --- |
| Linux | `vpn-client` | In a verified checkout, build with `cargo build --release -p vpn-client-cli` and use `target/release/vpn-client`, or explain the trusted binary installation and request approval. Also install `wg` and `wg-quick` through the OS package manager. |
| macOS | signed `tempvpnctl` plus `/Applications/TempVPN.app` | Build signed products with `clients/macos/build-macos-products.sh`, then request approval for `clients/macos/install-tempvpnctl.sh`; never use the unsigned `target/` artifacts for a tunnel. |

Do not run or present both platform branches. A development checkout does not
mean the CLI is installed: resolve `command -v vpn-client` or
`command -v tempvpnctl` first, then use only the verified installed command (or
the explicitly built Linux path). If neither the required binary nor a trusted
source/release is available, stop and report the missing client before
discovery or payment.

## Distribute clients for users

Do not tell end users to install a Rust crate. This workspace marks the client
package `publish = false`; Cargo source is a maintainer build input, not a
stable distribution channel. The project must publish trusted client artifacts
before the skill can bootstrap a user who has no checkout:

- **Linux:** CI builds `vpn-client` for each supported target with
  `cargo build --release -p vpn-client-cli`, packages each binary with a
  version and target name, and publishes SHA-256 checksums. Users still need
  the platform's `wg` and `wg-quick` packages; those are OS dependencies, not
  Rust crates.
- **macOS:** CI builds the host app and CLI with the same Apple team, signs the
  app, Packet Tunnel extension, and CLI, notarizes the distributable, and
  publishes the app plus CLI together. Never distribute the unsigned
  `target/` artifacts.
- **Payment:** `mppx` remains a separate Node/npm CLI and is not bundled into
  either VPN client. If it is absent, explain the approved installation of the
  pinned/approved `mppx` release separately.

When a trusted HTTPS release URL and checksum/signature policy are configured,
the skill may offer to download and install the matching platform artifact
after explicit user approval. Verify the artifact before installation, keep
the temporary archive private, and never substitute a source checkout or an
unverified URL. Without a published artifact source, the skill can diagnose the
missing client and provide the maintainer build instructions, but it must stop
before installation, discovery, or payment.

## Select the node

The platform client queries the indexer with structured filters, removes nodes
that are draining or full, probes candidates from the user's device, and
chooses the lowest median latency:

Use exactly one branch:

```bash
# Linux only
vpn-client select --country BE --selection-policy lowest-latency --json
```

```bash
# macOS only
tempvpnctl select --country BE --selection-policy lowest-latency --json
```

Add `--city CITY` or `--region REGION` only when present in the normalized
intent. Omit `--country`, `--city`, and `--region` for global fastest. Parse the
JSON result and retain its exact `node_url`, node name/ID, structured location,
and `expected_exit_ip` for payment, connection, verification, and reporting.

An explicit node URL may bypass catalog ranking but must still pass its health
check. The indexer URL is not a payment endpoint unless it is also the selected
exit node.

## Purchase and connect

Create a private temporary working directory and restrict its permissions. Keep
the session response and any Linux key file there; do not use predictable shared
filenames.

Payment must target the exact selected node:

Immediately before `mppx`, perform the native live availability check:

```bash
# Linux only
vpn-client check --node-url "$SELECTED_NODE_URL" --json
```

```bash
# macOS only
tempvpnctl check --node-url "$SELECTED_NODE_URL" --json
```

Run only the command for the selected platform. If it fails, stop without
calling `mppx`; a catalog result is an advisory snapshot, not a capacity
reservation.

Then payment must target the exact selected node:

```bash
mppx "$SELECTED_NODE_URL/sessions" \
  --account main \
  --json-body '{"duration_seconds":1800}' \
  --silent
```

Save the JSON response without logging sensitive command output. Payment creates
a paused logical-node usage balance; connected time begins on activation. A
later resume may return a new generation-specific WireGuard server key and
endpoint while keeping the same logical node URL and balance.

### Linux

Generate the private key locally and make it owner-readable only:

```bash
wg genkey
chmod 600 PRIVATE_KEY_PATH
```

Connect with the paid response and the same node:

```bash
sudo vpn-client connect \
  --node-url "$SELECTED_NODE_URL" \
  --session-response SESSION_RESPONSE_PATH \
  --private-key-path PRIVATE_KEY_PATH
```

### macOS

The signed native client generates the private key in Keychain and sends only
its public key:

```bash
tempvpnctl connect \
  --node-url "$SELECTED_NODE_URL" \
  --session-response SESSION_RESPONSE_PATH \
  --node-name "$SELECTED_NODE_NAME" \
  --country-code "$SELECTED_COUNTRY_CODE" \
  --city "$SELECTED_CITY" \
  --region "$SELECTED_REGION" \
  --json
```

Omit optional location arguments that were absent from the selection result.
These values preserve advertised location in the native VPN profile; they do
not affect Keychain access or contain private-key material.

macOS may request one-time VPN profile approval on the first connection. This is
not a recurring private-key or biometric prompt.

## Streaming sessions (Tempo Session v2)

Some nodes also offer metered streaming access at:

```text
GET /sessions/stream?client_public_key=<wireguard-public-key>&duration_seconds=<safety-cap>
```

Instead of buying a fixed duration up front, the client opens a Session v2
payment channel reserve and the node charges one unit per billing interval
while the tunnel stays up. Extending service time is a channel operation, not
a new purchase. `duration_seconds` is a safety cap, not a prepaid amount.

An unpaid request returns `402` with a `WWW-Authenticate: Payment` challenge:
MPP method `tempo`, intent `session`, `sessionProtocol: "v2"`, with the
currency, recipient, per-interval `unitAmount`, and a `suggestedDeposit` for
the channel reserve. Answer it with an `Authorization: Payment` credential
carrying a Session v2 payload:

- `Open` — open and fund a new channel on the TIP-20 reserve contract
- `Voucher` — newer cumulative signed voucher for an existing channel
- `TopUp` — on-chain `topUp` transaction that adds deposit to the reserve
- `Close` — finalize the channel and end the session

On success the node responds `200 text/event-stream` with an
`x-vpn-session-id` header. The SSE stream carries:

- `message` JSON `type: "vpn-session"` — connection details: `session`,
  `channelId`, `billingIntervalSeconds`, `unitAmount`
- `message` JSON `type: "paid-interval"` — one billed unit: `sessionId`,
  `channelId`, `units`, `spent`
- `payment-need-voucher` — accepted value cannot cover the next interval:
  `{channelId, requiredCumulative, acceptedCumulative, deposit}`; the node
  pauses the WireGuard peer and does not bill the paused period
- `payment-receipt` — final receipt with accepted cumulative value, `spent`,
  and `units` when the stream ends

To extend or resume, submit a newer voucher (or an on-chain `TopUp`) whose
cumulative amount reaches `requiredCumulative`. Use `HEAD /sessions/stream`
with the same query parameters to submit channel operations (voucher, top-up,
or close) without consuming the SSE body. The same logical session resumes
once the new state verifies. If the client does not replenish within the
node's grace period, the peer is removed and the stream ends with a final
receipt.

If the node answers `404` with "streaming payments are disabled", fall back to
the one-time `POST /sessions` purchase flow above.

## Verify before claiming success

After the tunnel reports connected:

1. Read platform status.
2. Query the visible public IP through the intended route.
3. Compare it with the selected node's `expected_exit_ip`.
4. When location data is available, confirm the result is consistent with the
   requested location.

Do not claim success merely because a command exited zero. If the tunnel is
active but exit verification fails or shows another node, report the mismatch,
disconnect, and pause the paid balance unless the user explicitly asks to keep
the tunnel for diagnosis.

Report:

- connected or failed;
- selected node name and advertised location;
- purchased duration and remaining seconds;
- observed and expected exit IP;
- grace deadline;
- whether exit routing was verified.

Never report or print the client private key, MPP account key, daemon admin
token, or registry-write token.

## Recover safely

- **No matching/healthy node:** stop before payment.
- **Payment fails:** do not create another account or retry with another paid
  node without the user's direction.
- **Payment succeeds but connection fails:** ensure the logical-node session is
  paused and report that the unused balance remains available until its grace
  deadline. Retry a server-declared transitional response against the same
  logical node; never pay another node or fall back to an in-memory session.
- **Coordinator temporarily unavailable:** do not retry payment elsewhere. Keep
  the existing session response private and retry the same logical node only
  when its API reports healthy; durable mutations fail closed.
- **Exit verification fails:** tear down the local tunnel and pause the session.
- **Stream emits `payment-need-voucher`:** replenish the same channel with a
  newer voucher or an on-chain top-up; do not start a second paid session.
- **Temporary files:** remove only the exact files created for this workflow
  after they are no longer required.

Do not use administrative session lookup or deletion in any paid-client flow.

## Status and disconnect

Use only the client selected during preflight. Do not run both branches.

```bash
# Linux only
vpn-client status
vpn-client heartbeat
vpn-client disconnect
```

```bash
# macOS only
tempvpnctl status --json
tempvpnctl disconnect --json
```

Disconnect means local tunnel teardown plus server-side pause. Verify that the
visible public IP no longer equals the VPN node's expected exit IP. Do not
delete or revoke the paid session; it expires automatically when connected time
is exhausted or the grace deadline passes.

## Example

For:

```text
Connect 30 mins to Belgium
```

perform:

1. Parse `duration_seconds = 1800` and location `Belgium`.
2. Preflight the native client and MPP account.
3. Normalize Belgium to `BE` and call platform `select --country BE --json`;
   never forward the sentence to the indexer.
4. Exclude expired, unavailable, draining, or full nodes.
5. Ping the eligible shortlist from the user's Mac and select the lowest-latency
   healthy match.
6. Run the platform `check --node-url` command immediately before payment.
7. Pay that node—never the indexer or a node in another country.
8. Connect using the paid response and the same node URL.
9. Verify the observed public IP against the node's expected exit IP.
10. Report the selected Belgian node, remaining time, and verification result.
