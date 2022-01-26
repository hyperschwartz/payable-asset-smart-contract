use cosmwasm_std::{
    coin, to_binary, BankMsg, Binary, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo,
    Response, StdError, StdResult, Uint128,
};
use provwasm_std::{
    add_attribute, bind_name, AttributeValueType, NameBinding, PartyType, ProvenanceMsg,
    ProvenanceQuerier,
};
use std::ops::Mul;

use crate::error::ContractError;
use crate::helper::to_percent;
use crate::msg::{ExecuteMsg, InitMsg, MigrateMsg, QueryMsg};
use crate::state::{config, config_read, State};

/// Initialize the contract
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InitMsg,
) -> Result<Response<ProvenanceMsg>, StdError> {
    // Ensure no funds were sent with the message
    if !info.funds.is_empty() {
        let err = "purchase funds are not allowed to be sent during init";
        return Err(StdError::generic_err(err));
    }

    if msg.fee_percent > Decimal::one() {
        return Err(StdError::generic_err(format!(
            "fee [{}%] must be less than 100%",
            to_percent(msg.fee_percent)
        )));
    }

    // Create and save contract config state. The name is used for setting attributes on user accounts
    config(deps.storage).save(&State {
        contract_name: msg.contract_name.clone(),
        onboarding_cost: Uint128::new(msg.onboarding_cost.as_str().parse::<u128>().unwrap()),
        onboarding_denom: msg.onboarding_denom.clone(),
        fee_collection_address: deps
            .api
            .addr_validate(msg.fee_collection_address.as_str())?,
        fee_percent: msg.fee_percent,
        oracle_address: deps.api.addr_validate(msg.oracle_address.as_str())?,
    })?;

    // Create a message that will bind a restricted name to the contract address.
    let bind_name_msg = bind_name(
        &msg.contract_name,
        env.contract.address,
        NameBinding::Restricted,
    )?;

    // Dispatch messages and emit event attributes
    Ok(Response::new()
        .add_message(bind_name_msg)
        .add_attribute("action", "init"))
}

/// Query contract state.
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::QueryRequest {} => {
            let state = config_read(deps.storage).load()?;
            let json = to_binary(&state)?;
            Ok(json)
        }
    }
}

/// Handle purchase messages.
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response<ProvenanceMsg>, ContractError> {
    match msg {
        ExecuteMsg::RegisterScope { scope_id } => register_scope(deps, info, scope_id),
    }
}

fn register_scope(
    deps: DepsMut,
    info: MessageInfo,
    scope_id: String,
) -> Result<Response<ProvenanceMsg>, ContractError> {
    let state = config(deps.storage).load()?;
    let querier = ProvenanceQuerier::new(&deps.querier);
    let scope = querier.get_scope(scope_id.clone())?;
    if !scope
        .owners
        .into_iter()
        .any(|owner| owner.role == PartyType::Owner && owner.address == info.sender.as_str())
    {
        return Err(ContractError::Unauthorized);
    }
    let scope_tag_request = add_attribute(
        // Tag the scope with the attribute
        deps.api.addr_validate(&scope_id)?,
        // Use the contract name as the tag
        state.contract_name.clone(),
        // TODO: Maybe don't use the scope id as the value of the attribute. Something more useful will likely present its as development continues
        to_binary(&scope_id)?,
        AttributeValueType::String,
    )?;
    let fee_charge_response = validate_fee_params_get_messages(&info, &state)?;
    let mut response = Response::new().add_message(scope_tag_request);
    if let Some(fee_message) = fee_charge_response.fee_charge_message {
        response = response.add_message(fee_message).add_attribute(
            "oracle_funds_kept",
            format!(
                "{}{}",
                fee_charge_response.oracle_fee_amount_kept, state.onboarding_denom
            ),
        )
    }
    if let Some(refund_message) = fee_charge_response.fee_refund_message {
        response = response.add_message(refund_message).add_attribute(
            "refund_amount",
            format!(
                "{}{}",
                fee_charge_response.refund_amount, state.onboarding_denom
            ),
        )
    }
    Ok(response)
}

struct FeeChargeResponse {
    fee_charge_message: Option<CosmosMsg<ProvenanceMsg>>,
    fee_refund_message: Option<CosmosMsg<ProvenanceMsg>>,
    refund_amount: u128,
    oracle_fee_amount_kept: u128,
}

