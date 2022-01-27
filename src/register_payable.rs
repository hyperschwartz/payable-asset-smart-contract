use cosmwasm_std::{Addr, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct RegisterPayableMarkerV1 {
    pub marker_address: Addr,
    pub marker_denom: String,
    pub scope_id: String,
    pub payable_denom: String,
    pub payable_total: Uint128,
}
