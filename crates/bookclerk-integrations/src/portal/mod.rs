//! Connect portal HTTP nest (claim ticket + integration credential login).

mod brands;
mod html;
mod routes;

#[cfg(test)]
mod enabled_tests;

pub use routes::{
    mint_for_external_user, portal_identity_from_headers, portal_router, portal_spa_router,
    PortalState,
};
