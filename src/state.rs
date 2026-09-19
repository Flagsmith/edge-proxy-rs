use crate::services::{EnvironmentService, UsageProcessor};
use axum::extract::FromRef;
use std::sync::Arc;

#[derive(Clone, FromRef)]
pub struct AppState {
    pub environments: Arc<EnvironmentService>,
    pub usage: Arc<UsageProcessor>,
}
