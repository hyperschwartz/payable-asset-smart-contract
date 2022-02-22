use crate::core::error::ContractError;
use crate::core::state::{config_read_v2, payable_meta_storage_v2, PayableMetaV2, PayableScopeAttribute, StateV2};
use crate::util::constants::{ORACLE_ADDRESS_KEY, ORACLE_FUNDS_KEPT, PAYABLE_REGISTERED_KEY, PAYABLE_TYPE_KEY, PAYABLE_UUID_KEY, REFUND_AMOUNT_KEY, REGISTERED_DENOM_KEY, SCOPE_ID_KEY, TOTAL_OWED_KEY};
use crate::util::provenance_util::{ProvenanceUtil, ProvenanceUtilImpl};
use cosmwasm_std::{coin, Attribute, BankMsg, CosmosMsg, DepsMut, MessageInfo, Response, Uint128, Addr};
use provwasm_std::{ProvenanceMsg, ProvenanceQuery};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::ops::Mul;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct RegisterPayableV2 {
    pub payable_type: String,
    pub payable_uuid: String,
    pub scope_id: String,
    pub oracle_address: String,
    pub payable_denom: String,
    pub payable_total: Uint128,
}
impl RegisterPayableV2 {
    pub fn to_scope_attribute(self) -> PayableScopeAttribute {
        PayableScopeAttribute {
            payable_type: self.payable_type,
            payable_uuid: self.payable_uuid,
            scope_id: self.scope_id,
            oracle_address: Addr::unchecked(self.oracle_address),
            payable_denom: self.payable_denom,
            payable_total_owed: self.payable_total,
            payable_remaining_owed: self.payable_total,
            oracle_approved: false,
        }
    }
}

pub fn register_payable(
    deps: DepsMut<ProvenanceQuery>,
    info: MessageInfo,
    register: RegisterPayableV2,
) -> Result<Response<ProvenanceMsg>, ContractError> {
    register_payable_with_util(deps, &ProvenanceUtilImpl, info, register)
}

