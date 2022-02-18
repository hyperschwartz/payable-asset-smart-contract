use cosmwasm_std::Uint128;

/// Global Variables
pub const ONE_HUNDRED: Uint128 = Uint128::new(100);

/// Execution output attributes.  All should be prefixed with "payable_" to make them easy to
/// discern when observed in the event stream

/// Payable registration output attributes
pub const PAYABLE_REGISTERED_KEY: &str = "payable_registered"; // Value = Emtpy String
pub const SCOPE_ID_KEY: &str = "payable_related_scope_id"; // Value = Scope ID Tied to the Payable (String)
pub const TOTAL_OWED_KEY: &str = "payable_total_owed"; // Value = Payable total owed input value (u128)
pub const REGISTERED_DENOM_KEY: &str = "payable_denom"; // Value = Payable denom input value (String)
pub const ORACLE_FUNDS_KEPT: &str = "payable_oracle_funds_kept"; // Value = Amount of funds kept in the contract address to redistribute to the oracle later (u128 + denom: ex "420/nhash")
pub const REFUND_AMOUNT_KEY: &str = "payable_refund_amount"; // Value = Amount of overage funds refunded to the sender (u128 + denom: ex "100/nhash")

/// Oracle approved output attributes
pub const ORACLE_APPROVED_KEY: &str = "payable_oracle_approved"; // Value = Empty String

/// Payment made output attributes
pub const PAYMENT_MADE_KEY: &str = "payable_payment_made"; // Value = Empty String
pub const PAYMENT_AMOUNT_KEY: &str = "payable_amount_paid"; // Value = Amount of payment input value (Long)
pub const TOTAL_REMAINING_KEY: &str = "payable_total_remaining"; // Value = Amount remaining owed after payment (Long)
pub const PAYER_KEY: &str = "payable_payer"; // Value = Bech32 address of the entity that made the payment (String)
pub const PAYEE_KEY: &str = "payable_payee"; // Value = Bech32 address of th entity that received the payment (String)

/// Migration output attributes
pub const MIGRATION_STATE_CHANGE_PREFIX: &str = "payable_migration_state_field_"; // Value = Name of the field in the state being altered by the migration (String)
pub const MIGRATION_CONTRACT_NAME: &str = "payable_migration_contract_name"; // Value = Name of the contract being migrated to, which should never change (String)
pub const MIGRATION_CONTRACT_VERSION: &str = "payable_migration_contract_version"; // Value = New contract version that has been migrated to (String)

/// Shared output attributes
pub const PAYABLE_UUID_KEY: &str = "payable_uuid"; // Value = Payable UUID (String)
pub const PAYABLE_TYPE_KEY: &str = "payable_type"; // Value = Payable Type (String)
