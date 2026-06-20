# tempVPN

tempVPN is a Codex skill and Rust client for buying a short-lived WireGuard VPN
session with a Tempo MPP payment, connecting the local Mac, and verifying the
resulting public IP.

> [!IMPORTANT]
> The supported end-to-end client workflow currently runs on **macOS only**.
> Other build targets and server-side Linux code in this repository are
> development artifacts, not supported client platforms yet.

## How it works

```text
User prompt
    |
    v
Codex + tempvpn/SKILL.md
    |
    v
buy-and-connect-macos.sh
    |-- creates an ephemeral WireGuard keypair
    |-- asks mppx to pay POST /sessions with the `main` account
    |-- receives a temporary VPN session
    |-- opens the native macOS administrator dialog
    |-- starts the local WireGuard tunnel with vpn-client
    `-- verifies the active interface and public exit IP
```

Only the WireGuard **public** key is sent to the VPN node. The private key stays
on the Mac in a restricted temporary directory and is removed when the launcher
finishes. The paid server-side peer expires automatically.

## Repository parts

| Path | What it does | Why it matters |
| --- | --- | --- |
| `tempvpn/SKILL.md` | Teaches Codex when and how to buy, connect, verify, and disconnect tempVPN. | This is the workflow the agent loads; the README alone does not control agent behavior. |
| `tempvpn/scripts/buy-and-connect-macos.sh` | Runs the supported macOS purchase-and-connect flow. | It keeps key generation, payment, elevation, connection, and verification in one repeatable command. |
| `tempvpn/scripts/connect-with-admin.applescript` | Opens the native macOS administrator prompt and runs the privileged connection step. | WireGuard needs permission to create and route traffic through a network interface. |
| `tempvpn/crates/vpn-client-cli` | Builds the local `vpn-client` executable. | It creates/removes the WireGuard interface, writes local status, verifies the exit IP, and can run a child command through a SOCKS5 proxy. |
| `tempvpn/crates/vpn-node-daemon` | Runs the paid session API on the VPN server. | It validates the MPP payment, allocates an address, adds the temporary WireGuard peer, and removes expired peers. Normal clients never need its admin token. |
| `tempvpn/configs` | Example client, daemon, and WireGuard server configuration. | These are deployment/development templates; they are not required for the default macOS demo. |
| `tunnel_terraform` | Provisions the cloud VPN-node infrastructure. | This is for operators deploying the server, not for users connecting from a Mac. |
| `rust`, `foundry` | Tempo MPP and Account Keychain examples used by the wider workspace. | They support development and testing but are not part of the normal tempVPN client setup. |

More implementation detail is in [`tempvpn/README.md`](tempvpn/README.md).

## Prerequisites

The supported flow requires all of the following:

1. **macOS with administrator access**

   The launcher uses `osascript` to show a native administrator dialog.
   Administrator permission is required to create the WireGuard interface and
   change system routes.

2. **Codex CLI, IDE extension, or app**

   Codex discovers and loads `SKILL.md`. You can also run the launcher manually,
   but the natural-language workflow depends on Codex.

3. **WireGuard command-line tools**

   The `wg` command creates the ephemeral keypair and inspects the tunnel.

   ```bash
   brew install wireguard-tools
   wg --version
   ```

4. **Rust and Cargo**

   Cargo builds the local `vpn-client` binary used by the launcher.
   Install Rust from [rustup.rs](https://rustup.rs/), then verify it:

   ```bash
   rustc --version
   cargo --version
   ```

5. **Node.js/npm and `mppx`**

   `mppx` handles the Tempo MPP payment challenge returned by `POST /sessions`.

   ```bash
   node --version
   npm --version
   npm install -g mppx
   mppx --help
   ```

6. **An MPPX account named `main` with suitable Tempo funds**

   Follow the [MPP agent quickstart](https://mpp.dev/quickstart/agent) to create
   and fund the account during initial setup:

   ```bash
   mppx account create --account main
   mppx account view --account main
   ```

   On macOS, MPPX stores account material in Keychain. Create or replace an
   account only as an explicit setup action in a trusted terminal. Never paste
   its private key into chat, commit it, or create a replacement automatically
   after a launcher failure.

7. **Network access to the configured VPN node**

   The current demo contacts `http://34.30.107.52:8080` for session creation and
   connects WireGuard to the endpoint returned by that service.

