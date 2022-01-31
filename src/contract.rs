use cosmwasm_std::{coin, to_binary, Attribute, BankMsg, Binary, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, Response, StdError, StdResult, Uint128, Addr};
use provwasm_std::{
    activate_marker, add_attribute, bind_name, create_marker, finalize_marker, grant_marker_access,
    AttributeValueType, MarkerType, NameBinding, ProvenanceMsg,
};
use std::ops::Mul;

use crate::error::ContractError;
use crate::helper::{to_percent, CONTRACT_MARKER_PERMISSIONS, DEFAULT_MARKER_COIN_AMOUNT};
use crate::msg::{ExecuteMsg, InitMsg, MigrateMsg, QueryMsg};
use crate::oracle_approval::OracleApprovalV1;
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
        ExecuteMsg::OracleApproval { marker_denom } => {
            oracle_approval(deps, info, OracleApprovalV1 { marker_denom })
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
    messages.append(
        &mut generate_registration_marker_messages(
            register.marker_denom.clone(),
            env.contract.address,
            register.marker_address.clone(),
            state.contract_name.clone(),
            register.scope_id.clone(),
        )?
    );
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
        oracle_approved: false,
    };
    let mut meta_storage = payable_meta_storage(deps.storage);
    meta_storage.save(register.marker_denom.as_bytes(), &payable_meta)?;
    Ok(Response::new()
        .add_messages(messages)
        .add_attributes(attributes))
}

