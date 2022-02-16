use crate::core::error::ContractError;

/// Defines a self-validating contract message. The response should create a ContractError if any
/// provided fields are improperly-formatted.
pub trait ValidatedMsg {
    fn validate(&self) -> Result<(), ContractError>;
}
