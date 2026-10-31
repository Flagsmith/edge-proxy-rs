use crate::models::{IdentityWithTraits, TraitModel};
use async_trait::async_trait;

#[async_trait]
pub trait IdentityStore: Send + Sync {
    async fn load_traits(
        &self,
        environment: &serde_json::Value,
        identity: &IdentityWithTraits,
    ) -> IdentityTraits;

    fn save_traits(
        &self,
        environment: &serde_json::Value,
        identity: &IdentityWithTraits,
        key_type: EnvironmentKeyType,
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityTraits {
    pub traits: Vec<TraitModel>,
    pub system_traits: Vec<TraitModel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvironmentKeyType {
    Client,
    Server,
}
