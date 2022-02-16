use cosmwasm_std::{Decimal, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use crate::core::error::ContractError;

use crate::core::state::State;
use crate::util::traits::ValidatedMsg;

/// A message sent to initialize the contract state.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InitMsg {
    // The type of payable that this contract handles. All incoming registration requests will validate that the source is this type.
    pub payable_type: String,
    // Name of the contract that is tagged on various things
    pub contract_name: String,
    // Cost to onboard each payable
    pub onboarding_cost: String,
    // Coin type for onboarding charge
    pub onboarding_denom: String,
    // The address that will collect onboarding fees
    pub fee_collection_address: String,
    // Percentage of each transaction that is taken as fee
    pub fee_percent: Decimal,
    // Address of the oracle application that can withdraw excess fees after fee percent is removed from onboarding_cost
    pub oracle_address: String,
    // Whether or not this contract should have assistance for local environments
    pub is_local: Option<bool>,
}
impl ValidatedMsg for InitMsg {
    fn validate(&self) -> Result<(), ContractError> {
        let mut invalid_fields: Vec<&str> = vec![];
        if self.payable_type.is_empty() {
            invalid_fields.push("payable_type");
        }
        if self.contract_name.is_empty() {
            invalid_fields.push("contract_name");
        }
        if let Err(_) = self.onboarding_cost.parse::<u128>() {
            invalid_fields.push("onboarding_cost");
        }
        if self.onboarding_denom.is_empty() {
            invalid_fields.push("onboarding_denom");
        }
        if self.fee_collection_address.is_empty() {
            invalid_fields.push("fee_collection_address");
        }
        if self.fee_percent > Decimal::one() {
            invalid_fields.push("fee_percent");
        }
        if self.oracle_address.is_empty() {
            invalid_fields.push("oracle_address");
        }
        if !invalid_fields.is_empty() {
            ContractError::invalid_fields(invalid_fields).to_result()
        } else {
            Ok(())
        }
    }
}

/// A message sent to register a name with the name service
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    RegisterPayable {
        payable_type: String,
        payable_uuid: String,
        scope_id: String,
        payable_denom: String,
        payable_total: Uint128,
    },
    OracleApproval {
        payable_uuid: String,
    },
    MakePayment {
        payable_uuid: String,
    },
}
impl ValidatedMsg for ExecuteMsg {
    fn validate(&self) -> Result<(), ContractError> {
        let mut invalid_fields: Vec<&str> = vec![];
        match self {
            ExecuteMsg::RegisterPayable { payable_type, payable_uuid, scope_id, payable_denom, payable_total } => {
                if payable_type.is_empty() {
                    invalid_fields.push("payable_type");
                }
                if payable_uuid.is_empty() {
                    invalid_fields.push("payable_uuid");
                }
                if scope_id.is_empty() {
                    invalid_fields.push("scope_id");
                }
                if payable_denom.is_empty() {
                    invalid_fields.push("payable_denom");
                }
                if payable_total.u128() == 0 {
                    invalid_fields.push("payable_total");
                }
            },
            ExecuteMsg::OracleApproval { payable_uuid } => {
                if payable_uuid.is_empty() {
                    invalid_fields.push("payable_uuid");
                }
            },
            ExecuteMsg::MakePayment { payable_uuid } => {
                if payable_uuid.is_empty() {
                    invalid_fields.push("payable_uuid");
                }
            },
        };
        if !invalid_fields.is_empty() {
            ContractError::invalid_fields(invalid_fields).to_result()
        } else {
            Ok(())
        }
    }
}

/// A message sent to query contract config state.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    QueryState {},
    QueryPayable { payable_uuid: String },
}
impl ValidatedMsg for QueryMsg {
    fn validate(&self) -> Result<(), ContractError> {
        let mut invalid_fields: Vec<&str> = vec![];
        match self {
            QueryMsg::QueryState {} => (),
            QueryMsg::QueryPayable { payable_uuid } => {
                if payable_uuid.is_empty() {
                    invalid_fields.push("query_payable");
                }
            }
        };
        if !invalid_fields.is_empty() {
            ContractError::invalid_fields(invalid_fields).to_result()
        } else {
            Ok(())
        }
    }
}

/// A type alias for contract state.
pub type QueryResponse = State;

/// Migrate the contract
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct MigrateMsg {
}
impl ValidatedMsg for MigrateMsg {
    fn validate(&self) -> Result<(), ContractError> {
        Ok(())
    }
}
