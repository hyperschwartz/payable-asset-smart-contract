use crate::core::error::ContractError;
use crate::core::state::{config, payable_meta_storage, PayableMeta, State};
use crate::util::constants::{
    ORACLE_FUNDS_KEPT, PAYABLE_REGISTERED_KEY, PAYABLE_TYPE_KEY, PAYABLE_UUID_KEY,
    REFUND_AMOUNT_KEY, REGISTERED_DENOM_KEY, SCOPE_ID_KEY, TOTAL_OWED_KEY,
};
use crate::util::provenance_utils::get_scope_by_id;
use cosmwasm_std::{coin, Attribute, BankMsg, CosmosMsg, DepsMut, MessageInfo, Response, Uint128};
use provwasm_std::{ProvenanceMsg, ProvenanceQuery};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::ops::Mul;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct RegisterPayableV1 {
    pub payable_type: String,
    pub payable_uuid: String,
    pub scope_id: String,
    pub payable_denom: String,
    pub payable_total: Uint128,
}

pub fn register_payable(
    deps: DepsMut<ProvenanceQuery>,
    info: MessageInfo,
    register: RegisterPayableV1,
) -> Result<Response<ProvenanceMsg>, ContractError> {
    let state = config(deps.storage).load()?;
    if state.payable_type != register.payable_type {
        return Err(ContractError::InvalidPayable {
            payable_uuid: register.payable_uuid,
            invalid_reason: format!(
                "this contract accepts payables of type [{}], but received type [{}]",
                state.payable_type, register.payable_type
            ),
        });
    }
    let mut messages: Vec<CosmosMsg<ProvenanceMsg>> = vec![];
    let mut attributes: Vec<Attribute> = vec![];
    let fee_charge_response = validate_fee_params_get_messages(&info, &state)?;
    // TODO: Tag the payable uuid on the scope as an attribute
    if let Some(fee_message) = fee_charge_response.fee_charge_message {
        messages.push(fee_message);
        attributes.push(Attribute::new(
            ORACLE_FUNDS_KEPT,
            format!(
                "{}/{}",
                fee_charge_response.oracle_fee_amount_kept, state.onboarding_denom
            ),
        ));
    }
    if let Some(refund_message) = fee_charge_response.fee_refund_message {
        messages.push(refund_message);
        attributes.push(Attribute::new(
            REFUND_AMOUNT_KEY,
            format!(
                "{}/{}",
                fee_charge_response.refund_amount, state.onboarding_denom
            ),
        ));
    }
    // If the sender's address is not listed as an owner address on the target scope for the payable,
    // then they are not authorized to register this payable.
    // Skip this step locally - creating a scope is an unnecessary piece of testing this
    if !state.is_local
        && get_scope_by_id(&deps.querier, &register.scope_id)?
            .owners
            .into_iter()
            .filter(|owner| owner.address == info.sender)
            .count()
            == 0
    {
        return Err(ContractError::Unauthorized);
    }
    // Ensure that this payable registration can be picked up by event key
    attributes.push(Attribute::new(
        PAYABLE_REGISTERED_KEY,
        &register.payable_uuid,
    ));
    attributes.push(Attribute::new(PAYABLE_TYPE_KEY, &register.payable_type));
    attributes.push(Attribute::new(PAYABLE_UUID_KEY, &register.payable_uuid));
    attributes.push(Attribute::new(
        TOTAL_OWED_KEY,
        &register.payable_total.to_string(),
    ));
    attributes.push(Attribute::new(
        REGISTERED_DENOM_KEY,
        &register.payable_denom,
    ));
    attributes.push(Attribute::new(SCOPE_ID_KEY, &register.scope_id));
    let payable_meta = PayableMeta {
        payable_uuid: register.payable_uuid,
        scope_id: register.scope_id,
        payable_denom: register.payable_denom,
        payable_total_owed: register.payable_total,
        payable_remaining_owed: register.payable_total,
        oracle_approved: false,
    };
    let mut meta_storage = payable_meta_storage(deps.storage);
    meta_storage.save(payable_meta.payable_uuid.as_bytes(), &payable_meta)?;
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
        oracle_fee_amount_kept: (onboarding_cost - fee_collected_amount).u128(),
    })
}

#[cfg(test)]
mod tests {
    use crate::contract::execute;
    use crate::core::error::ContractError;
    use crate::core::error::ContractError::Std;
    use crate::core::msg::ExecuteMsg;
    use crate::testutil::test_utilities::{
        default_register_payable, get_duped_scope, test_instantiate, InstArgs,
        DEFAULT_FEE_COLLECTION_ADDRESS, DEFAULT_INFO_NAME, DEFAULT_ONBOARDING_DENOM,
        DEFAULT_PAYABLE_DENOM, DEFAULT_PAYABLE_TOTAL, DEFAULT_PAYABLE_TYPE, DEFAULT_PAYABLE_UUID,
        DEFAULT_SCOPE_ID,
    };
    use crate::util::constants::{
        ORACLE_FUNDS_KEPT, PAYABLE_REGISTERED_KEY, PAYABLE_TYPE_KEY, PAYABLE_UUID_KEY,
        REFUND_AMOUNT_KEY, REGISTERED_DENOM_KEY, SCOPE_ID_KEY, TOTAL_OWED_KEY,
    };
    use cosmwasm_std::testing::{mock_env, mock_info};
    use cosmwasm_std::StdError::GenericErr;
    use cosmwasm_std::{coin, BankMsg, CosmosMsg, Uint128};
    use provwasm_mocks::mock_dependencies;

