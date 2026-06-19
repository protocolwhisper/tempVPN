output "public_ip" {
  description = "Static public IP of the US VPN node."
  value       = google_compute_address.this.address
}

output "wireguard_endpoint" {
  description = "WireGuard endpoint for client configs."
  value       = "${google_compute_address.this.address}:${var.wireguard_port}"
}

output "daemon_url" {
  description = "vpn-node-daemon URL. Reachable only if admin_api_source_ranges allows your IP."
  value       = "http://${google_compute_address.this.address}:${var.daemon_port}"
}

output "ssh_hint" {
  description = "SSH hint when OS Login is enabled."
  value       = "gcloud compute ssh ${var.name} --zone ${var.zone} --project ${var.project_id}"
}

output "server_public_key_command" {
  description = "Run on the VM to read the generated WireGuard server public key."
  value       = "sudo cat /etc/wireguard/server_public.key"
}
