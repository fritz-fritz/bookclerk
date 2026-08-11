//! `encrypted_secrets` table entity.
//!
//! Retained in the schema and entities per the multi-storefront design; the
//! encryption / secrets-file removal work lives in [`crate::secrets`].

use sea_orm::entity::prelude::*;

/// Model.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "encrypted_secrets")]
pub struct Model {
    /// Identifier.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Kind.
    pub kind: String,
    /// Provider.
    pub provider: Option<String>,
    /// Ownership namespace: [`crate::secrets::secret_account_type`].
    pub account_type: String,
    /// Account Identifier.
    pub account_id: Option<String>,
    /// Name.
    pub name: String,
    /// Format.
    pub format: String,
    /// Ciphertext.
    pub ciphertext: Vec<u8>,
    /// Kdf algorithm.
    pub kdf_algorithm: Option<String>,
    /// Kdf salt.
    pub kdf_salt: Option<Vec<u8>>,
    /// Kdf m cost.
    pub kdf_m_cost: Option<i64>,
    /// Kdf t cost.
    pub kdf_t_cost: Option<i64>,
    /// Kdf p cost.
    pub kdf_p_cost: Option<i64>,
    /// Cipher algorithm.
    pub cipher_algorithm: Option<String>,
    /// Cipher nonce.
    pub cipher_nonce: Option<Vec<u8>>,
    /// Created at.
    pub created_at: String,
    /// Updated at.
    pub updated_at: String,
}

/// Relation.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
