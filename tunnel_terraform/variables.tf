variable "project_id" {
  description = "GCP project ID where the VPN node will be created."
  type        = string
}

variable "region" {
  description = "GCP region. us-central1 is Iowa, United States."
  type        = string
  default     = "us-central1"
}

variable "zone" {
  description = "GCP zone. Must belong to var.region."
  type        = string
  default     = "us-central1-a"
}

variable "name" {
  description = "Name prefix for the VPN node resources."
  type        = string
  default     = "us-vpn-node"
}

variable "machine_type" {
  description = "GCE machine type for the VPN node."
  type        = string
  default     = "e2-small"
}

variable "boot_disk_size_gb" {
  description = "Boot disk size in GB."
  type        = number
  default     = 20
}

variable "ssh_source_ranges" {
  description = "CIDR ranges allowed to SSH to the node. Restrict this to your IP."
  type        = list(string)
  default     = []
}

variable "admin_api_source_ranges" {
  description = "CIDR ranges allowed to call vpn-node-daemon on TCP 8080. Use 0.0.0.0/0 when paid session purchase must be publicly reachable."
  type        = list(string)
  default     = ["0.0.0.0/0"]
}

variable "wireguard_source_ranges" {
  description = "CIDR ranges allowed to reach WireGuard UDP 51820."
  type        = list(string)
  default     = ["0.0.0.0/0"]
}

variable "admin_token" {
  description = "Admin token for vpn-node-daemon. Pass through TF_VAR_admin_token or terraform.tfvars; do not commit it."
  type        = string
  sensitive   = true
}

variable "daemon_binary_url" {
  description = "Optional HTTPS URL for a prebuilt vpn-node-daemon Linux binary. If empty, upload /usr/local/bin/vpn-node-daemon manually."
  type        = string
  default     = ""
}

variable "daemon_port" {
  description = "TCP port for vpn-node-daemon."
  type        = number
  default     = 8080
}

variable "wireguard_port" {
  description = "UDP port for WireGuard."
  type        = number
  default     = 51820
}

variable "tunnel_cidr" {
  description = "WireGuard tunnel CIDR. The Rust MVP allocator currently expects /24."
  type        = string
  default     = "10.8.0.0/24"
}

variable "server_tunnel_ip" {
  description = "WireGuard server interface address."
  type        = string
  default     = "10.8.0.1/24"
}

variable "max_duration_seconds" {
  description = "Maximum VPN session duration accepted by the daemon."
  type        = number
  default     = 3600
}

variable "sweep_interval_seconds" {
  description = "How often the daemon checks for expired sessions."
  type        = number
  default     = 10
}

variable "network_name" {
  description = "Name of the VPC to create."
  type        = string
  default     = "vpn-client-vpc"
}

variable "subnet_cidr" {
  description = "CIDR for the VM subnet."
  type        = string
  default     = "10.20.0.0/24"
}
