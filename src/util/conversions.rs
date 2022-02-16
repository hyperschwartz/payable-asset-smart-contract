use crate::util::constants::ONE_HUNDRED;
use cosmwasm_std::Decimal;

pub fn to_percent(dec: Decimal) -> String {
    (dec * ONE_HUNDRED).to_string()
}
