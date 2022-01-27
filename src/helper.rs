use cosmwasm_std::{Decimal, Uint128};
use provwasm_std::MarkerAccess;

/// Global Variables
pub const ONE_HUNDRED: Uint128 = Uint128::new(100);
pub const SENDER_MARKER_PERMISSIONS: [MarkerAccess; 5] = [
    MarkerAccess::Burn,
    MarkerAccess::Deposit,
    MarkerAccess::Mint,
    MarkerAccess::Transfer,
    MarkerAccess::Withdraw,
];
pub const CONTRACT_MARKER_PERMISSIONS: [MarkerAccess; 7] = [
    MarkerAccess::Mint,
    MarkerAccess::Burn,
    MarkerAccess::Deposit,
    MarkerAccess::Withdraw,
    MarkerAccess::Delete,
    MarkerAccess::Admin,
    MarkerAccess::Transfer,
];

/// Global Functions
pub fn to_percent(dec: Decimal) -> String {
    (dec * ONE_HUNDRED).to_string()
}