## Install and build

```bash
git clone https://github.com/protocolwhisper/tempVPN.git
cd tempVPN/tempvpn
cargo build -p vpn-client-cli
./target/debug/vpn-client --help
```

The supported launcher expects the debug binary at
`tempvpn/target/debug/vpn-client`, so run the build command before the first
connection.

## Load the skill into Codex

Installing a skill and invoking it are separate steps:

- **Install/discover:** place the skill directory in a location Codex scans.
- **Invoke/load:** mention the installed skill in a prompt so Codex reads the
  full `SKILL.md` and follows it.

These locations and invocation methods follow the official
[Codex skills documentation](https://developers.openai.com/codex/skills).

### Option 1: install with Codex

In a Codex conversation, enter:

```text
$skill-installer Install the skill from https://github.com/protocolwhisper/tempVPN/tree/main/tempvpn
```

Codex detects newly installed skills automatically. If `tempvpn` does not
appear, restart Codex.

### Option 2: link a local clone

For a user-level skill available in every repository:

```bash
mkdir -p "$HOME/.agents/skills"
ln -s "/absolute/path/to/tempVPN/tempvpn" "$HOME/.agents/skills/tempvpn"
```

For a skill available only in one repository, link it under that repository:

```bash
mkdir -p .agents/skills
ln -s "/absolute/path/to/tempVPN/tempvpn" .agents/skills/tempvpn
```

Use an absolute path and do not move the clone after creating the link. Codex
supports symlinked skill directories. Restart Codex if the skill is not detected
after the link is created.

### Verify and invoke the skill

In Codex CLI or the IDE extension, use `/skills` or type `$` and confirm that
`tempvpn` appears. Then invoke it explicitly:

```text
$tempvpn Buy 30 minutes of VPN access with Tempo, connect this Mac, and verify the public IP.
```

The description also supports implicit prompts such as:

```text
Load tempvpn and connect for 30 minutes.
```

Explicit `$tempvpn` invocation is preferable for the first run because it makes
the intended workflow unambiguous.

## What happens during a connection

1. Codex loads `tempvpn/SKILL.md` and selects the macOS launcher.
2. The launcher checks for macOS, `wg`, `mppx`, `osascript`, the `main` MPPX
   account, and the compiled `vpn-client`.
3. It creates a one-time WireGuard keypair locally.
4. `mppx` pays the API challenge and submits the public key plus requested
   duration.
5. The server returns the assigned tunnel address, server public key, endpoint,
   and expiry time.
6. macOS asks for the administrator password and `vpn-client` brings up the
   tunnel.
7. `vpn-client status` reports the interface, session, exit IP, and expiration.

Do not run the launcher once inside a sandbox and then retry it outside the
sandbox. MPPX account discovery needs access to the real macOS Keychain on the
first attempt.

## Run without the agent

From the `tempvpn` directory:

```bash
./scripts/buy-and-connect-macos.sh 30m
```

The duration accepts seconds (`30` or `30s`), minutes (`30m`), or hours (`1h`).
The launcher purchases access, connects, and verifies status as one operation.

To disconnect the local tunnel:

```bash
sudo ./target/debug/vpn-client disconnect
```

Disconnecting removes local VPN state only. It does not request a refund or
delete the paid server session; the server peer expires automatically.

## Current limitations

- The supported client workflow is macOS only.
- The demo API currently uses plain HTTP; production deployments should add TLS.
- The current service is a single VPN region/node.
- A session is time-limited and its payment is not reversed by disconnecting.
- Windows and Linux release/build artifacts are not supported end-to-end client
  flows yet.

## Security rules

- Never send or commit WireGuard or MPPX private keys.
- Use the MPPX account named `main`; do not rely on an arbitrary default account.
- Never give a client or agent the VPN daemon admin token.
- Do not call session revoke/delete endpoints during normal client use.
- Verify the reported public exit IP before claiming the VPN is active.
