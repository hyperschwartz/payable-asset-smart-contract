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

#[cfg(test)]
mod tests {
    use cosmwasm_std::from_binary;
    use cosmwasm_std::testing::mock_env;
    use provwasm_mocks::mock_dependencies;
    use crate::contract::query;
    use crate::core::msg::QueryMsg;
    use crate::core::state::PayableScopeAttribute;
    use crate::query::query_payable_by_scope_id::query_payable_attribute_by_scope_id;
    use crate::testutil::register_payable_helpers::{test_register_payable, TestRegisterPayable};
    use crate::testutil::test_utilities::{DEFAULT_ORACLE_ADDRESS, DEFAULT_PAYABLE_DENOM, DEFAULT_PAYABLE_TOTAL, DEFAULT_PAYABLE_TYPE, DEFAULT_PAYABLE_UUID, DEFAULT_SCOPE_ID, InstArgs, setup_test_suite};

    #[test]
    fn test_query_payable_by_scope_id_after_register() {
        let mut deps = mock_dependencies(&[]);
        let provenance_util = setup_test_suite(&mut deps, InstArgs::default());
        test_register_payable(&mut deps, &provenance_util, TestRegisterPayable::default()).unwrap();
        let payable_binary = query(
            deps.as_ref(),
            mock_env(),
            QueryMsg::QueryPayableByScopeId {
                scope_id: DEFAULT_SCOPE_ID.to_string(),
            }
        ).unwrap();
        let scope_attribute = from_binary::<PayableScopeAttribute>(&payable_binary).unwrap();
        assert_eq!(
            DEFAULT_PAYABLE_TYPE,
            scope_attribute.payable_type.as_str(),
            "expected the default payable type to be returned",
        );
        assert_eq!(
            DEFAULT_PAYABLE_UUID,
            scope_attribute.payable_uuid.as_str(),
            "expected the default marker address to be returned"
        );
        assert_eq!(
            DEFAULT_SCOPE_ID,
            scope_attribute.scope_id.as_str(),
            "expected the default scope id to be returned"
        );
        assert_eq!(
            DEFAULT_ORACLE_ADDRESS,
            scope_attribute.oracle_address.as_str(),
            "expected the default oracle address to be returned",
        );
        assert_eq!(
            DEFAULT_PAYABLE_DENOM, scope_attribute.payable_denom,
            "expected the payable to expect payment in the onboarding denom"
        );
        assert_eq!(
            DEFAULT_PAYABLE_TOTAL,
            scope_attribute.payable_total_owed.u128(),
            "expected the payable total owed to reflect the default value"
        );
        assert_eq!(DEFAULT_PAYABLE_TOTAL, scope_attribute.payable_remaining_owed.u128(), "expected the payable remaining owed to reflect the default value because no payments have been made");
        assert_eq!(false, scope_attribute.oracle_approved, "when initially created, the meta should show that the oracle has not yet approved the payable");
    }

    #[test]
    fn test_query_payable_attribute_by_scope_id() {
        let mut deps = mock_dependencies(&[]);
        let provenance_util = setup_test_suite(&mut deps, InstArgs::default());
        test_register_payable(&mut deps, &provenance_util, TestRegisterPayable::default()).unwrap();
        let scope_attr = query_payable_attribute_by_scope_id(&deps.as_ref(), DEFAULT_SCOPE_ID)
            .expect("the default payable should deserialize correctly");
        provenance_util.assert_attribute_matches_latest(&scope_attr);
    }
}