fn generate_registration_marker_messages(
    marker_denom: String,
    contract_address: Addr,
    marker_address: Addr,
    contract_name: String,
    scope_id: String,
) -> Result<Vec<CosmosMsg<ProvenanceMsg>>, ContractError> {
    let mut messages: Vec<CosmosMsg<ProvenanceMsg>> = vec!();
    // Create a marker that owns the scope
    let marker_gen_request = create_marker(
        // amount: The amount of coin that starts in the marker
        DEFAULT_MARKER_COIN_AMOUNT,
        // denom: The denomination on the new coin. Should be "payable-type-<payable-uuid>"
        marker_denom.clone(),
        // Restrict the marker - only the contract should be able to make changes to it
        MarkerType::Restricted,
    )?;
    messages.push(marker_gen_request);
    // Grant the contract permission to manipulate the marker in any way it sees fit in order to
    // facilitate trade functionality
    let contract_marker_grant_request = grant_marker_access(
        marker_denom.clone(),
        contract_address,
        CONTRACT_MARKER_PERMISSIONS.to_vec(),
    )?;
    messages.push(contract_marker_grant_request);
    let marker_finalize_request = finalize_marker(marker_denom.clone())?;
    messages.push(marker_finalize_request);
    let marker_activate_request = activate_marker(marker_denom)?;
    messages.push(marker_activate_request);
    let marker_tag_request = add_attribute(
        // Tag the newly-created marker address with an attribute indicating its managed scope
        marker_address,
        // The contract's name should be stamped on the attribute, verifying its source
        contract_name,
        // Serialize the scope id as the value within the attribute - showing that this marker owns the scope
        to_binary(&scope_id)?,
        AttributeValueType::String,
    )?;
    messages.push(marker_tag_request);
    Ok(messages)
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

fn oracle_approval(
    deps: DepsMut,
    info: MessageInfo,
    oracle_approval: OracleApprovalV1,
) -> Result<Response<ProvenanceMsg>, ContractError> {
    let mut messages: Vec<CosmosMsg<ProvenanceMsg>> = vec![];
    // Oracle approval should not require any funds
    if !info.funds.is_empty() {
        return Err(ContractError::FundsPresent);
    }
    let state = config(deps.storage).load()?;
    // Only the designated oracle can mark an approval on a denomination
    if info.sender != state.oracle_address {
        return Err(ContractError::Unauthorized);
    }
    let mut payables_bucket = payable_meta_storage(deps.storage);
    // Ensure the target payable definition actually exists
    let mut target_payable =
        match payables_bucket.load(oracle_approval.marker_denom.clone().as_bytes()) {
            Ok(meta) => {
                if meta.oracle_approved {
                    return Err(ContractError::DuplicateApproval {
                        payable_denom: oracle_approval.marker_denom,
                    });
                }
                meta
            }
            Err(_) => {
                return Err(ContractError::PayableNotFound {
                    target_denom: oracle_approval.marker_denom,
                });
            }
        };
    let oracle_approval_message = add_attribute(
        target_payable.marker_address.clone(),
        state.contract_name,
        to_binary(&state.oracle_address.as_str())?,
        AttributeValueType::String,
    )?;
    messages.push(oracle_approval_message);
    // The oracle is paid X on each approval, where X is the remaining amount after the fee is taken
    // from the onboarding funds.
    let oracle_withdraw_amount =
        state.onboarding_cost - state.onboarding_cost.mul(state.fee_percent);
    // Only create a payment to the oracle if there were funds stored in the first place
    if oracle_withdraw_amount.u128() > 0 {
        messages.push(CosmosMsg::Bank(BankMsg::Send {
            to_address: state.oracle_address.into(),
            amount: vec![coin(oracle_withdraw_amount.u128(), state.onboarding_denom)],
        }));
    }
    target_payable.oracle_approved = true;
    payables_bucket.save(oracle_approval.marker_denom.as_bytes(), &target_payable)?;
    Ok(Response::new().add_messages(messages))
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
    // mock_env() creates this as the default contract address
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
            6,
            response.messages.len(),
            "six messages expected: create marker, one grant marker, one finalize marker, one activate marker, one add attribute, and one fee exchange",
        );
        response.messages.into_iter().for_each(|msg| match msg.msg {
            CosmosMsg::Custom(ProvenanceMsg { params, .. }) => {
                match params {
                    // Handled in order in which they appear in the contract's execution
                    ProvenanceMsgParams::Marker(MarkerMsgParams::CreateMarker { coin, marker_type }) => {
                        assert_eq!(DEFAULT_MARKER_DENOM, coin.denom.as_str());
                        assert_eq!(DEFAULT_MARKER_COIN_AMOUNT, coin.amount.u128());
                        assert_eq!(MarkerType::Restricted, marker_type);
                    },
                    ProvenanceMsgParams::Marker(MarkerMsgParams::GrantMarkerAccess { denom, address, permissions }) => {
                        assert_eq!(DEFAULT_MARKER_DENOM, denom.as_str());
                        assert_eq!(MOCK_COSMOS_CONTRACT_ADDRESS, address.as_str());
                        assert_eq!(CONTRACT_MARKER_PERMISSIONS.to_vec(), permissions);
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
            7,
            response.messages.len(),
            "seven messages expected: create marker, one grant marker, one finalize marker, one activate marker, one add attribute, one fee exchange, and one fee refund",
        );
        response.messages.into_iter().for_each(|msg| match msg.msg {
            CosmosMsg::Custom(ProvenanceMsg { params, .. }) => {
                match params {
                    // Handled in order in which they appear in the contract's execution
                    ProvenanceMsgParams::Marker(MarkerMsgParams::CreateMarker { coin, marker_type }) => {
                        assert_eq!(DEFAULT_MARKER_DENOM, coin.denom.as_str());
                        assert_eq!(DEFAULT_MARKER_COIN_AMOUNT, coin.amount.u128());
                        assert_eq!(MarkerType::Restricted, marker_type);
                    },
                    ProvenanceMsgParams::Marker(MarkerMsgParams::GrantMarkerAccess { denom, address, permissions }) => {
                        assert_eq!(DEFAULT_MARKER_DENOM, denom.as_str());
                        assert_eq!(MOCK_COSMOS_CONTRACT_ADDRESS, address.as_str());
                        assert_eq!(CONTRACT_MARKER_PERMISSIONS.to_vec(), permissions);
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
        assert_eq!(false, payable_meta.oracle_approved, "when initially created, the meta should show that the oracle has not yet approved the payable");
    }

    #[test]
    fn test_execute_oracle_approval_success() {
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
        let approval_response = execute(
            deps.as_mut(),
            mock_env(),
            mock_info(DEFAULT_ORACLE_ADDRESS, &[]),
            ExecuteMsg::OracleApproval {
                marker_denom: DEFAULT_MARKER_DENOM.into(),
            },
        )
        .unwrap();
        assert_eq!(2, approval_response.messages.len(), "expected a message for the oracle attribute stamp a message for the oracle fee withdrawal");
        approval_response.messages.into_iter().for_each(|msg| match msg.msg {
            CosmosMsg::Custom(ProvenanceMsg { params, .. }) => {
                match params {
                    ProvenanceMsgParams::Attribute(AttributeMsgParams::AddAttribute { address, name, value, value_type, }) => {
                        assert_eq!(DEFAULT_MARKER_ADDRESS, address.as_str(), "expected the attribute to be added to the marker");
                        assert_eq!(
                            DEFAULT_CONTRACT_NAME,
                            name.as_str(),
                            "the attribute name bound should be the contract name",
                        );
                        assert_eq!(
                            DEFAULT_ORACLE_ADDRESS,
                            from_binary::<String>(&value).unwrap().as_str(),
                            "the attribute value should equate to the oracle's address",
                        );
                        assert_eq!(
                            AttributeValueType::String,
                            value_type,
                            "the value type should be a string"
                        );
                    },
                    _ => panic!("unexpected provenance message occurred during oracle approval"),
                }
            },
            CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                assert_eq!(
                    DEFAULT_ORACLE_ADDRESS,
                    to_address.as_str(),
                    "the bank transfer should be to the oracle",
                );
                assert_eq!(1, amount.len(), "only one coin should be included in the transfer");
                let coin = amount.first().unwrap();
                assert_eq!(
                    DEFAULT_ONBOARDING_DENOM,
                    coin.denom,
                    "the denomination for the oracle withdrawal should be the onboarding denom",
                );
                assert_eq!(
                    25,
                    coin.amount.u128(),
                    "the oracle withdrawal amount should be 25, because the onboarding cost is 100 and the fee is 75%, leaving the remaining 25% for the oracle",
                );
            },
            _ => panic!("unexpected message occurred during oracle approval"),
        });
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
            true, payable_meta.oracle_approved,
            "the payable should be marked as oracle approved after the function executes"
        );
    }

    #[test]
    fn test_execute_oracle_approval_success_with_no_oracle_fee() {
        let mut deps = mock_dependencies(&[]);
        // Set the fee percent to 100%, ensuring that all funds are taken as a fee to the fee
        // collector, with none remaining for the oracle to withdraw
        test_instantiate(
            deps.as_mut(),
            InstArgs {
                fee_percent: Decimal::percent(100),
                ..Default::default()
            },
        )
        .unwrap();
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
        let approval_response = execute(
            deps.as_mut(),
            mock_env(),
            mock_info(DEFAULT_ORACLE_ADDRESS, &[]),
            ExecuteMsg::OracleApproval {
                marker_denom: DEFAULT_MARKER_DENOM.into(),
            },
        )
        .unwrap();
        assert_eq!(
            1,
            approval_response.messages.len(),
            "expected only a single message to be sent for the oracle attribute registration"
        );
        approval_response
            .messages
            .into_iter()
            .for_each(|msg| match msg.msg {
                CosmosMsg::Custom(ProvenanceMsg { params, .. }) => match params {
                    ProvenanceMsgParams::Attribute(AttributeMsgParams::AddAttribute {
                        address,
                        name,
                        value,
                        value_type,
                    }) => {
                        assert_eq!(
                            DEFAULT_MARKER_ADDRESS,
                            address.as_str(),
                            "expected the attribute to be added to the marker"
                        );
                        assert_eq!(
                            DEFAULT_CONTRACT_NAME,
                            name.as_str(),
                            "the attribute name bound should be the contract name",
                        );
                        assert_eq!(
                            DEFAULT_ORACLE_ADDRESS,
                            from_binary::<String>(&value).unwrap().as_str(),
                            "the attribute value should equate to the oracle's address",
                        );
                        assert_eq!(
                            AttributeValueType::String,
                            value_type,
                            "the value type should be a string"
                        );
                    }
                    _ => panic!("unexpected provenance message occurred during oracle approval"),
                },
                _ => panic!("unexpected message occurred during oracle approval"),
            });
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
            true, payable_meta.oracle_approved,
            "the payable should be marked as oracle approved after the function executes"
        );
    }

    #[test]
    fn test_execute_oracle_approval_fails_for_included_funds() {
        let mut deps = mock_dependencies(&[]);
        // Set the fee percent to 100%, ensuring that all funds are taken as a fee to the fee
        // collector, with none remaining for the oracle to withdraw
        test_instantiate(
            deps.as_mut(),
            InstArgs {
                fee_percent: Decimal::percent(100),
                ..Default::default()
            },
        )
        .unwrap();
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
        let error = execute(
            deps.as_mut(),
            mock_env(),
            // Include some hash in the request, which should cause a rejection
            mock_info(
                DEFAULT_ORACLE_ADDRESS,
                &[coin(400, DEFAULT_ONBOARDING_DENOM.to_string())],
            ),
            ExecuteMsg::OracleApproval {
                marker_denom: DEFAULT_MARKER_DENOM.into(),
            },
        )
        .unwrap_err();
        assert!(
            matches!(error, ContractError::FundsPresent),
            "The execution should be rejected because the oracle sent funds",
        );
    }

    #[test]
    fn test_execute_oracle_approval_fails_for_invalid_sender_address() {
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
        let error = execute(
            deps.as_mut(),
            mock_env(),
            // Try to call into the oracle approval as the fee collector.  Only the oracle is
            // allowed to make this call, so the execution should fail.
            mock_info(DEFAULT_FEE_COLLECTION_ADDRESS, &[]),
            ExecuteMsg::OracleApproval {
                marker_denom: DEFAULT_MARKER_DENOM.into(),
            },
        )
        .unwrap_err();
        assert!(
            matches!(error, ContractError::Unauthorized),
            "The execution should be rejected because the sender was not the oracle",
        );
    }

    #[test]
    fn test_execute_oracle_approval_fails_for_duplicate_execution() {
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
        // Closure to keep code more concise than some of these other tests I wrote...
        let mut execute_as_oracle = || {
            execute(
                deps.as_mut(),
                mock_env(),
                mock_info(DEFAULT_ORACLE_ADDRESS, &[]),
                ExecuteMsg::OracleApproval {
                    marker_denom: DEFAULT_MARKER_DENOM.into(),
                },
            )
        };
        // Execute once with good args, should be a success
        execute_as_oracle().unwrap();
        // Execute a second time, should be rejected because the oracle stamp has already been added
        let error = execute_as_oracle().unwrap_err();
        match error {
            ContractError::DuplicateApproval { payable_denom } => {
                assert_eq!(
                    DEFAULT_MARKER_DENOM,
                    payable_denom.as_str(),
                    "the error message should include the marker denomination target"
                );
            }
            _ => panic!("unexpected error occurred during execution"),
        };
    }

    #[test]
    fn test_execute_oracle_approval_fails_for_wrong_target_payable() {
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
        // Closure to keep code more concise than some of these other tests I wrote...
        let error = execute(
            deps.as_mut(),
            mock_env(),
            mock_info(DEFAULT_ORACLE_ADDRESS, &[]),
            ExecuteMsg::OracleApproval {
                marker_denom: "some-other-denom".into(),
            },
        )
        .unwrap_err();
        match error {
            ContractError::PayableNotFound { target_denom } => {
                assert_eq!(
                    "some-other-denom",
                    target_denom.as_str(),
                    "the incorrect denomination should be included in the error message"
                );
            }
            _ => panic!("unexpected error occurred during execution"),
        }
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