    #[test]
    fn test_register_valid_no_refund() {
        let mut deps = mock_dependencies(&[]);
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
        deps.querier
            .with_scope(get_duped_scope(DEFAULT_SCOPE_ID, DEFAULT_INFO_NAME));
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
            1,
            response.messages.len(),
            "one message expected during registration: a fee charge",
        );
        response.messages.into_iter().for_each(|msg| match msg.msg {
            CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                assert_eq!(DEFAULT_FEE_COLLECTION_ADDRESS, to_address, "expected the fee send to go the default fee collection address");
                assert_eq!(1, amount.len(), "expected only one coin to be added to the fee transfer");
                let coin = amount.first().unwrap();
                assert_eq!(75, coin.amount.u128(), "expected the fee charged to be equal to 75, because the onboarding cost is 100 and the fee percent is 75%");
                assert_eq!(DEFAULT_ONBOARDING_DENOM, coin.denom.as_str(), "expected the fee's denomination to equate to the contract's specified denomination");
            },
            _ => panic!("unexpected response message type"),
        });
        assert_eq!(
            7,
            response.attributes.len(),
            "expected all registration attributes to be recorded"
        );
        assert_eq!(
            DEFAULT_PAYABLE_UUID,
            response
                .attributes
                .iter()
                .find(|attr| attr.key.as_str() == PAYABLE_REGISTERED_KEY)
                .unwrap()
                .value
                .as_str(),
            "the PAYABLE_REGISTERED_KEY should be present and equal to the payable uuid",
        );
        assert_eq!(
            DEFAULT_PAYABLE_TYPE,
            response
                .attributes
                .iter()
                .find(|attr| attr.key.as_str() == PAYABLE_TYPE_KEY)
                .unwrap()
                .value
                .as_str(),
            "the PAYABLE_TYPE_KEY should contain the contract's payable type",
        );
        assert_eq!(
            DEFAULT_PAYABLE_UUID,
            response
                .attributes
                .iter()
                .find(|attr| attr.key.as_str() == PAYABLE_UUID_KEY)
                .unwrap()
                .value
                .as_str(),
            "the PAYABLE_UUID_KEY value should equate to the payable uuid",
        );
        assert_eq!(
            DEFAULT_SCOPE_ID,
            response
                .attributes
                .iter()
                .find(|attr| attr.key.as_str() == SCOPE_ID_KEY)
                .unwrap()
                .value
                .as_str(),
            "the SCOPE_ID_KEY should equate to the input scope id",
        );
        assert_eq!(
            DEFAULT_PAYABLE_TOTAL.to_string(),
            response
                .attributes
                .iter()
                .find(|attr| attr.key.as_str() == TOTAL_OWED_KEY)
                .unwrap()
                .value
                .as_str(),
            "the TOTAL_OWED_KEY value should equate to the default total owed amount",
        );
        assert_eq!(
            DEFAULT_PAYABLE_DENOM,
            response
                .attributes
                .iter()
                .find(|attr| attr.key.as_str() == REGISTERED_DENOM_KEY)
                .unwrap()
                .value
                .as_str(),
            "the REGISTERED_DENOM_KEY value should equate to the denomination used for the payable",
        );
        assert_eq!(
            "25/nhash",
            response.attributes.iter().find(|attr| attr.key.as_str() == ORACLE_FUNDS_KEPT).unwrap().value.as_str(),
            "the oracle funds kept should equal to total amount sent (100) - total amount sent * fee percent (75%)"
        );
    }

    #[test]
    fn test_register_valid_with_refund() {
        let mut deps = mock_dependencies(&[]);
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
        deps.querier
            .with_scope(get_duped_scope(DEFAULT_SCOPE_ID, DEFAULT_INFO_NAME));
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
            2,
            response.messages.len(),
            "two messages expected during registration: a fee charge and a fee refund",
        );
        response.messages.into_iter().for_each(|msg| match msg.msg {
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
        assert_eq!(
            8,
            response.attributes.len(),
            "expected all registration attributes to be recorded"
        );
        assert_eq!(
            DEFAULT_PAYABLE_UUID,
            response
                .attributes
                .iter()
                .find(|attr| attr.key.as_str() == PAYABLE_REGISTERED_KEY)
                .unwrap()
                .value
                .as_str(),
            "the PAYABLE_REGISTERED_KEY should be present and equal to the payable uuid",
        );
        assert_eq!(
            DEFAULT_PAYABLE_TYPE,
            response
                .attributes
                .iter()
                .find(|attr| attr.key.as_str() == PAYABLE_TYPE_KEY)
                .unwrap()
                .value
                .as_str(),
            "the PAYABLE_TYPE_KEY should contain the contract's payable type",
        );
        assert_eq!(
            DEFAULT_PAYABLE_UUID,
            response
                .attributes
                .iter()
                .find(|attr| attr.key.as_str() == PAYABLE_UUID_KEY)
                .unwrap()
                .value
                .as_str(),
            "the PAYABLE_UUID_KEY value should equate to the payable uuid",
        );
        assert_eq!(
            DEFAULT_SCOPE_ID,
            response
                .attributes
                .iter()
                .find(|attr| attr.key.as_str() == SCOPE_ID_KEY)
                .unwrap()
                .value
                .as_str(),
            "the SCOPE_ID_KEY should equate to the input scope id",
        );
        assert_eq!(
            DEFAULT_PAYABLE_TOTAL.to_string(),
            response
                .attributes
                .iter()
                .find(|attr| attr.key.as_str() == TOTAL_OWED_KEY)
                .unwrap()
                .value
                .as_str(),
            "the TOTAL_OWED_KEY value should equate to the default total owed amount",
        );
        assert_eq!(
            DEFAULT_PAYABLE_DENOM,
            response
                .attributes
                .iter()
                .find(|attr| attr.key.as_str() == REGISTERED_DENOM_KEY)
                .unwrap()
                .value
                .as_str(),
            "the REGISTERED_DENOM_KEY value should equate to the denomination used for the payable",
        );
        assert_eq!(
            "25/nhash",
            response.attributes.iter().find(|attr| attr.key.as_str() == ORACLE_FUNDS_KEPT).unwrap().value.as_str(),
            "the oracle funds kept should equal to total amount sent (100) - total amount sent * fee percent (75%)"
        );
        assert_eq!(
            "50/nhash",
            response.attributes.iter().find(|attr| attr.key.as_str() == REFUND_AMOUNT_KEY).unwrap().value.as_str(),
            "the refund amount should equal the amount provided over the onboarding cost (150 - 100)",
        );
    }

    #[test]
    fn test_register_invalid_payable_type() {
        let mut deps = mock_dependencies(&[]);
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
        let failure = execute(
            deps.as_mut(),
            mock_env(),
            mock_info(DEFAULT_INFO_NAME, &[coin(100, DEFAULT_ONBOARDING_DENOM)]),
            ExecuteMsg::RegisterPayable {
                payable_type: "wrong-payable-type".into(),
                payable_uuid: DEFAULT_PAYABLE_UUID.into(),
                scope_id: DEFAULT_SCOPE_ID.into(),
                payable_denom: DEFAULT_PAYABLE_DENOM.into(),
                payable_total: Uint128::new(DEFAULT_PAYABLE_TOTAL),
            },
        )
        .unwrap_err();
        match failure {
            ContractError::InvalidPayable {
                payable_uuid,
                invalid_reason,
            } => {
                assert_eq!(
                    DEFAULT_PAYABLE_UUID,
                    payable_uuid.as_str(),
                    "expected the attempted payable uuid to be input"
                );
                assert_eq!(
                    "this contract accepts payables of type [invoice], but received type [wrong-payable-type]",
                    invalid_reason,
                    "expected the correct message to be added to the message",
                );
            }
            _ => panic!("unexpected contract error encountered"),
        };
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
    fn test_register_scope_not_found() {
        let mut deps = mock_dependencies(&[]);
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
        // Skip registering a fake scope, causing the contract to fail to find one
        let failure = execute(
            deps.as_mut(),
            mock_env(),
            mock_info(
                DEFAULT_INFO_NAME,
                &[coin(100, DEFAULT_ONBOARDING_DENOM.to_string())],
            ),
            default_register_payable(),
        )
        .unwrap_err();
        match failure {
            Std(GenericErr { msg, .. }) => {
                assert!(msg
                    .contains("Querier system error: Cannot parse request: metadata not found in"))
            }
            _ => panic!("unexpected error received when the target scope was missing"),
        }
    }

    #[test]
    fn test_register_invalid_sender() {
        let mut deps = mock_dependencies(&[]);
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
        // Register a scope with a different owner than the sender to simulate the situation
        deps.querier
            .with_scope(get_duped_scope(DEFAULT_SCOPE_ID, "another-guy"));
        let _failure = execute(
            deps.as_mut(),
            mock_env(),
            mock_info(
                DEFAULT_INFO_NAME,
                &[coin(100, DEFAULT_ONBOARDING_DENOM.to_string())],
            ),
            default_register_payable(),
        )
        .unwrap_err();
        assert!(
            matches!(ContractError::Unauthorized, _failure),
            "the error should show that the sender is unauthorized to make this request"
        );
    }
}
