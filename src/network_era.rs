use serde::Serialize;

// FUTURE_IDEA_ONLY: any miner incentive campaigns should remain non-consensus
// messaging/UX until explicitly reviewed as a separate economic change.
pub const EARLY_MINER_ERA_END_HEIGHT: u64 = 25_000;
pub const GROWTH_ERA_END_HEIGHT: u64 = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct NetworkEraInfo {
    pub era_name: &'static str,
    pub era_description: &'static str,
    pub era_tagline: Option<&'static str>,
    pub early_participation_signal: bool,
}

pub fn network_era(height: u64) -> NetworkEraInfo {
    match height {
        0..=EARLY_MINER_ERA_END_HEIGHT => NetworkEraInfo {
            era_name: "Early Miner Era",
            era_description:
                "Irium is still in its early miner phase. Early participants are helping secure and shape the network.",
            era_tagline: Some("Early participants are helping secure the network."),
            early_participation_signal: true,
        },
        h if h <= GROWTH_ERA_END_HEIGHT => NetworkEraInfo {
            era_name: "Growth Era",
            era_description:
                "Irium is expanding beyond its bootstrap phase as network usage, block production, and infrastructure continue to grow.",
            era_tagline: Some("Network participation is broadening as the chain grows."),
            early_participation_signal: false,
        },
        _ => NetworkEraInfo {
            era_name: "Mature Network Era",
            era_description:
                "Irium has moved into a more mature operating phase with established network history and infrastructure.",
            era_tagline: Some("The network is operating in a more established phase."),
            early_participation_signal: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn returns_expected_eras_for_height_thresholds() {
        assert_eq!(network_era(0).era_name, "Early Miner Era");
        assert_eq!(network_era(25_000).era_name, "Early Miner Era");
        assert_eq!(network_era(25_001).era_name, "Growth Era");
        assert_eq!(network_era(100_000).era_name, "Growth Era");
        assert_eq!(network_era(100_001).era_name, "Mature Network Era");
    }

    #[test]
    fn serializes_era_fields_cleanly() {
        let era = network_era(12_345);
        let value = serde_json::to_value(era).expect("serialize era");
        assert_eq!(
            value.get("era_name"),
            Some(&Value::String("Early Miner Era".to_string()))
        );
        assert_eq!(
            value.get("early_participation_signal"),
            Some(&Value::Bool(true))
        );
        assert!(value
            .get("era_description")
            .and_then(Value::as_str)
            .is_some());
    }
}
