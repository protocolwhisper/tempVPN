---
name: tempvpn
description: Securely bootstrap, preflight, connect, buy, resume, verify, inspect, and disconnect portable TempVPN balances through the registry control plane and Tempo MPP on Linux or macOS. Use for requests such as "Connect 30 mins to Belgium", "install TempVPN", "get me the fastest VPN in Singapore", "what does TempVPN need", "show VPN status", or "disconnect the VPN". Downloads only verified native releases, selects the fastest eligible node, reuses unexpired balance across nodes, keeps session capabilities and private keys local, and pauses unused balance on disconnect.
---

# tempVPN

Turn a plain-language request into a safe, complete temporary VPN workflow:

```text
resolve intent → preflight → query registry → shortlist available nodes
→ client latency-rank → reuse saved balance or pay registry
→ connect through registry → verify exit → report
```

## Canonical source and updates

The canonical skill bundle is
`https://github.com/protocolwhisper/tempVPN/tree/main/tempvpn/agent`. Its
entrypoint is
`https://raw.githubusercontent.com/protocolwhisper/tempVPN/main/tempvpn/agent/SKILL.md`.
Keep these URLs in user-facing setup guidance so an installed copy can always
identify its maintained source.

For live service behavior, prefer the machine-readable references maintained
by the registry:

- Human docs: `https://tempvpn.xyz/docs/`
- Agent-readable Markdown: `https://registry.tempvpn.xyz/docs/markdown.md`
- OpenAPI: `https://registry.tempvpn.xyz/openapi.json`
- Compact agent reference: `https://registry.tempvpn.xyz/llms.txt`

Treat the registry Markdown and OpenAPI documents as authoritative when the
website prose lags a deployment. Do not infer node-direct payment routing from
an older copy of the human docs.

Do not silently replace this skill during setup, connection, or payment. When
the user asks to install, update, or check for updates, compare the installed
bundle with the canonical `main` bundle, explain whether it differs, and obtain
approval before replacing local files. Update `SKILL.md` and `scripts/` from
the same repository commit; never mix an updated entrypoint with older
verification scripts. Validate the downloaded frontmatter has `name: tempvpn`
and that both bootstrap and package-verification scripts are present before
activating it. A skill update never authorizes client installation, account
changes, or VPN payment.

In a verified TempVPN checkout, `agent/SKILL.md` is the source file. An installed
skill may be a symlink to it, so resolve links before creating or editing a
second copy.

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

Never send, store, or log the user's raw prompt in the registry or node request.
Only pass the normalized fields supported by the platform client.

Requests such as “connect,” “start,” or “use” include purchase, local
connection, and exit-IP verification. Stop after purchase only when the user
explicitly asks to buy without connecting.

Treat “what does TempVPN need?” or “is TempVPN ready?” as read-only preflight.
Treat “set up TempVPN” as authorization to inspect first and then perform only
the separately approved installation or account-provisioning actions; it never
authorizes a VPN purchase.

Treat “Connect 30 mins to Belgium” as authorization to obtain 1,800 seconds of
usable balance for a Belgian connection. Reuse a sufficient saved balance first;
only its absence authorizes a new 1,800-second purchase. Still use any approval
mechanism required by the environment for payment, installation, or
administrator access. Do not ask for a redundant confirmation after successful
payment; continue immediately to connection.

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

## Use the registry as the control plane

Use the registry for every public fixed-session operation, not only discovery.
The canonical production origin is `https://registry.tempvpn.xyz`. Send
structured `country`, `city`, `region`, and `available` query parameters through
the platform client—never the raw prompt. The registry returns currently leased
nodes matching all supplied filters.

The public fixed-session contract is:

```text
GET  <registry>/nodes
POST <registry>/sessions
POST <registry>/sessions/{session_id}/connect
POST <registry>/sessions/{session_id}/pause
POST <registry>/sessions/{session_id}/heartbeat
GET  <registry>/sessions/{session_id}/status
```

All payment challenges, receipts, and fixed-session lifecycle requests stay at
the registry origin. A selected node's `api_url` may be contacted directly only
for read-only latency and health probes. Never use it as the public payment or
lifecycle origin.

Choose the registry URL in this order:

1. Preserve an explicit `VPN_CLIENT_REGISTRY_URL` or equivalent client option.
2. Otherwise use `https://registry.tempvpn.xyz` only when its `/health`
   endpoint succeeds with valid TLS.
