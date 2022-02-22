use crate::core::error::ContractError;
use crate::core::state::{config_read_v2, PayableScopeAttribute};
use cosmwasm_std::{to_binary, Addr, Binary, Deps};
use provwasm_std::{ProvenanceQuerier, ProvenanceQuery};

/// Finds the PayableScopeAttribute tagged to a scope's address (scope_id - as it's referred to in
/// many places in the documentation, which is a bech32 address prefixed with "scope").
pub fn query_payable_binary_by_scope_id(
    deps: &Deps<ProvenanceQuery>,
    scope_id: impl Into<String>,
) -> Result<Binary, ContractError> {
    let attribute_result = query_payable_attribute_by_scope_id(deps, scope_id);
    if let Ok(attr) = attribute_result {
        Ok(to_binary(&attr)?)
    } else {
        Err(attribute_result.expect_err("result should be error"))
    }
}

pub fn query_payable_attribute_by_scope_id(
    deps: &Deps<ProvenanceQuery>,
    scope_id: impl Into<String>,
) -> Result<PayableScopeAttribute, ContractError> {
    let state = config_read_v2(deps.storage).load()?;
    let scope_id = scope_id.into();
    let attributes = ProvenanceQuerier::new(&deps.querier)
        .get_json_attributes::<Addr, String, PayableScopeAttribute>(
            Addr::unchecked(&scope_id),
            state.contract_name,
        )?;
    // Only one scope attribute should ever be tagged on a scope.  If there are > 1, then a bug has
    // occurred, and if there are zero, then the scope being queried has never been registered with
    // the contract (or an even more terrible bug has occurred).
    if attributes.len() != 1 {
        return ContractError::InvalidScopeAttribute {
            scope_id,
            attribute_amount: attributes.len(),
        }
        .to_result();
    }
    Ok(attributes.first().unwrap().to_owned())
}
