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
        origin: RequestOrigin,
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityTraits {
    pub evaluation: Vec<TraitModel>,
    pub response: Vec<TraitModel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestOrigin {
    Client,
    Server,
}
