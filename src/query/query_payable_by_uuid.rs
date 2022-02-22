use crate::core::error::ContractError;
use crate::core::state::{payable_meta_storage_read_v2, PayableScopeAttribute};
use crate::query::query_payable_by_scope_id::{
    query_payable_attribute_by_scope_id, query_payable_binary_by_scope_id,
};
use cosmwasm_std::{Binary, Deps};
use provwasm_std::ProvenanceQuery;

/// Finds the scope_id by looking up local storage for the payable uuid link, and funnels the result
/// into the query_payable_by_scope_id function, which pulls all the data from the actual scope's
/// attribute list.
pub fn query_payable_binary_by_uuid(
    deps: &Deps<ProvenanceQuery>,
    payable_uuid: impl Into<String>,
) -> Result<Binary, ContractError> {
    query_payable_binary_by_scope_id(deps, get_scope_id_for_payable_uuid(deps, payable_uuid)?)
}

/// Finds the scope id by querying local storage for the payable uuid to scope id link, and then
/// forks the functionality into the query by scope id functionality to derive the resulting
/// deserialized PayableScopeAttribute.
pub fn query_payable_attribute_by_uuid(
    deps: &Deps<ProvenanceQuery>,
    payable_uuid: impl Into<String>,
) -> Result<PayableScopeAttribute, ContractError> {
    query_payable_attribute_by_scope_id(deps, get_scope_id_for_payable_uuid(deps, payable_uuid)?)
}

/// Queries local storage for the contained payable uuid to scope id link and returns the resulting
/// scope id, if present.  Otherwise, defaults to a Std contract error indicating the issue in
/// storage load.
fn get_scope_id_for_payable_uuid(
    deps: &Deps<ProvenanceQuery>,
    payable_uuid: impl Into<String>,
) -> Result<String, ContractError> {
    Ok(payable_meta_storage_read_v2(deps.storage)
        .load(payable_uuid.into().as_bytes())?
        .scope_id)
}

#[cfg(test)]
mod tests {
    use crate::contract::query;
    use crate::core::msg::QueryMsg;
    use crate::core::state::PayableScopeAttribute;
    use crate::query::query_payable_by_uuid::query_payable_attribute_by_uuid;
    use crate::testutil::register_payable_helpers::{test_register_payable, TestRegisterPayable};
    use crate::testutil::test_utilities::{
        setup_test_suite, InstArgs, DEFAULT_ORACLE_ADDRESS, DEFAULT_PAYABLE_DENOM,
        DEFAULT_PAYABLE_TOTAL, DEFAULT_PAYABLE_TYPE, DEFAULT_PAYABLE_UUID, DEFAULT_SCOPE_ID,
    };
    use cosmwasm_std::from_binary;
    use cosmwasm_std::testing::mock_env;
    use provwasm_mocks::mock_dependencies;

    #[test]
    fn test_query_payable_by_uuid_after_register() {
        let mut deps = mock_dependencies(&[]);
        let provenance_util = setup_test_suite(&mut deps, InstArgs::default());
        test_register_payable(&mut deps, &provenance_util, TestRegisterPayable::default()).unwrap();
        let payable_binary = query(
            deps.as_ref(),
            mock_env(),
            QueryMsg::QueryPayableByUuid {
                payable_uuid: DEFAULT_PAYABLE_UUID.to_string(),
            },
        )
        .unwrap();
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
    fn test_query_payable_attribute_by_uuid() {
        let mut deps = mock_dependencies(&[]);
        let provenance_util = setup_test_suite(&mut deps, InstArgs::default());
        test_register_payable(&mut deps, &provenance_util, TestRegisterPayable::default()).unwrap();
        let scope_attr = query_payable_attribute_by_uuid(&deps.as_ref(), DEFAULT_PAYABLE_UUID)
            .expect("the default payable should deserialize correctly");
        provenance_util.assert_attribute_matches_latest(&scope_attr);
    }
}
