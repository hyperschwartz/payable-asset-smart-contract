use cosmwasm_std::{
    coin, to_binary, Attribute, BankMsg, Binary, CosmosMsg, Decimal, Deps, DepsMut, Env,
    MessageInfo, Response, StdError, StdResult, Uint128,
};
use provwasm_std::{
    activate_marker, add_attribute, bind_name, create_marker, finalize_marker, grant_marker_access,
    AttributeValueType, MarkerType, NameBinding, ProvenanceMsg,
};
use std::ops::Mul;

use crate::error::ContractError;
use crate::helper::{to_percent, CONTRACT_MARKER_PERMISSIONS, SENDER_MARKER_PERMISSIONS};
use crate::msg::{ExecuteMsg, InitMsg, MigrateMsg, QueryMsg};
use crate::register_payable::RegisterPayableMarkerV1;
use crate::state::{
    config, config_read, payable_meta_storage, payable_meta_storage_read, PayableMeta, State,
};

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
        QueryMsg::QueryState {} => {
            let state = config_read(deps.storage).load()?;
            let json = to_binary(&state)?;
            Ok(json)
        }
        QueryMsg::QueryPayable { marker_denom } => {
            let meta_storage = payable_meta_storage_read(deps.storage);
            let payable_meta = meta_storage.load(marker_denom.as_bytes())?;
            let json = to_binary(&payable_meta)?;
            Ok(json)
        }
    }
}

/// Handle purchase messages.
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response<ProvenanceMsg>, ContractError> {
    match msg {
        ExecuteMsg::RegisterPayableMarker {
            marker_address,
            marker_denom,
            scope_id,
            payable_denom,
            payable_total,
        } => {
            let marker_address = deps.api.addr_validate(marker_address.as_str())?;
            register_payable_marker(
                deps,
                env,
                info,
                RegisterPayableMarkerV1 {
                    marker_address,
                    marker_denom,
                    scope_id,
                    payable_denom,
                    payable_total,
                },
            )
        }
    }
}