3. If the registry is unavailable, stop before payment or mutation and report
   the control-plane outage. Do not substitute a node origin or silently narrow
   a global request to one region.

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

Use the registry to:

1. Filter for the requested country, city, or region.
2. Remove expired, unavailable, draining, or capacity-exhausted nodes.
3. Produce a small eligible shortlist.

Then health-check and latency-rank the shortlist from the user's machine. Do not
select solely from a ping measured by the registry: that latency describes the
registry-to-node path, not the user-to-node path.

Perform one final direct, read-only node health check immediately before a
purchase or connection. The registry remains the authority for routing the
session operation.

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

- A healthy production global registry or an explicit trusted registry
  override.
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
test it. Ensure the account and the registry's MPP challenge use the same Tempo
network and payment currency.

Account provisioning is separate from connection. Never automatically create,
fund, replace, export, or repair an MPPX account after a failed check or
payment. If the user explicitly requests account setup, perform it as a
separate visible action and do not expose command output that could contain
account or private-key material. `mppx account fund` supplies testnet tokens
only; never present it as a production/mainnet funding method.

On macOS, if either installed product is missing, offer the official native
bootstrap before discovery or payment. Explain that it downloads a notarized
package from the TempVPN GitHub release, verifies its checksum, Developer ID
Installer signature, stapled notarization ticket, Apple team `T4295L8LL4`,
bundle identifiers, nested code signatures, Network Extension entitlement, and
shared Keychain group, then requires a normal macOS administrator prompt.

After the user approves the download, create a private temporary directory and
run `scripts/bootstrap-macos-client.sh --destination PRIVATE_DIR` relative to
this skill. Do not override its manifest URL unless the user explicitly names a
trusted developer/test release. Parse its JSON locally. Proceed only when it
returns `ready_to_install: true`.

Request installation approval separately, then open the verified `.pkg` with
the macOS Installer UI. Never ask for, receive, or type the administrator
password. Wait for the user to complete the system prompt, launch
`/Applications/TempVPN.app` once with `open -gj`, and rerun full preflight.
Remove only the exact private temporary directory after installation succeeds
or the user cancels. Expect one-time VPN profile approval on first connection.

Use a development checkout only for maintainer work when the release is
unavailable. Unsigned, ad-hoc, or Apple Development artifacts under `target/`
may validate compilation but must never be installed for an end user or used
for a paid session.

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
| macOS | signed `tempvpnctl` plus `/Applications/TempVPN.app` | Bootstrap the verified notarized package with `scripts/bootstrap-macos-client.sh`, then request approval to open it in macOS Installer. |

Do not run or present both platform branches. A development checkout does not
mean the CLI is installed: resolve `command -v vpn-client` or
`command -v tempvpnctl` first, then use only the verified installed command (or
the explicitly built Linux path). If neither the required binary nor a trusted
source/release is available, stop and report the missing client before
discovery or payment.

If the official signed macOS release is temporarily unavailable, maintainers
may use a locally built, development-signed install; a free Apple ID is enough
(no paid Apple Developer Program required):

1. On the user's Mac, install Xcode and Go (`brew install go`) and clone the
   trusted source checkout.
2. Add any free Apple ID under Xcode > Settings > Accounts; it yields a
   Personal Team with a team ID.
3. Build signed products:

```bash
export APPLE_DEVELOPMENT_TEAM="TEAMID"
export CODE_SIGN_IDENTITY="Apple Development"
./clients/macos/build-macos-products.sh
```

4. Request approval, then install with `sudo ./clients/macos/install-tempvpnctl.sh`.

A development-signed local build is signed, passes the installer's signature
verification, and runs on that Mac without notarization; locally built apps
carry no Gatekeeper quarantine. If Xcode reports the bundle identifier
`com.tempo.tempvpn` as unavailable for the personal team, pick a unique bundle
ID prefix for the app and Packet Tunnel targets before building. Do not use
unsigned `target/` artifacts for a tunnel.

## Distribute clients for users

Do not tell end users to install a Rust crate or build a checkout. This workspace
marks the client package `publish = false`; source is a maintainer input, not a
stable distribution channel.

- **Linux:** CI builds `vpn-client` for each supported target with
  `cargo build --release -p vpn-client-cli`, packages each binary with a
  version and target name, and publishes SHA-256 checksums. Users still need
  the platform's `wg` and `wg-quick` packages; those are OS dependencies, not
  Rust crates.
