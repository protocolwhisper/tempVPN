# GCP US VPN Node Terraform

This Terraform creates one GCP VM intended to run the `vpn-node-daemon` WireGuard egress node from `../vpnnode`.

Default location:

```text
region = us-central1
zone   = us-central1-a
```

That is GCP Iowa, United States.

## What It Creates

- VPC and subnet.
- Static public IP.
- Debian 12 Compute Engine VM.
- Firewall for WireGuard UDP `51820`.
- Optional firewall for daemon HTTP API TCP `8080`.
- Optional firewall for SSH TCP `22`.
- Startup script that installs WireGuard tools, creates `wg0`, enables IP forwarding, adds NAT, generates the server WireGuard keypair, writes `/etc/vpn-node-daemon/vpn-node.toml`, and creates a `vpn-node-daemon` systemd unit.

The startup script does not hardcode a WireGuard private key. The keypair is generated on first boot and kept on the VM under `/etc/wireguard`.

## Prereqs

Authenticate GCP locally:

```sh
gcloud auth application-default login
gcloud config set project YOUR_PROJECT_ID
```

Build a Linux daemon binary. For example, from `../vpnnode` on a Linux builder:

```sh
cargo build --release -p vpn-node-daemon
```

You can either upload the binary yourself after Terraform completes, or provide `daemon_binary_url` pointing at a private/restricted HTTPS URL for the binary.

## Configure

```sh
cd tunnel
cp terraform.tfvars.example terraform.tfvars
```

Edit:

```hcl
project_id = "your-gcp-project-id"
admin_token = "use-a-real-secret"
ssh_source_ranges = ["YOUR_PUBLIC_IP/32"]
admin_api_source_ranges = ["YOUR_PUBLIC_IP/32"]
```

Keep `terraform.tfvars` out of git because it contains the daemon admin token.

## Apply

```sh
terraform init
terraform plan
terraform apply
```

Outputs include:

```text
public_ip
wireguard_endpoint
daemon_url
ssh_hint
```

## If You Did Not Provide daemon_binary_url

Upload the daemon:

```sh
gcloud compute scp ../vpnnode/target/release/vpn-node-daemon \
  us-vpn-node:/tmp/vpn-node-daemon \
  --zone us-central1-a \
  --project YOUR_PROJECT_ID
```

SSH in and install/start it:

```sh
gcloud compute ssh us-vpn-node --zone us-central1-a --project YOUR_PROJECT_ID

sudo install -m 755 /tmp/vpn-node-daemon /usr/local/bin/vpn-node-daemon
sudo systemctl daemon-reload
sudo systemctl start vpn-node-daemon
sudo systemctl status vpn-node-daemon
```

Read the generated WireGuard server public key:

```sh
sudo cat /etc/wireguard/server_public.key
```

The daemon config already uses that key at:

```text
/etc/vpn-node-daemon/vpn-node.toml
```

## Local Client Test

In `../vpnnode`, create `agent-egress.toml`:

```toml
node_url = "http://GCP_PUBLIC_IP:8080"
admin_token = "same-admin-token"
proxy_addr = "127.0.0.1:1080"
status_file = "/tmp/agent-egress-status.json"
wg_quick_command = "wg-quick"
wg_command = "wg"
interface_name = "aegress0"
expected_exit_ip = "GCP_PUBLIC_IP"
```

Run:

```sh
sudo -E cargo run -p agent-egress-cli -- \
  --config agent-egress.toml \
  run --region us --duration 5m -- curl ifconfig.me
```

Expected output is the GCP VM public IP.

On the VM, verify the daemon and WireGuard peer:

```sh
sudo systemctl status vpn-node-daemon
sudo wg show
```

## Security Notes

- Restrict `admin_api_source_ranges` to your local public IP. Do not expose the daemon API to the whole internet for this MVP.
- Restrict `ssh_source_ranges` to your local public IP.
- The admin token protects session creation. It is not a WireGuard key.
- The client private WireGuard key is still generated locally by `agent-egress` and is never sent to the VM.
- This is not a payment layer yet.

## Destroy

```sh
terraform destroy
```
