use cosmwasm_std::{Decimal, Uint128};

/// Global Variables
pub const ONE_HUNDRED: Uint128 = Uint128::new(100);
pub const PAYABLE_REGISTERED_KEY: &str = "PAYABLE_REGISTERED";
pub const ORACLE_APPROVED_KEY: &str = "ORACLE_APPROVED";
pub const PAYMENT_MADE_KEY: &str = "PAYMENT_MADE";

/// Global Functions
pub fn to_percent(dec: Decimal) -> String {
    (dec * ONE_HUNDRED).to_string()
}
