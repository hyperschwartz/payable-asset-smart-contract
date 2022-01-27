use cosmwasm_std::{Decimal, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::state::State;

/// A message sent to initialize the contract state.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InitMsg {
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
}

/// A message sent to register a name with the name service
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    RegisterPayableMarker {
        marker_address: String,
        marker_denom: String,
        scope_id: String,
        payable_denom: String,
        payable_total: Uint128,
    },
}

/// A message sent to query contract config state.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    QueryState {},
    QueryPayable { marker_denom: String },
}

/// A type alias for contract state.
pub type QueryResponse = State;

/// Migrate the contract
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct MigrateMsg {}
