use crate::core::error::ContractError;
use crate::core::msg::InitMsg;
use crate::core::state::{config, State};
use crate::util::conversions::to_percent;
use cosmwasm_std::{Decimal, DepsMut, Env, MessageInfo, Response, Uint128};
use provwasm_std::{bind_name, NameBinding, ProvenanceMsg, ProvenanceQuery};
use crate::migrate::version_info::migrate_version_info;

pub fn init_contract(
    deps: DepsMut<ProvenanceQuery>,
    env: Env,
    info: MessageInfo,
    msg: InitMsg,
) -> Result<Response<ProvenanceMsg>, ContractError> {
    // Ensure no funds were sent with the message
    if !info.funds.is_empty() {
        return ContractError::std_err("purchase funds are not allowed to be sent during init");
    }

    if msg.fee_percent > Decimal::one() {
        return ContractError::std_err(format!(
            "fee [{}%] must be less than 100%",
            to_percent(msg.fee_percent)
        ));
    }

    // Create and save contract config state. The name is used for setting attributes on user accounts
    config(deps.storage).save(&State {
        payable_type: msg.payable_type.clone(),
        contract_name: msg.contract_name.clone(),
        onboarding_cost: Uint128::new(msg.onboarding_cost.as_str().parse::<u128>().unwrap()),
        onboarding_denom: msg.onboarding_denom.clone(),
        fee_collection_address: deps
            .api
            .addr_validate(msg.fee_collection_address.as_str())?,
        fee_percent: msg.fee_percent,
        oracle_address: deps.api.addr_validate(msg.oracle_address.as_str())?,
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
    use crate::testutil::test_utilities::{test_instantiate, InstArgs, DEFAULT_ONBOARDING_DENOM};
    use cosmwasm_std::testing::mock_info;
    use cosmwasm_std::{coin, CosmosMsg, StdError};
    use provwasm_mocks::mock_dependencies;
    use provwasm_std::{NameMsgParams, ProvenanceMsgParams};
    use crate::migrate::version_info::{CONTRACT_NAME, CONTRACT_VERSION, get_version_info};

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
                oracle_address: "oracle".into(),
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
        let generated_state = config(deps.as_mut().storage).load().unwrap();
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
        assert_eq!(
            "oracle",
            generated_state.oracle_address.as_str(),
            "expected state to include the proper oracle address",
        );
        let version_info = get_version_info(deps.as_ref().storage).unwrap();
        assert_eq!(
            CONTRACT_NAME,
            version_info.contract,
            "the contract name should be properly stored after a successful instantiation",
        );
        assert_eq!(
            CONTRACT_VERSION,
            version_info.version,
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
            ContractError::Std(std_err) => match std_err {
                StdError::GenericErr { msg, .. } => {
                    assert_eq!(
                        "fee [101%] must be less than 100%", msg,
                        "unexpected error message during bad fee percent provided",
                    )
                }
                _ => panic!("unexpected stderr encountered during bad fee percent"),
            },
            _ => panic!("unexpected error encountered when too high fee percent provided"),
        };
    }
}