fn validate_fee_params_get_messages(
    info: &MessageInfo,
    state: &State,
) -> Result<FeeChargeResponse, ContractError> {
    let invalid_funds = info
        .funds
        .iter()
        .filter(|coin| coin.denom != state.onboarding_denom)
        .map(|coin| coin.denom.clone())
        .collect::<Vec<String>>();
    if !invalid_funds.is_empty() {
        return Err(ContractError::InvalidFundsProvided {
            valid_denom: state.onboarding_denom.clone(),
            invalid_denoms: invalid_funds,
        });
    }
    let onboarding_cost = state.onboarding_cost;
    let funds_sent = match info
        .clone()
        .funds
        .into_iter()
        .find(|coin| coin.denom == state.onboarding_denom)
    {
        Some(coin) => {
            let amount_sent = coin.amount;
            if onboarding_cost > amount_sent {
                return Err(ContractError::InsufficientFundsProvided {
                    amount_needed: onboarding_cost.u128(),
                    amount_provided: amount_sent.u128(),
                });
            } else {
                amount_sent
            }
        }
        None => {
            if onboarding_cost.u128() > 0 {
                return Err(ContractError::NoFundsProvided {
                    valid_denom: state.onboarding_denom.clone(),
                });
            } else {
                Uint128::zero()
            }
        }
    };
    // The collected fee is the fee percent * the onboarding cost.  The remaining amount will stay in
    // the contract's account, waiting for the oracle to withdraw it
    let fee_collected_amount = onboarding_cost.mul(state.fee_percent);
    let fee_charge_message = if fee_collected_amount.u128() > 0 {
        Some(CosmosMsg::Bank(BankMsg::Send {
            to_address: state.fee_collection_address.clone().into(),
            amount: vec![coin(
                fee_collected_amount.u128(),
                state.onboarding_denom.clone(),
            )],
        }))
    } else {
        None
    };
    // If any excess funds are sent beyond the onboarding cost, they should be refunded to the sender
    let refund_amount = funds_sent - onboarding_cost;
    let fee_refund_message = if refund_amount.u128() > 0 {
        Some(CosmosMsg::Bank(BankMsg::Send {
            to_address: info.sender.clone().into(),
            amount: vec![coin(refund_amount.u128(), state.onboarding_denom.clone())],
        }))
    } else {
        None
    };
    Ok(FeeChargeResponse {
        fee_charge_message,
        fee_refund_message,
        refund_amount: 69,
        oracle_fee_amount_kept: 420,
    })
}

/// Called when migrating a contract instance to a new code ID.
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    Ok(Response::default())
}

#[cfg(test)]
mod tests {
    use crate::msg::QueryResponse;

