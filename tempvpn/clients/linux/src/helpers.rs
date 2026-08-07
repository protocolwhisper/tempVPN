use crate::node_client::{DiscoveryFilters, Node};

pub fn endpoint_host_ip(endpoint: &str) -> Option<String> {
    let (host, _) = endpoint.rsplit_once(':')?;
    host.parse::<std::net::IpAddr>().ok()?;
    Some(host.to_string())
}

pub fn filter_nodes(nodes: Vec<Node>, filters: &DiscoveryFilters) -> Vec<Node> {
    nodes
        .into_iter()
        .filter(|node| {
            matches_text(node.country_code.as_deref(), filters.country.as_deref())
                && matches_text(node.city.as_deref(), filters.city.as_deref())
                && matches_text(Some(&node.region), filters.region.as_deref())
                && filters.available.is_none_or(|requested| {
                    requested
                        == (node.accepting_sessions == Some(true)
                            && node.available_slots.is_some_and(|slots| slots > 0))
                })
        })
        .collect()
}

fn matches_text(actual: Option<&str>, requested: Option<&str>) -> bool {
    requested.is_none_or(|requested| {
        actual.is_some_and(|actual| actual.trim().eq_ignore_ascii_case(requested.trim()))
    })
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
            country_code: None,
            subdivision_code: None,
            city: None,
            accepting_sessions: None,
            available_slots: None,
            api_url: format!("https://{id}.example"),
            wireguard_endpoint: "192.0.2.1:51820".into(),
            expected_exit_ip: "192.0.2.1".into(),
            lease_expires_at: Utc::now() + chrono::Duration::seconds(90),
        }
    }

    #[test]
    fn structured_filters_are_conjunctive_and_case_insensitive() {
        let mut germany = node("de", "eu");
        germany.country_code = Some("DE".into());
        germany.city = Some("Frankfurt".into());
        germany.accepting_sessions = Some(true);
        germany.available_slots = Some(2);
        let mut france = node("fr", "eu");
        france.country_code = Some("FR".into());
        france.city = Some("Paris".into());
        france.accepting_sessions = Some(true);
        france.available_slots = Some(2);
        let filters = DiscoveryFilters::new(Some("de"), Some("frankfurt"), Some("EU")).unwrap();
        let candidates = vec![france, germany];
        let selected = filter_nodes(candidates.clone(), &filters);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].id, "de");

        let global = DiscoveryFilters::new(None, None, None).unwrap();
        let globally_eligible = filter_nodes(candidates, &global);
        assert_eq!(globally_eligible.len(), 2);
        assert!(globally_eligible.iter().any(|node| node.id == "fr"));
    }

    #[test]
    fn availability_filter_excludes_legacy_and_full_nodes() {
        let legacy = node("legacy", "eu");
        let mut full = node("full", "eu");
        full.accepting_sessions = Some(true);
        full.available_slots = Some(0);
        assert!(filter_nodes(
            vec![legacy, full],
            &DiscoveryFilters::new(None, None, None).unwrap()
        )
        .is_empty());
    }
}
