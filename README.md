# tempVPN

tempVPN is a Codex skill that buys a temporary WireGuard VPN session with a
Tempo MPP payment, connects the local machine, tracks connected-time usage, and
verifies the public IP.

> [!IMPORTANT]
> Linux uses the Rust CLI with WireGuard command-line tools. macOS uses the
> Swift `tempvpnctl` plus an invisible native host and Packet Tunnel extension
> backed by WireGuardKit. Windows is not supported.

## How it works

```text
Node daemons --authenticated leases--> registry-mode daemon
Clients fetch /nodes, filter and latency-rank healthy nodes
Agent pays the selected node's POST /sessions directly with mppx
Linux vpn-client or macOS tempvpnctl imports that node-bound response
```

Only the WireGuard public key is sent when activating the paid balance. The
private key remains local, and the server removes the peer automatically when
connected time is exhausted or the grace deadline passes.

## The three important parts

1. **The Codex skill** (`tempvpn/SKILL.md`)

   Turns a request such as “connect for 30 minutes” into the correct workflow.
   It tells the agent how to pay, connect, verify the exit IP, and disconnect
   without exposing private keys or using server admin credentials.

2. **The platform clients** (`vpn-client` on Linux; native headless
   `TempVPN.app`/`tempvpnctl` on macOS)

   Select a healthy node, import the paid response, retain private keys locally,
   start the tunnel, heartbeat usage, and pause unused time on disconnect.

3. **The VPN node and registry** (`vpn-node-daemon`)

   Runs on the remote server. It validates the Tempo MPP payment, creates a
   paid usage balance, activates temporary WireGuard peers, and removes peers
   automatically when purchased connected time expires. Optional registry mode
   maintains authenticated 90-second node leases without proxying payments.

See [tempvpn/README.md](tempvpn/README.md) for implementation and server details.

## Prerequisites

- **A configured registry URL** — distributed to both platform clients.
- **Codex** — discovers and loads the skill for natural-language operation.
- **WireGuard tools and Rust/Cargo** — required for the Linux CLI.
- **Xcode, Go, WireGuardKit, and Apple signing** — required for the native
  headless macOS client. It has no graphical interface.
- **Node.js/npm and `mppx`** — handles the Tempo MPP payment.
- **A funded MPPX account named `main`** — used for VPN payments and stored in
  macOS Keychain.

Install the command-line dependencies:

```bash
brew install wireguard-tools
npm install -g mppx
```

Install Rust from [rustup.rs](https://rustup.rs/). Follow the
[MPP agent quickstart](https://mpp.dev/quickstart/agent) to create and fund the
`main` account:

```bash
mppx account create --account main
mppx account view --account main
```

Create or replace an MPPX account only as an explicit setup action in a trusted
terminal. Never share or commit its private key.

## Install and build

```bash
git clone https://github.com/protocolwhisper/tempVPN.git
cd tempVPN/tempvpn
cargo build -p vpn-client-cli
./clients/macos/build-macos-products.sh
```

Before signing, the macOS build is for compilation verification only. Once the
app, Packet Tunnel extension, and CLI are signed with the same Apple team, the
installer places the invisible host in `/Applications` and `tempvpnctl` in
`/usr/local/bin`.

## Load the skill into Codex

Ask Codex to install it:

```text
$skill-installer Install the skill from https://github.com/protocolwhisper/tempVPN/tree/main/tempvpn/agent
```

The skill records that same canonical bundle URL and the raw `SKILL.md` URL in
its own instructions. To check for updates later, ask Codex to compare the
installed `tempvpn` bundle with that canonical source and reinstall it after
showing you the changes. The entrypoint and verification scripts must always
come from the same repository commit.

Alternatively, link a local clone into the user skill directory:

```bash
mkdir -p "$HOME/.agents/skills"
ln -s "/absolute/path/to/tempVPN/tempvpn" "$HOME/.agents/skills/tempvpn"
```

Codex normally detects the skill automatically. Restart Codex if it does not
appear in `/skills`. These locations follow the official
[Codex skills documentation](https://developers.openai.com/codex/skills).

Invoke the skill explicitly:

```text
$tempvpn Buy 30 minutes of VPN access, connect, and verify the public IP.
```

You can also say: `Load tempvpn and connect for 30 minutes.`

## Current scope

Linux and macOS are supported. Never share private keys, daemon admin tokens,
or registry-write tokens with the client or agent.