- **macOS:** maintainers publish `TempVPN-VERSION-macos-ARCH.pkg` plus
  `tempvpn-macos-manifest.json` in the same GitHub release. The stable manifest
  is `https://github.com/protocolwhisper/tempVPN/releases/latest/download/tempvpn-macos-manifest.json`.
  Never distribute unsigned, Apple Development, ad-hoc, unstapled, or
  differently signed artifacts.
- **Payment:** `mppx` remains a separate Node/npm CLI and is not bundled into
  either VPN client. If it is absent, explain the approved installation of the
  pinned/approved `mppx` release separately.

Treat the bundled verifier and pinned Apple identity as the trust policy. A
checksum from the manifest alone is insufficient. If download, signature,
notarization, identifier, entitlement, architecture, version, or checksum
verification fails, delete the exact temporary directory, stop before
installation and payment, and report `needs native client signing/install`.

## Select the node

The platform client queries the registry with structured filters, removes nodes
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
JSON result and retain its exact `node_id`, `node_url`, structured location, and
`expected_exit_ip` for routing, health checks, connection, verification, and
reporting. `node_id` selects the destination through the registry; `node_url`
does not become the control-plane origin.

An explicit node URL may bypass catalog ranking but must still pass its health
check and resolve to a live catalog entry with a stable `node_id`. Do not use an
unregistered URL for a portable fixed session.

## Retain and reuse the session capability

`POST /sessions` returns a `session_id`. Treat it as a secret bearer capability:
it authorizes status, connection, heartbeat, and pause operations without a
wallet lookup. Never print it, put it in a prompt, telemetry, shell history, or
a predictable shared filename.

The native client must import the paid response into a private persistent
session store containing at least:

```text
session_id, registry_url, remaining_seconds, grace_deadline,
last_node_id, last_node_url, local_private_key_reference
```

The registry deliberately cannot enumerate a wallet's sessions. Before buying,
ask the native client for locally saved sessions and validate candidates with
`GET <registry>/sessions/{session_id}/status`. Reuse a paused, unexpired session
with sufficient remaining balance. If no compatible saved session exists, make
one authorized purchase. Never pay again merely because the user selected a
different node.

If the installed native client cannot persist, list, and resume registry-backed
sessions, report that it is incompatible and stop before payment. Do not work
around this by tracking a session capability only in conversational memory.

## Purchase or resume and connect

Create a private temporary working directory and restrict its permissions. Use
it only while importing a new paid response or holding a Linux key file; do not
use predictable shared filenames. Delete the temporary paid response only after
the native client confirms durable import.

Immediately before a purchase or resume, perform the direct read-only health
check against the selected node:

```bash
# Linux only
vpn-client check --node-url "$SELECTED_NODE_URL" --json
```

```bash
# macOS only
tempvpnctl check --node-url "$SELECTED_NODE_URL" --json
```

Run only the command for the selected platform. If it fails, stop without
paying or connecting; a catalog result is an advisory snapshot, not a capacity
reservation. Direct node access ends after this check.

When no reusable session exists, payment targets the registry and names the
selected node in the request body:

```bash
mppx "$REGISTRY_URL/sessions" \
  --account main \
  --json-body '{"node_id":"belgium","duration_seconds":1800}' \
  --silent
```

Construct the JSON from normalized values rather than interpolating untrusted
text. Save the response without logging it, import it into the native client's
persistent store, and retain its `session_id`. The runtime HTTP 402 challenge is
authoritative for price, currency, network, and payment terms; do not describe
the fixed-session price using a guessed per-minute `amountHint`.

Payment creates a paused, globally portable usage balance. Connected time begins
on activation. A later resume can target a different healthy `node_id` and may
return a different WireGuard endpoint, server key, assigned IP, and node URL
while retaining the same `session_id` and remaining balance.

### Linux

Generate the private key locally and make it owner-readable only:

```bash
wg genkey
chmod 600 PRIVATE_KEY_PATH
```

Connect the imported or reused session through the registry:

```bash
sudo vpn-client connect \
  --registry-url "$REGISTRY_URL" \
  --node-id "$SELECTED_NODE_ID" \
  --session-response SESSION_RESPONSE_PATH \
  --private-key-path PRIVATE_KEY_PATH
```

For a reused session, use the client's saved-session option instead of
`--session-response`. Never reconstruct a paid response from remembered fields.

