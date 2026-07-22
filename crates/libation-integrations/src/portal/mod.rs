//! Connect portal HTTP nest (claim ticket + integration credential login).

mod html;
mod routes;

pub use routes::{mint_for_external_user, portal_router, PortalState};
