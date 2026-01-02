// Wrapper types for flagsmith-flag-engine to add missing fields

use flagsmith_flag_engine::environments::Environment as FlagsmithEnvironment;
use flagsmith_flag_engine::organisations::Organisation;
use flagsmith_flag_engine::projects::Project as FlagsmithProject;
use flagsmith_flag_engine::segments::Segment;
use flagsmith_flag_engine::features::FeatureState;
use flagsmith_flag_engine::identities::Identity;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Project {
    pub id: u32,
    pub name: String,
    pub organisation: Organisation,
    pub hide_disabled_flags: bool,
    pub segments: Vec<Segment>,

    #[serde(default)]
    pub server_key_only_feature_ids: Vec<u32>,
}

impl From<Project> for FlagsmithProject {
    fn from(project: Project) -> Self {
        FlagsmithProject {
            id: project.id,
            name: project.name,
            organisation: project.organisation,
            hide_disabled_flags: project.hide_disabled_flags,
            segments: project.segments,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Environment {
    pub id: u32,
    pub api_key: String,
    #[serde(default)]
    pub name: String,
    pub project: Project,
    pub feature_states: Vec<FeatureState>,

    #[serde(default)]
    pub identity_overrides: Vec<Identity>,
}

impl Environment {
    pub fn to_flagsmith_environment(&self) -> FlagsmithEnvironment {
        FlagsmithEnvironment {
            id: self.id,
            api_key: self.api_key.clone(),
            name: self.name.clone(),
            project: self.project.clone().into(),
            feature_states: self.feature_states.clone(),
            identity_overrides: self.identity_overrides.clone(),
        }
    }
}