### macOS

The signed native client generates the private key in Keychain and sends only
its public key:

```bash
tempvpnctl connect \
  --registry-url "$REGISTRY_URL" \
  --node-id "$SELECTED_NODE_ID" \
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

The native client sends only `node_id` and its locally derived public key to
`POST <registry>/sessions/{session_id}/connect`. It must store the returned node
metadata and use the registry for heartbeat, status, and pause. It must not
reject a valid response merely because `node_url` changed during a paused
cross-node resume.

macOS may request one-time VPN profile approval on the first connection. This is
not a recurring private-key or biometric prompt.

## Keep streaming separate (Tempo Session v2)

Some nodes also offer metered streaming access at:

```text
POST /sessions/stream
Content-Type: application/json

{"client_public_key":"<wireguard-public-key>","duration_seconds":<safety-cap>}
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
with `client_public_key` and `duration_seconds` as query parameters to submit
channel operations (voucher, top-up, or close) without consuming the SSE body.
The same logical session resumes once the new state verifies. If the client
does not replenish within the node's grace period, the peer is removed and the
stream ends with a final receipt. `GET /sessions/stream` is never payable and
must return `405 Method Not Allowed`.

If the node answers `404` with "streaming payments are disabled", fall back to
the fixed `POST /sessions` flow only before any streaming payment is accepted.

Do not use streaming for the normal TempVPN workflow until the installed client
explicitly supports its receipt validation, interrupted-payment recovery, and
node affinity. Prefer `POST /sessions` for portable balance and cross-node
resume. Never silently fall back from one product to the other after payment.

The streaming channel receipt and stream session ID are not fixed-session
capabilities: do not place them in the fixed-session store, use them with fixed
lifecycle routes, or assume their reserve is portable to another node.

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
- requested duration, whether balance was reused or purchased, and remaining
  seconds;
- observed and expected exit IP;
- grace deadline;
- whether exit routing was verified.

Never report or print the client private key, MPP account key, daemon admin
token, or registry-write token.

## Recover safely

- **No matching/healthy node:** stop before payment.
- **Payment fails:** do not create another account or retry with another paid
  node without the user's direction.
- **Payment succeeds but connection fails:** persist the `session_id`, pause it
  through the registry, and report that unused balance remains available until
  its grace deadline. Re-select a healthy eligible node and resume the same
  session; never pay again or fall back to an in-memory session.
- **Selected node fails after pause:** re-query the registry, select another
  eligible node, and connect the saved `session_id` with that new `node_id`.
- **Registry or coordinator temporarily unavailable:** do not retry payment at
  a node origin. Keep the saved session capability private and retry through
  the registry when its control plane is healthy; durable mutations fail
  closed.
- **Exit verification fails:** tear down the local tunnel and pause the session.
- **Stream emits `payment-need-voucher`:** replenish the same channel with a
  newer voucher or an on-chain top-up; do not start a second paid session.
- **Temporary files:** after confirmed durable import, remove only the exact
  temporary files created for this workflow. Never remove the native client's
  persistent session record merely because the tunnel was disconnected.

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

The client resolves the saved `session_id` and `registry_url`; the agent should
not ask the user to provide them again. Status and heartbeat go through the
registry. Disconnect means local tunnel teardown plus registry-side pause.
Verify that the visible public IP no longer equals the VPN node's expected exit
IP. Do not delete the persistent record or revoke the paid session; it expires
automatically when connected time is exhausted or the grace deadline passes.

## Example

For:

```text
Connect 30 mins to Belgium
```

perform:

1. Parse `duration_seconds = 1800` and location `Belgium`.
2. Preflight the native client and MPP account.
3. Normalize Belgium to `BE` and call platform `select --country BE --json`;
   never forward the sentence to the registry.
4. Exclude expired, unavailable, draining, or full nodes.
5. Ping the eligible shortlist from the user's Mac and select the lowest-latency
   healthy match.
6. Check the private local session store and validate reusable balances through
   the registry.
7. Run platform `check --node-url` against the selected node immediately before
   purchase or resume.
8. Reuse a suitable saved `session_id`; otherwise pay `POST <registry>/sessions`
   with `node_id = belgium` and import the returned capability durably.
9. Connect the saved session through the registry with the selected `node_id`.
10. Verify the observed public IP against the node's expected exit IP and
    report whether balance was reused or purchased.