    use super::*;
    use cosmwasm_std::testing::{mock_env, mock_info};
    use cosmwasm_std::{from_binary, CosmosMsg, Decimal};
    use provwasm_mocks::mock_dependencies;
    use provwasm_std::{AttributeMsgParams, NameMsgParams, Party, ProvenanceMsgParams, Scope};

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
    }

    #[test]
    fn test_invalid_init_funds_provided() {
        let mut deps = mock_dependencies(&[]);
        let err = test_instantiate(
            deps.as_mut(),
            InstArgs {
                info: mock_info("sender", &vec![coin(50, "nhash")]),
                ..Default::default()
            },
        )
        .unwrap_err();
        match err {
            StdError::GenericErr { msg, .. } => {
                assert_eq!(
                    "purchase funds are not allowed to be sent during init",
                    msg.as_str(),
                    "unexpected error message during fund failure",
                )
            }
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
            StdError::GenericErr { msg, .. } => {
                assert_eq!(
                    "fee [101%] must be less than 100%", msg,
                    "unexpected error message during bad fee percent provided",
                )
            }
            _ => panic!("unexpected error encountered when too high fee percent provided"),
        };
    }

    #[test]
    fn test_query() {
        // Create mocks
        let mut deps = mock_dependencies(&[]);

        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();

        // Call the smart contract query function to get stored state.
        let bin = query(deps.as_ref(), mock_env(), QueryMsg::QueryRequest {}).unwrap();
        let resp: QueryResponse = from_binary(&bin).unwrap();

        // Ensure the expected init fields were properly stored.
        assert_eq!("payables.asset", resp.contract_name);
        assert_eq!(Uint128::new(100), resp.onboarding_cost);
        assert_eq!("nhash", resp.onboarding_denom.as_str());
        assert_eq!("feebucket", resp.fee_collection_address.as_str());
        assert_eq!(Decimal::percent(75), resp.fee_percent);
        assert_eq!("matt", resp.oracle_address.as_str());
    }

    #[test]
    fn test_register_valid_no_refund() {
        let mut deps = mock_dependencies(&[]);
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
        let scope_id = "8fa88d64-7ed7-11ec-a2df-abe6f9c86f86".to_string();
        let owner_address = "owner_person".to_string();
        // Dupe the querier to respond with a scope that will pass validation
        deps.querier.with_scope(get_duped_scope(
            scope_id.clone(),
            "spec".into(),
            owner_address.clone(),
        ));
        let response = execute(
            deps.as_mut(),
            mock_env(),
            mock_info(owner_address.as_str(), &[coin(100, "nhash".to_string())]),
            ExecuteMsg::RegisterScope {
                scope_id: scope_id.clone(),
            },
        )
        .unwrap();
        assert_eq!(
            2,
            response.messages.len(),
            "expected two messages to be contained in the payload"
        );
        response.messages.into_iter().for_each(|msg| match msg.msg {
            CosmosMsg::Custom(ProvenanceMsg { params, .. }) => {
                match params {
                    ProvenanceMsgParams::Attribute(AttributeMsgParams::AddAttribute {
                        name,
                        value,
                        value_type,
                        ..
                    }) => {
                        assert_eq!("payables.asset".to_string(), name, "expected the registered attribute name to be the contract name");
                        assert_eq!(
                            scope_id.clone(),
                            from_binary::<String>(&value)
                                .expect("unable to deserialize value from result"),
                            "expected the registered value to the scope uuid",
                        );
                        assert_eq!(AttributeValueType::String, value_type, "expected the value type to be stored as a string");
                    }
                    _ => panic!("unexpected provenance message type contaiend in result"),
                }
            },
            CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                assert_eq!("feebucket", to_address, "expected the fee send to go the default fee collection address");
                assert_eq!(1, amount.len(), "expected only one coin to be added to the fee transfer");
                let coin = amount.first().unwrap();
                assert_eq!(75, coin.amount.u128(), "expected the fee charged to be equal to 75, because the onboarding cost is 100 and the fee percent is 75%");
                assert_eq!("nhash", coin.denom.as_str(), "expected the fee's denomination to equate to the contract's specified denomination");
            },
            _ => panic!("unexpected response message type"),
        });
    }

    #[test]
    fn test_register_valid_with_refund() {
        let mut deps = mock_dependencies(&[]);
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
        let scope_id = "8fa88d64-7ed7-11ec-a2df-abe6f9c86f86".to_string();
        let owner_address = "owner_person".to_string();
        // Dupe the querier to respond with a scope that will pass validation
        deps.querier.with_scope(get_duped_scope(
            scope_id.clone(),
            "spec".into(),
            owner_address.clone(),
        ));
        let response = execute(
            deps.as_mut(),
            mock_env(),
            mock_info(owner_address.as_str(), &[coin(150, "nhash".to_string())]),
            ExecuteMsg::RegisterScope {
                scope_id: scope_id.clone(),
            },
        )
        .unwrap();
        assert_eq!(
            3,
            response.messages.len(),
            "expected three messages to be contained in the payload"
        );
        response.messages.into_iter().for_each(|msg| match msg.msg {
            CosmosMsg::Custom(ProvenanceMsg { params, .. }) => {
                match params {
                    ProvenanceMsgParams::Attribute(AttributeMsgParams::AddAttribute {
                                                       name,
                                                       value,
                                                       value_type,
                                                       ..
                                                   }) => {
                        assert_eq!("payables.asset".to_string(), name, "expected the registered attribute name to be the contract name");
                        assert_eq!(
                            scope_id.clone(),
                            from_binary::<String>(&value)
                                .expect("unable to deserialize value from result"),
                            "expected the registered value to the scope uuid",
                        );
                        assert_eq!(AttributeValueType::String, value_type, "expected the value type to be stored as a string");
                    }
                    _ => panic!("unexpected provenance message type contaiend in result"),
                }
            },
            CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                assert_eq!(1, amount.len(), "expected only one coin to be added to the fee transfer");
                let coin = amount.first().unwrap();
                match to_address.as_str() {
                    "feebucket" => {
                        assert_eq!(75, coin.amount.u128(), "expected the fee charged to be equal to 75, because the onboarding cost is 100 and the fee percent is 75%");
                        assert_eq!("nhash", coin.denom.as_str(), "expected the fee's denomination to equate to the contract's specified denomination");
                    },
                    "owner_person" => {
                        assert_eq!(50, coin.amount.u128(), "expected the overage amount to be refunded to the sender");
                        assert_eq!("nhash", coin.denom.as_str(), "expected the refund's denomination to equate to the contract's specified denomination");
                    },
                    _ => panic!("unexpected address for bank message send"),
                }
            },
            _ => panic!("unexpected response message type"),
        });
    }

    #[test]
    fn test_register_invalid_sender_for_scope() {
        let mut deps = mock_dependencies(&[]);
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
        let scope_id = "rando-id".to_string();
        let owner_address = "owner_person".to_string();
        deps.querier.with_scope(get_duped_scope(
            scope_id.clone(),
            "spec".into(),
            "wrong_owner_address".into(),
        ));
        let failure = execute(
            deps.as_mut(),
            mock_env(),
            mock_info(owner_address.as_str(), &[coin(100, "nhash".to_string())]),
            ExecuteMsg::RegisterScope {
                scope_id: scope_id.clone(),
            },
        )
        .unwrap_err();
        assert!(
            matches!(failure, ContractError::Unauthorized),
            "expected a mismatched owner address on the scope to be met with an unauthorized error",
        );
    }

    #[test]
    fn test_register_invalid_fund_denom() {
        let mut deps = mock_dependencies(&[]);
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
        deps.querier.with_scope(get_duped_scope(
            "scope-id".into(),
            "spec-id".into(),
            "owner-address".into(),
        ));
        let failure = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner-address", &[coin(100, "nothash".to_string())]),
            ExecuteMsg::RegisterScope {
                scope_id: "scope-id".into(),
            },
        )
        .unwrap_err();
        match failure {
            ContractError::InvalidFundsProvided {
                valid_denom,
                invalid_denoms,
            } => {
                assert_eq!(
                    "nhash", valid_denom,
                    "expected the valid denomination returned to be the default value"
                );
                assert_eq!(
                    1,
                    invalid_denoms.len(),
                    "expected the one invalid value to be returned"
                );
                let invalid_denom = invalid_denoms.first().unwrap();
                assert_eq!("nothash", invalid_denom.as_str(), "expected the invalid denomination returned to be a reflection of the bad input");
            }
            _ => panic!("unexpected contract error encountered"),
        };
    }

    #[test]
    fn test_register_no_funds_provided_and_fee_charge_non_zero() {
        let mut deps = mock_dependencies(&[]);
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
        deps.querier.with_scope(get_duped_scope(
            "scope-id".into(),
            "spec-id".into(),
            "owner-address".into(),
        ));
        let failure = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner-address", &[]),
            ExecuteMsg::RegisterScope {
                scope_id: "scope-id".into(),
            },
        )
        .unwrap_err();
        match failure {
            ContractError::NoFundsProvided { valid_denom } => {
                assert_eq!(
                    "nhash", valid_denom,
                    "the error should reflect the desired fund type"
                );
            }
            _ => panic!("unexpected contract error encountered"),
        };
    }

    #[test]
    fn test_register_insufficient_funds_provided() {
        let mut deps = mock_dependencies(&[]);
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
        deps.querier.with_scope(get_duped_scope(
            "scope-id".into(),
            "spec-id".into(),
            "owner-address".into(),
        ));
        let failure = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner-address", &[coin(99, "nhash".to_string())]),
            ExecuteMsg::RegisterScope {
                scope_id: "scope-id".into(),
            },
        )
        .unwrap_err();
        match failure {
            ContractError::InsufficientFundsProvided {
                amount_needed,
                amount_provided,
            } => {
                assert_eq!(
                    100, amount_needed,
                    "expected the amount needed to reflect the default value"
                );
                assert_eq!(99, amount_provided, "expected the amount provided to reflect the amount provided when the contract was executed");
            }
            _ => panic!("unexpected contract error encountered"),
        };
    }

    struct InstArgs {
        env: Env,
        info: MessageInfo,
        contract_name: String,
        onboarding_cost: String,
        onboarding_denom: String,
        fee_collection_address: String,
        fee_percent: Decimal,
        oracle_address: String,
    }

    impl Default for InstArgs {
        fn default() -> Self {
            InstArgs {
                env: mock_env(),
                info: mock_info("admin", &[]),
                contract_name: "payables.asset".into(),
                onboarding_cost: "100".into(),
                onboarding_denom: "nhash".into(),
                fee_collection_address: "feebucket".into(),
                fee_percent: Decimal::percent(75),
                oracle_address: "matt".into(),
            }
        }
    }

    impl InstArgs {
        fn default() -> InstArgs {
            InstArgs {
                ..Default::default()
            }
        }
    }

    fn test_instantiate(
        deps: DepsMut,
        args: InstArgs,
    ) -> Result<Response<ProvenanceMsg>, StdError> {
        instantiate(
            deps,
            args.env,
            args.info,
            InitMsg {
                contract_name: args.contract_name,
                onboarding_cost: args.onboarding_cost,
                onboarding_denom: args.onboarding_denom,
                fee_collection_address: args.fee_collection_address,
                fee_percent: args.fee_percent,
                oracle_address: args.oracle_address,
            },
        )
    }

    /// Dupes the querier to respond with the given information when a scope is requested.
    fn get_duped_scope(scope_id: String, specification_id: String, owner_address: String) -> Scope {
        Scope {
            scope_id: scope_id.clone(),
            specification_id: specification_id.clone(),
            owners: vec![Party {
                address: owner_address.clone(),
                role: PartyType::Owner,
            }],
            data_access: vec![],
            value_owner_address: owner_address.clone(),
        }
    }
}
