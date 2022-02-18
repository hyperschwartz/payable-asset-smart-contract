use crate::core::error::ContractError;
use crate::core::state::{payable_meta_storage_read_v2, PayableScopeAttribute};
use cosmwasm_std::{Binary, Deps};
use provwasm_std::ProvenanceQuery;
use crate::query::query_payable_by_scope_id::{query_payable_attribute_by_scope_id, query_payable_binary_by_scope_id};

/// Finds the scope_id by looking up local storage for the payable uuid link, and funnels the result
/// into the query_payable_by_scope_id function, which pulls all the data from the actual scope's
/// attribute list.
pub fn query_payable_binary_by_uuid(
    deps: &Deps<ProvenanceQuery>,
    payable_uuid: impl Into<String>,
) -> Result<Binary, ContractError> {
    query_payable_binary_by_scope_id(deps, get_scope_id_for_payable_uuid(deps, &payable_uuid))
}

pub fn query_payable_attribute_by_uuid(
    deps: &Deps<ProvenanceQuery>,
    payable_uuid: impl Into<String>,
) -> Result<PayableScopeAttribute, ContractError> {
    query_payable_attribute_by_scope_id(deps, get_scope_id_for_payable_uuid(deps, &payable_uuid))
}

fn get_scope_id_for_payable_uuid(deps: &Deps<ProvenanceQuery>, payable_uuid: impl Into<String>) -> String {
    payable_meta_storage_read_v2(deps.storage)
        .load(payable_uuid.into().as_bytes())
    ?.scope_id
}

#[cfg(test)]
mod tests {
    use crate::contract::{execute, query};
    use crate::core::msg::QueryMsg;
    use crate::core::state::PayableMeta;
    use crate::testutil::test_utilities::{
        default_register_payable, get_duped_scope, test_instantiate, InstArgs, DEFAULT_INFO_NAME,
        DEFAULT_ONBOARDING_DENOM, DEFAULT_PAYABLE_DENOM, DEFAULT_PAYABLE_TOTAL,
        DEFAULT_PAYABLE_UUID, DEFAULT_SCOPE_ID,
    };
    use cosmwasm_std::testing::{mock_env, mock_info};
    use cosmwasm_std::{coin, from_binary};
    use provwasm_mocks::mock_dependencies;

    #[test]
    fn test_query_payable_by_uuid_after_register() {
        let mut deps = mock_dependencies(&[]);
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
        deps.querier
            .with_scope(get_duped_scope(DEFAULT_SCOPE_ID, DEFAULT_INFO_NAME));
        execute(
            deps.as_mut(),
            mock_env(),
            mock_info(
                DEFAULT_INFO_NAME,
                &[coin(100, DEFAULT_ONBOARDING_DENOM.to_string())],
            ),
            default_register_payable(),
        )
        .unwrap();
        let payable_binary = query(
            deps.as_ref(),
            mock_env(),
            QueryMsg::QueryPayable {
                payable_uuid: DEFAULT_PAYABLE_UUID.to_string(),
            },
        )
        .unwrap();
        let payable_meta = from_binary::<PayableMeta>(&payable_binary).unwrap();
        assert_eq!(
            DEFAULT_PAYABLE_UUID,
            payable_meta.payable_uuid.as_str(),
            "expected the default marker address to be returned"
        );
        assert_eq!(
            DEFAULT_SCOPE_ID,
            payable_meta.scope_id.as_str(),
            "expected the default scope id to be returned"
        );
        assert_eq!(
            DEFAULT_PAYABLE_DENOM, payable_meta.payable_denom,
            "expected the payable to expect payment in the onboarding denom"
        );
        assert_eq!(
            DEFAULT_PAYABLE_TOTAL,
            payable_meta.payable_total_owed.u128(),
            "expected the payable total owed to reflect the default value"
        );
        assert_eq!(DEFAULT_PAYABLE_TOTAL, payable_meta.payable_remaining_owed.u128(), "expected the payable remaining owed to reflect the default value because no payments have been made");
        assert_eq!(false, payable_meta.oracle_approved, "when initially created, the meta should show that the oracle has not yet approved the payable");
    }
}
