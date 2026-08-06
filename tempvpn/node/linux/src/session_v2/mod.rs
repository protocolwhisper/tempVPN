pub mod chain;
pub mod method;
pub mod protocol;
pub mod store;
pub mod stream;

use std::sync::Arc;

use mpp::server::{Mpp, TempoChargeMethod, TempoProvider};

use self::{method::TempoSessionV2Method, store::SessionStore};

pub type StreamingMpp = Mpp<TempoChargeMethod<TempoProvider>, TempoSessionV2Method>;

#[derive(Clone)]
pub struct StreamingPayments {
    pub mpp: StreamingMpp,
    pub store: Arc<SessionStore>,
}