fn register_payable_marker(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    register: RegisterPayableMarkerV1,
) -> Result<Response<ProvenanceMsg>, ContractError> {
    let state = config(deps.storage).load()?;
    let mut messages: Vec<CosmosMsg<ProvenanceMsg>> = vec![];
    let mut attributes: Vec<Attribute> = vec![];
    // Create a marker that owns the scope
    let marker_gen_request = create_marker(
        // Start with only one coin for our new marker type. This can be inflated later
        1,
        // Assume the marker denomination is valid
        register.marker_denom.clone(),
        MarkerType::Restricted,
    )?;
    messages.push(marker_gen_request);
    let wallet_marker_grant_request = grant_marker_access(
        register.marker_denom.clone(),
        info.sender.clone(),
        SENDER_MARKER_PERMISSIONS.to_vec(),
    )?;
    messages.push(wallet_marker_grant_request);
    let contract_marker_grant_request = grant_marker_access(
        register.marker_denom.clone(),
        env.contract.address,
        CONTRACT_MARKER_PERMISSIONS.to_vec(),
    )?;
    messages.push(contract_marker_grant_request);
    let marker_finalize_request = finalize_marker(register.marker_denom.clone())?;
    messages.push(marker_finalize_request);
    let marker_activate_request = activate_marker(register.marker_denom.clone())?;
    messages.push(marker_activate_request);
    let marker_tag_request = add_attribute(
        // Tag the newly-created marker address with an attribute indicating its managed scope
        register.marker_address.clone(),
        //
        state.contract_name.clone(),
        to_binary(&register.scope_id)?,
        AttributeValueType::String,
    )?;
    messages.push(marker_tag_request);
    let fee_charge_response = validate_fee_params_get_messages(&info, &state)?;
    if let Some(fee_message) = fee_charge_response.fee_charge_message {
        messages.push(fee_message);
        attributes.push(Attribute::new(
            "oracle_funds_kept",
            format!(
                "{}{}",
                fee_charge_response.oracle_fee_amount_kept, state.onboarding_denom
            ),
        ));
    }
    if let Some(refund_message) = fee_charge_response.fee_refund_message {
        messages.push(refund_message);
        attributes.push(Attribute::new(
            "refund_amount",
            format!(
                "{}{}",
                fee_charge_response.refund_amount, state.onboarding_denom
            ),
        ));
    }
    let payable_meta = PayableMeta {
        marker_address: register.marker_address,
        marker_denom: register.marker_denom.clone(),
        scope_id: register.scope_id,
        payable_denom: register.payable_denom,
        payable_total_owed: register.payable_total,
        payable_remaining_owed: register.payable_total,
    };
    let mut meta_storage = payable_meta_storage(deps.storage);
    meta_storage.save(register.marker_denom.as_bytes(), &payable_meta)?;
    Ok(Response::new()
        .add_messages(messages)
        .add_attributes(attributes))
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
        refund_amount: refund_amount.u128(),
        oracle_fee_amount_kept: (funds_sent - onboarding_cost).u128(),
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
    use provwasm_std::{AttributeMsgParams, MarkerMsgParams, NameMsgParams, ProvenanceMsgParams};

    const DEFAULT_INFO_NAME: &str = "admin";
    const DEFAULT_CONTRACT_NAME: &str = "payables.asset";
    const DEFAULT_ONBOARDING_COST: &str = "100";
    const DEFAULT_ONBOARDING_DENOM: &str = "nhash";
    const DEFAULT_FEE_COLLECTION_ADDRESS: &str = "feebucket";
    const DEFAULT_FEE_PERCENT: u64 = 75;
    const DEFAULT_ORACLE_ADDRESS: &str = "matt";
    const DEFAULT_MARKER_ADDRESS: &str = "tp1cf4n639gawu07wmspwpg9wkry0zn6vhdppcnrv";
    const DEFAULT_MARKER_DENOM: &str = "invoice-480d2352-7af8-11ec-88fb-9f79ab0248a0";
    const DEFAULT_SCOPE_ID: &str = "scope1qpyq6g6j0tuprmyglw0hn2czfzsq6fcyl8";
    const DEFAULT_PAYABLE_DENOM: &str = "nhash";
    const DEFAULT_PAYABLE_TOTAL: u128 = 10000;
    const MOCK_COSMOS_CONTRACT_ADDRESS: &str = "cosmos2contract";

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
                info: mock_info("sender", &vec![coin(50, DEFAULT_ONBOARDING_DENOM)]),
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

    #[test]
    fn test_register_valid_no_refund() {
        let mut deps = mock_dependencies(&[]);
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
        let response = execute(
            deps.as_mut(),
            mock_env(),
            mock_info(
                DEFAULT_INFO_NAME,
                &[coin(100, DEFAULT_ONBOARDING_DENOM.to_string())],
            ),
            default_register_payable(),
        )
        .unwrap();
        assert_eq!(
            7,
            response.messages.len(),
            "seven messages expected: create marker, two grant marker, one finalize marker, one activate marker, one add attribute, and one fee exchange",
        );
        response.messages.into_iter().for_each(|msg| match msg.msg {
            CosmosMsg::Custom(ProvenanceMsg { params, .. }) => {
                match params {
                    // Handled in order in which they appear in the contract's execution
                    ProvenanceMsgParams::Marker(MarkerMsgParams::CreateMarker { coin, marker_type }) => {
                        assert_eq!(DEFAULT_MARKER_DENOM, coin.denom.as_str());
                        assert_eq!(1, coin.amount.u128());
                        assert_eq!(MarkerType::Restricted, marker_type);
                    },
                    ProvenanceMsgParams::Marker(MarkerMsgParams::GrantMarkerAccess { denom, address, permissions }) => {
                        assert_eq!(DEFAULT_MARKER_DENOM, denom.as_str());
                        match address.as_str() {
                            // Declared at the beginning for the contract address
                            DEFAULT_INFO_NAME => {
                                assert_eq!(SENDER_MARKER_PERMISSIONS.to_vec(), permissions);
                            },
                            // mock_env() creates this as the default contract address
                            MOCK_COSMOS_CONTRACT_ADDRESS => {
                                assert_eq!(CONTRACT_MARKER_PERMISSIONS.to_vec(), permissions);
                            },
                            _ => panic!("unexpected address encountered"),
                        }
                    },
                    ProvenanceMsgParams::Attribute(AttributeMsgParams::AddAttribute {
                        name,
                        value,
                        value_type,
                        ..
                    }) => {
                        assert_eq!(DEFAULT_CONTRACT_NAME, name.as_str(), "expected the registered attribute name to be the contract name");
                        assert_eq!(
                            DEFAULT_SCOPE_ID.to_string(),
                            from_binary::<String>(&value)
                                .expect("unable to deserialize value from result"),
                            "expected the registered value to the scope uuid",
                        );
                        assert_eq!(AttributeValueType::String, value_type, "expected the value type to be stored as a string");
                    },
                    ProvenanceMsgParams::Marker(MarkerMsgParams::FinalizeMarker { denom }) => {
                        assert_eq!(DEFAULT_MARKER_DENOM, denom.as_str());
                    },
                    ProvenanceMsgParams::Marker(MarkerMsgParams::ActivateMarker { denom }) => {
                        assert_eq!(DEFAULT_MARKER_DENOM, denom.as_str());
                    },
                    _ => panic!("unexpected provenance message type contaiend in result"),
                }
            },
            CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                assert_eq!(DEFAULT_FEE_COLLECTION_ADDRESS, to_address, "expected the fee send to go the default fee collection address");
                assert_eq!(1, amount.len(), "expected only one coin to be added to the fee transfer");
                let coin = amount.first().unwrap();
                assert_eq!(75, coin.amount.u128(), "expected the fee charged to be equal to 75, because the onboarding cost is 100 and the fee percent is 75%");
                assert_eq!(DEFAULT_ONBOARDING_DENOM, coin.denom.as_str(), "expected the fee's denomination to equate to the contract's specified denomination");
            },
            _ => panic!("unexpected response message type"),
        });
    }

    #[test]
    fn test_register_valid_with_refund() {
        let mut deps = mock_dependencies(&[]);
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
        let response = execute(
            deps.as_mut(),
            mock_env(),
            mock_info(
                DEFAULT_INFO_NAME,
                &[coin(150, DEFAULT_ONBOARDING_DENOM.to_string())],
            ),
            default_register_payable(),
        )
        .unwrap();
        assert_eq!(
            8,
            response.messages.len(),
            "seven messages expected: create marker, two grant marker, one finalize marker, one activate marker, one add attribute, one fee exchange, and one fee refund",
        );
        response.messages.into_iter().for_each(|msg| match msg.msg {
            CosmosMsg::Custom(ProvenanceMsg { params, .. }) => {
                match params {
                    // Handled in order in which they appear in the contract's execution
                    ProvenanceMsgParams::Marker(MarkerMsgParams::CreateMarker { coin, marker_type }) => {
                        assert_eq!(DEFAULT_MARKER_DENOM, coin.denom.as_str());
                        assert_eq!(1, coin.amount.u128());
                        assert_eq!(MarkerType::Restricted, marker_type);
                    },
                    ProvenanceMsgParams::Marker(MarkerMsgParams::GrantMarkerAccess { denom, address, permissions }) => {
                        assert_eq!(DEFAULT_MARKER_DENOM, denom.as_str());
                        match address.as_str() {
                            // Declared at the beginning for the contract address
                            DEFAULT_INFO_NAME => {
                                assert_eq!(SENDER_MARKER_PERMISSIONS.to_vec(), permissions);
                            },
                            // mock_env() creates this as the default contract address
                            MOCK_COSMOS_CONTRACT_ADDRESS => {
                                assert_eq!(CONTRACT_MARKER_PERMISSIONS.to_vec(), permissions);
                            },
                            _ => panic!("unexpected address encountered"),
                        }
                    },
                    ProvenanceMsgParams::Attribute(AttributeMsgParams::AddAttribute {
                                                       name,
                                                       value,
                                                       value_type,
                                                       ..
                                                   }) => {
                        assert_eq!(DEFAULT_CONTRACT_NAME, name.as_str(), "expected the registered attribute name to be the contract name");
                        assert_eq!(
                            DEFAULT_SCOPE_ID.to_string(),
                            from_binary::<String>(&value)
                                .expect("unable to deserialize value from result"),
                            "expected the registered value to the scope uuid",
                        );
                        assert_eq!(AttributeValueType::String, value_type, "expected the value type to be stored as a string");
                    },
                    ProvenanceMsgParams::Marker(MarkerMsgParams::FinalizeMarker { denom }) => {
                        assert_eq!(DEFAULT_MARKER_DENOM, denom.as_str());
                    },
                    ProvenanceMsgParams::Marker(MarkerMsgParams::ActivateMarker { denom }) => {
                        assert_eq!(DEFAULT_MARKER_DENOM, denom.as_str());
                    },
                    _ => panic!("unexpected provenance message type contaiend in result"),
                }
            },
            CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                assert_eq!(1, amount.len(), "expected only one coin to be added to the fee transfer");
                let coin = amount.first().unwrap();
                match to_address.as_str() {
                    DEFAULT_FEE_COLLECTION_ADDRESS => {
                        assert_eq!(75, coin.amount.u128(), "expected the fee charged to be equal to 75, because the onboarding cost is 100 and the fee percent is 75%");
                        assert_eq!(DEFAULT_ONBOARDING_DENOM, coin.denom.as_str(), "expected the fee's denomination to equate to the contract's specified denomination");
                    },
                    DEFAULT_INFO_NAME => {
                        assert_eq!(50, coin.amount.u128(), "expected the overage amount to be refunded to the sender");
                        assert_eq!(DEFAULT_ONBOARDING_DENOM, coin.denom.as_str(), "expected the refund's denomination to equate to the contract's specified denomination");
                    },
                    _ => panic!("unexpected address for bank message send"),
                }
            },
            _ => panic!("unexpected response message type"),
        });
    }

    #[test]
    fn test_register_invalid_fund_denom() {
        let mut deps = mock_dependencies(&[]);
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
        let failure = execute(
            deps.as_mut(),
            mock_env(),
            mock_info(DEFAULT_INFO_NAME, &[coin(100, "nothash".to_string())]),
            default_register_payable(),
        )
        .unwrap_err();
        match failure {
            ContractError::InvalidFundsProvided {
                valid_denom,
                invalid_denoms,
            } => {
                assert_eq!(
                    DEFAULT_ONBOARDING_DENOM, valid_denom,
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
        let failure = execute(
            deps.as_mut(),
            mock_env(),
            mock_info(DEFAULT_INFO_NAME, &[]),
            default_register_payable(),
        )
        .unwrap_err();
        match failure {
            ContractError::NoFundsProvided { valid_denom } => {
                assert_eq!(
                    DEFAULT_ONBOARDING_DENOM, valid_denom,
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
        let failure = execute(
            deps.as_mut(),
            mock_env(),
            mock_info(
                DEFAULT_INFO_NAME,
                &[coin(99, DEFAULT_ONBOARDING_DENOM.to_string())],
            ),
            default_register_payable(),
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

    #[test]
    fn test_query_payable_after_register() {
        let mut deps = mock_dependencies(&[]);
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
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
                marker_denom: DEFAULT_MARKER_DENOM.to_string(),
            },
        )
        .unwrap();
        let payable_meta = from_binary::<PayableMeta>(&payable_binary).unwrap();
        assert_eq!(
            DEFAULT_MARKER_ADDRESS,
            payable_meta.marker_address.as_str(),
            "expected the default marker address to be returned"
        );
        assert_eq!(
            DEFAULT_MARKER_DENOM,
            payable_meta.marker_denom.as_str(),
            "expected the default marker denom to be returned"
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
                info: mock_info(DEFAULT_INFO_NAME, &[]),
                contract_name: DEFAULT_CONTRACT_NAME.into(),
                onboarding_cost: DEFAULT_ONBOARDING_COST.into(),
                onboarding_denom: DEFAULT_ONBOARDING_DENOM.into(),
                fee_collection_address: DEFAULT_FEE_COLLECTION_ADDRESS.into(),
                fee_percent: Decimal::percent(DEFAULT_FEE_PERCENT),
                oracle_address: DEFAULT_ORACLE_ADDRESS.into(),
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

    fn default_register_payable() -> ExecuteMsg {
        ExecuteMsg::RegisterPayableMarker {
            marker_address: DEFAULT_MARKER_ADDRESS.into(),
            marker_denom: DEFAULT_MARKER_DENOM.into(),
            scope_id: DEFAULT_SCOPE_ID.into(),
            payable_denom: DEFAULT_PAYABLE_DENOM.into(),
            payable_total: Uint128::new(DEFAULT_PAYABLE_TOTAL),
        }
    }
}
