//! Outbound integrations (Audiobookshelf) and connect portal.

mod abs;
mod error;
mod factory;
mod hooks;
mod portal;
mod registry;
mod tickets;
mod traits;
mod types;

pub use abs::{AbsApiClient, AbsIntegration, AbsLibrary, AbsUser};
pub use error::{IntegrationError, Result};
pub use factory::from_config;
pub use hooks::emit_book_liberated;
pub use portal::{mint_for_external_user, portal_router, PortalState};
pub use registry::IntegrationRegistry;
pub use tickets::{
    generate_token, hash_token, identity_from_session, mint_claim_ticket, normalize_portal_base,
    redeem_ticket_to_session, session_for_identity, ticket_portal_url, MintedClaimTicket,
};
pub use traits::{Integration, IntegrationContext};
pub use types::{ExternalUser, IntegrationEvent, IntegrationHealth};
