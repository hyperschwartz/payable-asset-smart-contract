use cosmwasm_std::{Decimal, Uint128};

const ONE_HUNDRED: Uint128 = Uint128::new(100);

pub fn to_percent(dec: Decimal) -> String {
    (dec * ONE_HUNDRED).to_string()
}
