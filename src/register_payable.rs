use cosmwasm_std::Uint128;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct RegisterPayableMarkerV1 {
    pub payable_uuid: String,
    pub scope_id: String,
    pub payable_denom: String,
    pub payable_total: Uint128,
}
