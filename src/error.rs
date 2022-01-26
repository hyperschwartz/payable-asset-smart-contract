use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized,

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

    #[error("No funds of type {valid_denom} were provided")]
    NoFundsProvided { valid_denom: String },
}
