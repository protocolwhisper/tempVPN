# tempVPN

tempVPN is a Codex skill that buys a temporary WireGuard VPN session with a
Tempo MPP payment, connects the local machine, and verifies the public IP.

> [!IMPORTANT]
> The client workflow currently supports **macOS only**.

## How it works

```text
Codex loads SKILL.md
  -> macOS launcher creates an ephemeral WireGuard keypair
  -> mppx pays POST /sessions with the `main` account
  -> VPN node returns a temporary session
  -> macOS requests administrator permission
  -> vpn-client connects and verifies the exit IP
```

Only the WireGuard public key is sent to the VPN node. The private key remains
local, and the server removes the peer automatically when the session expires.

## The three important parts

1. **The Codex skill** (`tempvpn/SKILL.md`)

   Turns a request such as “connect for 30 minutes” into the correct workflow.
   It tells the agent how to pay, connect, verify the exit IP, and disconnect
   without exposing private keys or using server admin credentials.

2. **The macOS client** (`buy-and-connect-macos.sh` and `vpn-client`)

   Runs on the user's Mac. It creates a temporary WireGuard keypair, uses
   `mppx` to pay for the session, requests administrator permission, starts the
   tunnel, and confirms that traffic is using the VPN.

3. **The VPN node** (`vpn-node-daemon`)

   Runs on the remote server. It validates the Tempo MPP payment, creates a
   temporary WireGuard peer, returns the connection details, and removes the
   peer automatically when the purchased time expires.

See [tempvpn/README.md](tempvpn/README.md) for implementation and server details.

## Prerequisites

- **macOS administrator access** — required to create the WireGuard interface
  and change routes.
- **Codex** — discovers and loads the skill for natural-language operation.
- **WireGuard tools** — provides `wg` and `wg-quick`.
- **Rust/Cargo** — builds the local `vpn-client`.
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
```

## Load the skill into Codex

Ask Codex to install it:

```text
$skill-installer Install the skill from https://github.com/protocolwhisper/tempVPN/tree/main/tempvpn
```

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
$tempvpn Buy 30 minutes of VPN access, connect this Mac, and verify the public IP.
```

You can also say: `Load tempvpn and connect for 30 minutes.`

## Current scope

The supported client workflow is macOS only. Never share private keys or daemon
admin credentials with the client or agent.
