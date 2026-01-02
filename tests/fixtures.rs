use serde_json::json;

pub fn environment_1_api_key() -> String {
    "environment_1_api_key".to_string()
}

pub fn environment_1() -> serde_json::Value {
    json!({
        "updated_at": "1969-07-20T20:17:40Z",
        "feature_states": [
            {
                "multivariate_feature_state_values": [],
                "feature_state_value": "feature_1_value",
                "feature": {
                    "name": "feature_1",
                    "type": "STANDARD",
                    "id": 1,
                },
                "enabled": false,
                "featurestate_uuid": "uuid-1",
            },
            {
                "multivariate_feature_state_values": [],
                "feature_state_value": "2.3",
                "feature": {
                    "name": "feature_2",
                    "type": "STANDARD",
                    "id": 2,
                },
                "enabled": true,
                "featurestate_uuid": "uuid-2",
            },
            {
                "multivariate_feature_state_values": [],
                "feature_state_value": null,
                "feature": {
                    "name": "feature_3",
                    "type": "STANDARD",
                    "id": 3,
                },
                "enabled": false,
                "featurestate_uuid": "uuid-3",
            }
        ],
        "identity_overrides": [
            {
                "identifier": "overridden-id",
                "identity_uuid": "0f21cde8-63c5-4e50-baca-87897fa6cd01",
                "created_date": "2019-08-27T14:53:45.698555Z",
                "environment_api_key": environment_1_api_key(),
                "identity_features": [
                    {
                        "django_id": 1,
                        "feature": {"id": 1, "name": "feature_1", "type": "STANDARD"},
                        "featurestate_uuid": "1bddb9a5-7e59-42c6-9be9-625fa369749f",
                        "feature_state_value": "identity_override",
                        "enabled": true,
                    }
                ],
                "identity_traits": []
            }
        ],
        "api_key": environment_1_api_key(),
        "project": {
            "name": "project-1",
            "organisation": {
                "feature_analytics": false,
                "name": "org-1",
                "id": 1,
                "persist_trait_data": true,
                "stop_serving_flags": false,
            },
            "id": 1,
            "hide_disabled_flags": false,
            "segments": [
                {
                    "name": "segment_1",
                    "rules": [
                        {
                            "conditions": [],
                            "type": "ALL",
                            "rules": [
                                {
                                    "conditions": [
                                        {
                                            "value": "test",
                                            "operator": "EQUAL",
                                            "property": "first_name",
                                        }
                                    ],
                                    "type": "ANY",
                                    "rules": [],
                                }
                            ],
                        }
                    ],
                    "id": 1,
                    "feature_states": [
                        {
                            "multivariate_feature_state_values": [],
                            "feature_state_value": "segment_override",
                            "feature": {
                                "name": "feature_2",
                                "type": "STANDARD",
                                "id": 2,
                            },
                            "enabled": true,
                            "featurestate_uuid": "uuid-segment-2",
                        }
                    ],
                }
            ],
            "server_key_only_feature_ids": [3],
        },
        "id": 1,
    })
}

pub fn expected_flags_response() -> serde_json::Value {
    json!([
        {
            "feature_state_value": "feature_1_value",
            "feature": {"name": "feature_1", "type": "STANDARD", "id": 1},
            "enabled": false,
        },
        {
            "feature_state_value": "2.3",
            "feature": {
                "name": "feature_2",
                "type": "STANDARD",
                "id": 2,
            },
            "enabled": true,
        },
    ])
}

pub fn expected_flags_response_with_segment_override() -> serde_json::Value {
    json!([
        {
            "feature_state_value": "feature_1_value",
            "feature": {"name": "feature_1", "type": "STANDARD", "id": 1},
            "enabled": false,
        },
        {
            "feature_state_value": "segment_override",
            "feature": {
                "name": "feature_2",
                "type": "STANDARD",
                "id": 2,
            },
            "enabled": true,
        },
    ])
}

pub fn expected_flags_response_with_identity_override() -> serde_json::Value {
    json!([
        {
            "feature_state_value": "identity_override",
            "feature": {"name": "feature_1", "type": "STANDARD", "id": 1},
            "enabled": true,
        },
        {
            "feature_state_value": "2.3",
            "feature": {
                "name": "feature_2",
                "type": "STANDARD",
                "id": 2,
            },
            "enabled": true,
        },
    ])
}