pub fn register_payable_with_util<T : ProvenanceUtil>(
    deps: DepsMut<ProvenanceQuery>,
    provenance_util: &T,
    info: MessageInfo,
    register: RegisterPayableV2,
) -> Result<Response<ProvenanceMsg>, ContractError> {
    let mut messages: Vec<CosmosMsg<ProvenanceMsg>> = vec![];
    let mut attributes: Vec<Attribute> = vec![];
    let state = config_read_v2(deps.storage).load()?;
    let fee_charge_response = validate_fee_params_get_messages(&info, &state)?;
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
        && provenance_util.get_scope_by_id(&deps.querier, &register.scope_id)?
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
    attributes.push(Attribute::new(ORACLE_ADDRESS_KEY, &register.oracle_address));
    attributes.push(Attribute::new(
        TOTAL_OWED_KEY,
        &register.payable_total.to_string(),
    ));
    attributes.push(Attribute::new(
        REGISTERED_DENOM_KEY,
        &register.payable_denom,
    ));
    attributes.push(Attribute::new(SCOPE_ID_KEY, &register.scope_id));
    // Tag the scope with an attribute that contains all information about its current payable
    // status
    let scope_attribute = register.to_scope_attribute();
    messages.push(provenance_util.get_add_initial_attribute_to_scope_msg(&deps.as_ref(), &scope_attribute, &state.contract_name)?);
    // Store a link between the payable's uuid and the scope id in local storage for queries
    let payable_meta = PayableMetaV2 {
        payable_uuid: scope_attribute.payable_uuid,
        scope_id: scope_attribute.scope_id,
    };
    payable_meta_storage_v2(deps.storage).save(payable_meta.payable_uuid.as_bytes(), &payable_meta)?;
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
    state: &StateV2,
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
    use crate::core::error::ContractError;
    use crate::core::error::ContractError::Std;
    use crate::testutil::test_utilities::{ get_duped_scope, single_attribute_for_key, test_instantiate, InstArgs, DEFAULT_FEE_COLLECTION_ADDRESS, DEFAULT_INFO_NAME, DEFAULT_ONBOARDING_DENOM, DEFAULT_PAYABLE_DENOM, DEFAULT_PAYABLE_TOTAL, DEFAULT_PAYABLE_TYPE, DEFAULT_PAYABLE_UUID, DEFAULT_SCOPE_ID, DEFAULT_ORACLE_ADDRESS, setup_test_suite, DEFAULT_CONTRACT_NAME};
    use crate::util::constants::{ORACLE_ADDRESS_KEY, ORACLE_FUNDS_KEPT, PAYABLE_REGISTERED_KEY, PAYABLE_TYPE_KEY, PAYABLE_UUID_KEY, REFUND_AMOUNT_KEY, REGISTERED_DENOM_KEY, SCOPE_ID_KEY, TOTAL_OWED_KEY};
    use cosmwasm_std::testing::{mock_info};
    use cosmwasm_std::StdError::GenericErr;
    use cosmwasm_std::{BankMsg, CosmosMsg, from_binary};
    use provwasm_mocks::mock_dependencies;
    use provwasm_std::{AttributeMsgParams, AttributeValueType, ProvenanceMsg, ProvenanceMsgParams};
    use crate::core::state::PayableScopeAttribute;
    use crate::testutil::mock_provenance_util::MockProvenanceUtil;
    use crate::testutil::register_payable_helpers::{test_register_payable, TestRegisterPayable};

    #[test]
    fn test_register_valid_no_refund() {
        let mut deps = mock_dependencies(&[]);
        let provenance_util = setup_test_suite(&mut deps, InstArgs::default());
        // The default message will register a payable with the exact amount required for no refund
        let response = test_register_payable(&mut deps, &provenance_util, TestRegisterPayable::default()).unwrap();
        assert_eq!(
            8,
            response.attributes.len(),
            "expected all registration attributes to be recorded"
        );
        assert_eq!(
            DEFAULT_PAYABLE_UUID,
            single_attribute_for_key(&response, PAYABLE_REGISTERED_KEY),
            "the PAYABLE_REGISTERED_KEY should be present and equal to the payable uuid",
        );
        assert_eq!(
            DEFAULT_PAYABLE_TYPE,
            single_attribute_for_key(&response, PAYABLE_TYPE_KEY),
            "the PAYABLE_TYPE_KEY should contain the contract's payable type",
        );
        assert_eq!(
            DEFAULT_PAYABLE_UUID,
            single_attribute_for_key(&response, PAYABLE_UUID_KEY),
            "the PAYABLE_UUID_KEY value should equate to the payable uuid",
        );
        assert_eq!(
            DEFAULT_ORACLE_ADDRESS,
            single_attribute_for_key(&response, ORACLE_ADDRESS_KEY),
            "the ORACLE_ADDRESS_KEY value should equate to the oracle's address",
        );
        assert_eq!(
            DEFAULT_SCOPE_ID,
            single_attribute_for_key(&response, SCOPE_ID_KEY),
            "the SCOPE_ID_KEY should equate to the input scope id",
        );
        assert_eq!(
            DEFAULT_PAYABLE_TOTAL.to_string(),
            single_attribute_for_key(&response, TOTAL_OWED_KEY),
            "the TOTAL_OWED_KEY value should equate to the default total owed amount",
        );
        assert_eq!(
            DEFAULT_PAYABLE_DENOM,
            single_attribute_for_key(&response, REGISTERED_DENOM_KEY),
            "the REGISTERED_DENOM_KEY value should equate to the denomination used for the payable",
        );
        assert_eq!(
            "25/nhash",
            single_attribute_for_key(&response, ORACLE_FUNDS_KEPT),
            "the oracle funds kept should equal to total amount sent (100) - total amount sent * fee percent (75%)"
        );
        assert_eq!(
            2,
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
            CosmosMsg::Custom(ProvenanceMsg { params, .. }) => {
                match params {
                    ProvenanceMsgParams::Attribute(AttributeMsgParams::AddAttribute {
                                                       name, value, value_type, ..
                                                   }) => {
                        assert_eq!(
                            DEFAULT_CONTRACT_NAME,
                            name,
                            "the contract name should be the name of the added attribute",
                        );
                        assert_eq!(
                            AttributeValueType::Json,
                            value_type,
                            "the attribute type added should be of the type Json",
                        );
                        let attribute = from_binary::<PayableScopeAttribute>(&value).unwrap();
                        provenance_util.assert_attribute_matches_latest(&attribute);
                    },
                    _ => panic!("unexpected provenance msg params"),
                }
            },
            _ => panic!("unexpected response message type"),
        });
    }

    #[test]
    fn test_register_valid_with_refund() {
        let mut deps = mock_dependencies(&[]);
        let provenance_util = setup_test_suite(&mut deps, InstArgs::default());
        let response = test_register_payable(
            &mut deps,
            &provenance_util,
            TestRegisterPayable::default_with_amount(150),
        ).unwrap();
        assert_eq!(
            9,
            response.attributes.len(),
            "expected all registration attributes to be recorded"
        );
        assert_eq!(
            DEFAULT_PAYABLE_UUID,
            single_attribute_for_key(&response, PAYABLE_REGISTERED_KEY),
            "the PAYABLE_REGISTERED_KEY should be present and equal to the payable uuid",
        );
        assert_eq!(
            DEFAULT_PAYABLE_TYPE,
            single_attribute_for_key(&response, PAYABLE_TYPE_KEY),
            "the PAYABLE_TYPE_KEY should contain the contract's payable type",
        );
        assert_eq!(
            DEFAULT_PAYABLE_UUID,
            single_attribute_for_key(&response, PAYABLE_UUID_KEY),
            "the PAYABLE_UUID_KEY value should equate to the payable uuid",
        );
        assert_eq!(
            DEFAULT_ORACLE_ADDRESS,
            single_attribute_for_key(&response, ORACLE_ADDRESS_KEY),
            "the ORACLE_ADDRESS_KEY value should equate to the oracle address",
        );
        assert_eq!(
            DEFAULT_SCOPE_ID,
            single_attribute_for_key(&response, SCOPE_ID_KEY),
            "the SCOPE_ID_KEY should equate to the input scope id",
        );
        assert_eq!(
            DEFAULT_PAYABLE_TOTAL.to_string(),
            single_attribute_for_key(&response, TOTAL_OWED_KEY),
            "the TOTAL_OWED_KEY value should equate to the default total owed amount",
        );
        assert_eq!(
            DEFAULT_PAYABLE_DENOM,
            single_attribute_for_key(&response, REGISTERED_DENOM_KEY),
            "the REGISTERED_DENOM_KEY value should equate to the denomination used for the payable",
        );
        assert_eq!(
            "25/nhash",
            single_attribute_for_key(&response, ORACLE_FUNDS_KEPT),
            "the oracle funds kept should equal to total amount sent (100) - total amount sent * fee percent (75%)"
        );
        assert_eq!(
            "50/nhash",
            single_attribute_for_key(&response, REFUND_AMOUNT_KEY),
            "the refund amount should equal the amount provided over the onboarding cost (150 - 100)",
        );
        assert_eq!(
            3,
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
            CosmosMsg::Custom(ProvenanceMsg { params, .. }) => {
                match params {
                    ProvenanceMsgParams::Attribute(AttributeMsgParams::AddAttribute {
                        name, value, value_type, ..
                    }) => {
                        assert_eq!(
                            DEFAULT_CONTRACT_NAME,
                            name,
                            "the contract name should be the name of the added attribute",
                        );
                        assert_eq!(
                            AttributeValueType::Json,
                            value_type,
                            "the attribute type added should be of the type Json",
                        );
                        let attribute = from_binary::<PayableScopeAttribute>(&value).unwrap();
                        provenance_util.assert_attribute_matches_latest(&attribute);
                    },
                    _ => panic!("unexpected provenance msg params"),
                }
            },
            _ => panic!("unexpected response message type"),
        });
    }

    #[test]
    fn test_register_invalid_fund_denom() {
        let mut deps = mock_dependencies(&[]);
        let provenance_util = setup_test_suite(&mut deps, InstArgs::default());
        let failure = test_register_payable(
            &mut deps,
            &provenance_util,
            TestRegisterPayable::default_with_denom("nothash"),
        ).unwrap_err();
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
        let provenance_util = setup_test_suite(&mut deps, InstArgs::default());
        let failure = test_register_payable(
            &mut deps,
            &provenance_util,
            TestRegisterPayable {
                info: mock_info(DEFAULT_INFO_NAME, &[]),
                ..Default::default()
            }
        ).unwrap_err();
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
        let provenance_util = setup_test_suite(&mut deps, InstArgs::default());
        let failure = test_register_payable(&mut deps, &provenance_util, TestRegisterPayable::default_with_amount(99)).unwrap_err();
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
        // Skip registering a fake scope, causing the contract to fail to find one. Using test_instantiate
        // instead of setup_test_suite will skip mocking a targeted scope
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
        let failure = test_register_payable(&mut deps, &MockProvenanceUtil::new(), TestRegisterPayable::default()).unwrap_err();
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
        let _failure = test_register_payable(
            &mut deps,
            &MockProvenanceUtil::new(),
            TestRegisterPayable::default()
        ).unwrap_err();
        assert!(
            matches!(ContractError::Unauthorized, _failure),
            "the error should show that the sender is unauthorized to make this request"
        );
    }
}
