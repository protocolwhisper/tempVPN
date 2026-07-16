use crate::node_client::Node;

pub fn endpoint_host_ip(endpoint: &str) -> Option<String> {
    let (host, _) = endpoint.rsplit_once(':')?;
    host.parse::<std::net::IpAddr>().ok()?;
    Some(host.to_string())
}

pub fn filter_region(nodes: Vec<Node>, region: Option<&str>) -> Vec<Node> {
    nodes
        .into_iter()
        .filter(|node| region.map(|region| node.region == region).unwrap_or(true))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn node(id: &str, region: &str) -> Node {
        Node {
            id: id.into(),
            name: id.into(),
            region: region.into(),
            api_url: format!("https://{id}.example"),
            wireguard_endpoint: "192.0.2.1:51820".into(),
            expected_exit_ip: "192.0.2.1".into(),
            lease_expires_at: Utc::now() + chrono::Duration::seconds(90),
        }
    }

    #[test]
    fn region_filter_is_exact_and_optional() {
        let nodes = vec![node("a", "eu"), node("b", "us")];
        assert_eq!(filter_region(nodes.clone(), Some("eu"))[0].id, "a");
        assert_eq!(filter_region(nodes, None).len(), 2);
    }
}
