use cosmwasm_std::{CosmosMsg, CustomMsg, Decimal, Uint128};
use provwasm_std::{ProvenanceMsg, ProvenanceMsgParams, ProvenanceRoute};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::state::State;

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

/// A message sent to query contract config state.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    QueryState {},
    QueryPayable { payable_uuid: String },
}

/// A type alias for contract state.
pub type QueryResponse = State;

/// Migrate the contract
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct MigrateMsg {}

/// FIXME: This will be fixed in 1.0.1-beta of provwasm.  This struct exists as a clone of
/// FIXME: ProvenanceMsg, but implements CustomMsg.  CosmWasm 1.0.0-betaX does not allow exported
/// FIXME: entry_point functions to expose Response<X> values, where X does not implement CustomMsg.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ProvenanceMsgV2 {
    pub route: ProvenanceRoute,
    pub params: ProvenanceMsgParams,
    pub version: String,
}
impl ProvenanceMsgV2 {
    pub fn from_cosmos(msg: CosmosMsg<ProvenanceMsg>) -> CosmosMsg<ProvenanceMsgV2> {
        match msg {
            CosmosMsg::Custom(provenance_msg) => {
                CosmosMsg::Custom(ProvenanceMsgV2 {
                    route: provenance_msg.route,
                    params: provenance_msg.params,
                    version: provenance_msg.version,
                })
            },
            _ => panic!("unexpected message type provided to converter"),
        }
    }

    pub fn from_prov(msg: ProvenanceMsg) -> ProvenanceMsgV2 {
        ProvenanceMsgV2 { route: msg.route, params: msg.params, version: msg.version, }
    }
}
/// Note: The linter does not like this, but it isn't seeing the derived JsonSchema, so it compiles
/// fine.
impl CustomMsg for ProvenanceMsgV2 {}
