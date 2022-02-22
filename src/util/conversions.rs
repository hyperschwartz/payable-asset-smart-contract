use crate::core::error::ContractError;
use cosmwasm_std::Uint128;

/// Converts the derived String into a Uint128, or returns a ContractError if a parsing failure
/// occurs.
pub fn to_uint128(string: impl Into<String>) -> Result<Uint128, ContractError> {
    match string.into().parse::<u128>() {
        Ok(int) => Ok(Uint128::new(int)),
        Err(e) => Err(ContractError::ParseInt(e)),
    }
}
