use cosmwasm_std::{QuerierWrapper, StdResult};
use provwasm_std::{ProvenanceQuerier, ProvenanceQuery, Scope};

pub fn get_scope_by_id(
    querier: &QuerierWrapper<ProvenanceQuery>,
    scope_id: &str,
) -> StdResult<Scope> {
    ProvenanceQuerier::new(querier).get_scope(scope_id)
}
