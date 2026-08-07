locals {
  tags = [
    "vpn-client",
    "wireguard",
    "vpn-node-daemon",
  ]
}

resource "google_compute_network" "this" {
  name                    = var.network_name
  auto_create_subnetworks = false
}

resource "google_compute_subnetwork" "this" {
  name          = "${var.name}-subnet"
  ip_cidr_range = var.subnet_cidr
  network       = google_compute_network.this.id
  region        = var.region
}

resource "google_compute_address" "this" {
  name         = "${var.name}-ip"
  address_type = "EXTERNAL"
  region       = var.region
}

resource "google_compute_firewall" "wireguard" {
  name    = "${var.name}-wireguard"
  network = google_compute_network.this.name

  allow {
    protocol = "udp"
    ports    = [tostring(var.wireguard_port)]
  }

  source_ranges = var.wireguard_source_ranges
  target_tags   = local.tags
}

resource "google_compute_firewall" "daemon_admin_api" {
  count   = length(var.admin_api_source_ranges) > 0 ? 1 : 0
  name    = "${var.name}-daemon-api"
  network = google_compute_network.this.name

  allow {
    protocol = "tcp"
    ports    = [tostring(var.daemon_port)]
  }

  source_ranges = var.admin_api_source_ranges
  target_tags   = local.tags
}

resource "google_compute_firewall" "ssh" {
  count   = length(var.ssh_source_ranges) > 0 ? 1 : 0
  name    = "${var.name}-ssh"
  network = google_compute_network.this.name

  allow {
    protocol = "tcp"
    ports    = ["22"]
  }

  source_ranges = var.ssh_source_ranges
  target_tags   = local.tags
}

resource "google_compute_instance" "this" {
  name         = var.name
  machine_type = var.machine_type
  zone         = var.zone
  tags         = local.tags

  boot_disk {
    initialize_params {
      image = "debian-cloud/debian-12"
      size  = var.boot_disk_size_gb
      type  = "pd-balanced"
    }
  }

  network_interface {
    subnetwork = google_compute_subnetwork.this.id

    access_config {
      nat_ip = google_compute_address.this.address
    }
  }

  metadata = {
    enable-oslogin = "TRUE"
    startup-script = templatefile("${path.module}/scripts/startup.sh.tftpl", {
      admin_token_toml        = jsonencode(var.admin_token)
      node_id_toml            = jsonencode(var.node_id)
      node_name_toml          = jsonencode(var.node_name)
      node_region_toml        = jsonencode(var.region)
      public_api_url_toml     = jsonencode("http://${google_compute_address.this.address}:${var.daemon_port}")
      expected_exit_ip_toml   = jsonencode(google_compute_address.this.address)
      registry_mode           = var.registry_mode
      registry_url_toml       = jsonencode(var.registry_url)
      registry_token_toml     = jsonencode(var.registry_token)
      daemon_binary_url_shell = jsonencode(var.daemon_binary_url)
      daemon_port             = var.daemon_port
      endpoint_toml           = jsonencode("${google_compute_address.this.address}:${var.wireguard_port}")
      max_duration_seconds    = var.max_duration_seconds
      server_tunnel_ip        = var.server_tunnel_ip
      sweep_interval_seconds  = var.sweep_interval_seconds
      tunnel_cidr             = var.tunnel_cidr
      tunnel_cidr_toml        = jsonencode(var.tunnel_cidr)
      wireguard_port          = var.wireguard_port
    })
  }

  service_account {
    scopes = ["https://www.googleapis.com/auth/logging.write"]
  }

  allow_stopping_for_update = true

  depends_on = [
    google_compute_firewall.wireguard,
  ]
}
