use cosmwasm_std::{Decimal, Uint128};
use provwasm_std::MarkerAccess;

/// Global Variables
pub const ONE_HUNDRED: Uint128 = Uint128::new(100);
pub const DEFAULT_MARKER_COIN_AMOUNT: u128 = 1000;
pub const CONTRACT_MARKER_PERMISSIONS: [MarkerAccess; 7] = [
    MarkerAccess::Admin,
    MarkerAccess::Burn,
    MarkerAccess::Deposit,
    MarkerAccess::Delete,
    MarkerAccess::Mint,
    MarkerAccess::Transfer,
    MarkerAccess::Withdraw,
];

/// Global Functions
pub fn to_percent(dec: Decimal) -> String {
    (dec * ONE_HUNDRED).to_string()
}
