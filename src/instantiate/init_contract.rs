use crate::core::error::ContractError;
use crate::core::msg::InitMsg;
use crate::core::state::{config_v2, StateV2};
use crate::migrate::version_info::migrate_version_info;
use cosmwasm_std::{DepsMut, Env, MessageInfo, Response, Uint128};
use provwasm_std::{bind_name, NameBinding, ProvenanceMsg, ProvenanceQuery};

/// Standard entrypoint for contract -> instantiate.  Generates the initial StateV2 value that
/// drives and controls various configurations, and automatically binds the contract name to its
/// address, ensuring that it and it alone has access to its spawned attributes on the registered
/// payables' scopes.  Also establishes the initial version info storage.
pub fn init_contract(
    deps: DepsMut<ProvenanceQuery>,
    env: Env,
    info: MessageInfo,
    msg: InitMsg,
) -> Result<Response<ProvenanceMsg>, ContractError> {
    // Ensure no funds were sent with the message
    if !info.funds.is_empty() {
        return ContractError::std_err("purchase funds are not allowed to be sent during init")
            .to_result();
    }
    // Create and save contract config state. The name is used for setting attributes on user accounts
    config_v2(deps.storage).save(&StateV2 {
        contract_name: msg.contract_name.clone(),
        onboarding_cost: Uint128::new(msg.onboarding_cost.parse::<u128>().unwrap()),
        onboarding_denom: msg.onboarding_denom.clone(),
        fee_collection_address: deps
            .api
            .addr_validate(msg.fee_collection_address.as_str())?,
        fee_percent: msg.fee_percent,
        // Always default to non-local if the value is not provided
        is_local: msg.is_local.unwrap_or(false),
    })?;
    // Create a message that will bind a restricted name to the contract address.
    let bind_name_msg = bind_name(
        &msg.contract_name,
        env.contract.address,
        NameBinding::Restricted,
    )?;
    // Set the version info to the default contract values on instantiation
    migrate_version_info(deps.storage)?;
    // Dispatch messages and emit event attributes
    Ok(Response::new()
        .add_message(bind_name_msg)
        .add_attribute("action", "init"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::state::config_read_v2;
    use crate::migrate::version_info::{get_version_info, CONTRACT_NAME, CONTRACT_VERSION};
    use crate::testutil::test_utilities::{test_instantiate, InstArgs, DEFAULT_ONBOARDING_DENOM};
    use cosmwasm_std::testing::mock_info;
    use cosmwasm_std::{coin, CosmosMsg, Decimal, StdError};
    use provwasm_mocks::mock_dependencies;
    use provwasm_std::{NameMsgParams, ProvenanceMsgParams};

    #[test]
    fn test_valid_init() {
        // Create mocks
        let mut deps = mock_dependencies(&[]);

        // Create valid config state
        let res = test_instantiate(
            deps.as_mut(),
            InstArgs {
                contract_name: "payables.asset".into(),
                onboarding_cost: "420".into(),
                onboarding_denom: "usdf".into(),
                fee_collection_address: "test-address".into(),
                fee_percent: Decimal::percent(50),
                ..Default::default()
            },
        )
        .unwrap();

        // Ensure a message was created to bind the name to the contract address.
        assert_eq!(res.messages.len(), 1);
        match &res.messages[0].msg {
            CosmosMsg::Custom(msg) => match &msg.params {
                ProvenanceMsgParams::Name(p) => match &p {
                    NameMsgParams::BindName { name, .. } => assert_eq!(name, "payables.asset"),
                    _ => panic!("unexpected name params"),
                },
                _ => panic!("unexpected provenance params"),
            },
            _ => panic!("unexpected cosmos message"),
        }
        let generated_state = config_read_v2(deps.as_ref().storage).load().unwrap();
        assert_eq!(
            "payables.asset",
            generated_state.contract_name.as_str(),
            "expected state to include the proper contract name",
        );
        assert_eq!(
            Uint128::new(420),
            generated_state.onboarding_cost,
            "expected state to include the proper onboarding cost",
        );
        assert_eq!(
            "usdf",
            generated_state.onboarding_denom.as_str(),
            "expected state to include the proper onboarding denom",
        );
        assert_eq!(
            "test-address",
            generated_state.fee_collection_address.as_str(),
            "expected state to include the proper fee collection address",
        );
        assert_eq!(
            Decimal::percent(50),
            generated_state.fee_percent,
            "expected state to include the proper fee percent",
        );
        let version_info = get_version_info(deps.as_ref().storage).unwrap();
        assert_eq!(
            CONTRACT_NAME, version_info.contract,
            "the contract name should be properly stored after a successful instantiation",
        );
        assert_eq!(
            CONTRACT_VERSION, version_info.version,
            "the contract version should be properly stored after a succesful instantiation",
        );
    }

    #[test]
    fn test_invalid_init_funds_provided() {
        let mut deps = mock_dependencies(&[]);
        let err = test_instantiate(
            deps.as_mut(),
            InstArgs {
                info: mock_info("sender", &vec![coin(50, DEFAULT_ONBOARDING_DENOM)]),
                ..Default::default()
            },
        )
        .unwrap_err();
        match err {
            ContractError::Std(std_err) => match std_err {
                StdError::GenericErr { msg, .. } => {
                    assert_eq!(
                        "purchase funds are not allowed to be sent during init", msg,
                        "unexpected error message during fund failure",
                    )
                }
                _ => panic!("unexpected stderr encountered when funds provided"),
            },
            _ => panic!("unexpected error encountered when funds provided"),
        };
    }

    #[test]
    fn test_invalid_init_too_high_fee_percent() {
        let mut deps = mock_dependencies(&[]);
        let err = test_instantiate(
            deps.as_mut(),
            InstArgs {
                fee_percent: Decimal::percent(101),
                ..Default::default()
            },
        )
        .unwrap_err();
        match err {
            ContractError::InvalidFields { fields } => {
                assert!(
                    fields.contains(&"fee_percent".to_string()),
                    "the fee percent field should be detected as invalid when too high a fee is provided",
                );
            }
            _ => panic!("unexpected error encountered when too high fee percent provided"),
        };
    }
}
