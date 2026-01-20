use flagsmith_flag_engine::engine_eval::FlagResult;

pub fn filter_out_server_key_only_flag_results(
    flag_results: Vec<FlagResult>,
    server_key_only_feature_ids: &[u32],
) -> Vec<FlagResult> {
    flag_results
        .into_iter()
        .filter(|fr| !server_key_only_feature_ids.contains(&fr.metadata.feature_id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use flagsmith_flag_engine::engine_eval::FeatureMetadata;
    use flagsmith_flag_engine::types::{FlagsmithValue, FlagsmithValueType};

    fn create_flag_result(id: u32, name: &str) -> FlagResult {
        FlagResult {
            enabled: true,
            name: name.to_string(),
            value: FlagsmithValue {
                value_type: FlagsmithValueType::String,
                value: format!("value{}", id),
            },
            reason: "DEFAULT".to_string(),
            metadata: FeatureMetadata {
                feature_id: id,
                feature_type: "STANDARD".to_string(),
            },
        }
    }

    #[test]
    fn test_filter_out_server_key_only_features() {
        // Given
        let flag_results = vec![
            create_flag_result(1, "feature_1"),
            create_flag_result(2, "feature_2"),
            create_flag_result(3, "feature_3"),
        ];

        let server_key_only_ids = vec![3];

        // When
        let result = filter_out_server_key_only_flag_results(flag_results, &server_key_only_ids);

        // Then
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].metadata.feature_id, 1);
        assert_eq!(result[1].metadata.feature_id, 2);
    }
}
