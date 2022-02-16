use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized,

    #[error("Payable with uuid {payable_uuid} has already been approved")]
    DuplicateApproval { payable_uuid: String },

    #[error("Funds were provided for an operation that does not require them")]
    FundsPresent,

    #[error("Insufficient funds provided. Required {amount_needed} but got {amount_provided}")]
    InsufficientFundsProvided {
        amount_needed: u128,
        amount_provided: u128,
    },

    // Add any other custom errors you like here.
    // Look at https://docs.rs/thiserror/1.0.21/thiserror/ for details.
    #[error("Invalid fields: {fields:?}")]
    InvalidFields { fields: Vec<String> },

    #[error("Invalid fund types provided: {invalid_denoms:?}. Please provide coin of type {valid_denom}")]
    InvalidFundsProvided {
        valid_denom: String,
        invalid_denoms: Vec<String>,
    },

    #[error("Payable {payable_uuid} was invalid: {invalid_reason}")]
    InvalidPayable {
        payable_uuid: String,
        invalid_reason: String,
    },

    #[error("No funds of type {valid_denom} were provided")]
    NoFundsProvided { valid_denom: String },

    #[error("Target payable with uuid [{payable_uuid}] is not ready for payment due to: {not_ready_reason}")]
    NotReadyForPayment {
        payable_uuid: String,
        not_ready_reason: String,
    },

    #[error("Unable to locate target payable {payable_uuid}")]
    PayableNotFound { payable_uuid: String },

    #[error("Payment too large. Total owed [{total_owed}], amount provided [{amount_provided}]")]
    PaymentTooLarge {
        total_owed: u128,
        amount_provided: u128,
    },
}
impl ContractError {
    /// Allows ContractError instances to be generically returned as a Response in a fluent manner
    /// instead of wrapping in an Err() call, improving readability.
    /// Ex: return ContractError::NameNotFound.to_result();
    /// vs
    ///     return Err(ContractError::NameNotFound);
    pub fn to_result<T>(self) -> Result<T, ContractError> {
        Err(self)
    }
    /// A simple abstraction to wrap an error response just by passing the message
    pub fn std_err<T>(msg: impl Into<String>) -> Result<T, ContractError> {
        Err(ContractError::Std(StdError::generic_err(msg)))
    }
}
