use crate::core::error::ContractError;
use crate::core::state::config_read;
use cosmwasm_std::{to_binary, Binary, Deps};
use provwasm_std::ProvenanceQuery;

pub fn query_state(deps: Deps<ProvenanceQuery>) -> Result<Binary, ContractError> {
    let state = config_read(deps.storage).load()?;
    Ok(to_binary(&state)?)
}

#[cfg(test)]
mod tests {
    use crate::contract::query;
    use crate::core::msg::{QueryMsg, QueryResponse};
    use crate::testutil::test_utilities::{
        test_instantiate, InstArgs, DEFAULT_CONTRACT_NAME, DEFAULT_FEE_COLLECTION_ADDRESS,
        DEFAULT_FEE_PERCENT, DEFAULT_ONBOARDING_DENOM, DEFAULT_ORACLE_ADDRESS,
    };
    use cosmwasm_std::testing::mock_env;
    use cosmwasm_std::{from_binary, Decimal, Uint128};
    use provwasm_mocks::mock_dependencies;

    #[test]
    fn test_query_state() {
        // Create mocks
        let mut deps = mock_dependencies(&[]);

        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();

        // Call the smart contract query function to get stored state.
        let bin = query(deps.as_ref(), mock_env(), QueryMsg::QueryState {}).unwrap();
        let resp: QueryResponse = from_binary(&bin).unwrap();

        // Ensure the expected init fields were properly stored.
        assert_eq!(DEFAULT_CONTRACT_NAME, resp.contract_name);
        assert_eq!(Uint128::new(100), resp.onboarding_cost);
        assert_eq!(DEFAULT_ONBOARDING_DENOM, resp.onboarding_denom.as_str());
        assert_eq!(
            DEFAULT_FEE_COLLECTION_ADDRESS,
            resp.fee_collection_address.as_str()
        );
        assert_eq!(Decimal::percent(DEFAULT_FEE_PERCENT), resp.fee_percent);
        assert_eq!(DEFAULT_ORACLE_ADDRESS, resp.oracle_address.as_str());
    }
}
