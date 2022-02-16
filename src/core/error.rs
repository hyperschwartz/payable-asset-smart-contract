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

    #[error("Current contract name [{current_contract}] does not match provided migration name [{migration_contract}]")]
    InvalidContractName {
        current_contract: String,
        migration_contract: String,
    },

    #[error("Current contract version [{current_version}] is higher than provided migration version [{migration_version}]")]
    InvalidContractVersion {
        current_version: String,
        migration_version: String,
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

    #[error("Semver parsing error: {0}")]
    SemVer(String),
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
    /// Helper to map a Vec<&str> into an InvalidFields enum
    pub fn invalid_fields(fields: Vec<&str>) -> ContractError {
        ContractError::InvalidFields {
            fields: fields
                .into_iter()
                .map(|element| element.to_string())
                .collect(),
        }
    }
}
impl From<semver::Error> for ContractError {
    /// Enables SemVer issues to cast convert implicitly to contract error
    fn from(err: semver::Error) -> Self {
        Self::SemVer(err.to_string())
    }
}
