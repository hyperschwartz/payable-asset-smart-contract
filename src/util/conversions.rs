use crate::core::error::ContractError;
use crate::util::constants::ONE_HUNDRED;
use cosmwasm_std::{Decimal, Uint128};

pub fn to_percent(dec: Decimal) -> String {
    (dec * ONE_HUNDRED).to_string()
}

pub fn to_uint128(string: impl Into<String>) -> Result<Uint128, ContractError> {
    match string.into().parse::<u128>() {
        Ok(int) => Ok(Uint128::new(int)),
        Err(e) => Err(ContractError::ParseInt(e)),
    }
}
