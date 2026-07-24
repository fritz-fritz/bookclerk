//! Connect portal HTTP nest (claim ticket + integration credential login).

mod brands;
mod html;
mod routes;

pub use routes::{mint_for_external_user, portal_router, PortalState};
