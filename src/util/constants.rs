// Execution output attributes.  All should be prefixed with "payable_" to make them easy to
// discern when observed in the event stream

////////////////////////////////////////////
// Payable registration output attributes //
////////////////////////////////////////////

/// Value = Payable UUID (String)
pub const PAYABLE_REGISTERED_KEY: &str = "payable_registered";
/// Value = Scope ID Tied to the Payable (String)
pub const SCOPE_ID_KEY: &str = "payable_related_scope_id";
/// Value = Payable total owed input value (u128)
pub const TOTAL_OWED_KEY: &str = "payable_total_owed";
/// Value = Payable denom input value (String)
pub const REGISTERED_DENOM_KEY: &str = "payable_denom";
/// Value = Amount of funds kept in the contract address to redistribute to the oracle later (u128 + denom: ex "420/nhash")
pub const ORACLE_FUNDS_KEPT: &str = "payable_oracle_funds_kept";
/// Value = Amount of overage funds refunded to the sender (u128 + denom: ex "100/nhash")
pub const REFUND_AMOUNT_KEY: &str = "payable_refund_amount";

///////////////////////////////////////
// Oracle approved output attributes //
///////////////////////////////////////

/// Value = Payable UUID (String)
pub const ORACLE_APPROVED_KEY: &str = "payable_oracle_approved";

////////////////////////////////////
// Payment made output attributes //
////////////////////////////////////

/// Value = Payable UUID (String)
pub const PAYMENT_MADE_KEY: &str = "payable_payment_made";
/// Value = Amount of payment input value (Long)
pub const PAYMENT_AMOUNT_KEY: &str = "payable_amount_paid";
/// Value = Amount remaining owed after payment (Long)
pub const TOTAL_REMAINING_KEY: &str = "payable_total_remaining";
/// Value = Bech32 address of the entity that made the payment (String)
pub const PAYER_KEY: &str = "payable_payer";
/// Value = Bech32 address of th entity that received the payment (String)
pub const PAYEE_KEY: &str = "payable_payee";

/////////////////////////////////
// Migration output attributes //
/////////////////////////////////

/// Value = Name of the field in the state being altered by the migration (String)
pub const MIGRATION_STATE_CHANGE_PREFIX: &str = "payable_migration_state_field_";
/// Value = Name of the contract being migrated to, which should never change (String)
pub const MIGRATION_CONTRACT_NAME: &str = "payable_migration_contract_name";
/// Value = New contract version that has been migrated to (String)
pub const MIGRATION_CONTRACT_VERSION: &str = "payable_migration_contract_version";

//////////////////////////////
// Shared output attributes //
//////////////////////////////

/// Value = Payable UUID (String)
pub const PAYABLE_UUID_KEY: &str = "payable_uuid";
/// Value = Payable Type (String)
pub const PAYABLE_TYPE_KEY: &str = "payable_type";
/// Value = The address of the oracle associated with the payable (String)
pub const ORACLE_ADDRESS_KEY: &str = "payable_oracle_address";
