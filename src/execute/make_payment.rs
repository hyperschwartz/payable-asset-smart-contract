use crate::core::error::ContractError;
use crate::core::state::config_read_v2;
use crate::query::query_payable_by_uuid::query_payable_attribute_by_uuid;
use crate::util::constants::{
    ORACLE_ADDRESS_KEY, PAYABLE_TYPE_KEY, PAYABLE_UUID_KEY, PAYEE_KEY, PAYER_KEY,
    PAYMENT_AMOUNT_KEY, PAYMENT_MADE_KEY, TOTAL_REMAINING_KEY,
};
use crate::util::provenance_util::{ProvenanceUtil, ProvenanceUtilImpl};
use cosmwasm_std::{coin, BankMsg, CosmosMsg, DepsMut, MessageInfo, Response};
use provwasm_std::{ProvenanceMsg, ProvenanceQuery};

/// Contains all relevant fields required in order to make a payment for a payable.
pub struct MakePaymentV1 {
    pub payable_uuid: String,
}

/// Parent function path for the contract to register a payable.  Ensures that the ProvenanceUtilImpl
/// is the implementation used for this functionality outside of tests.
pub fn make_payment(
    deps: DepsMut<ProvenanceQuery>,
    info: MessageInfo,
    make_payment: MakePaymentV1,
) -> Result<Response<ProvenanceMsg>, ContractError> {
    make_payment_with_util(deps, &ProvenanceUtilImpl, info, make_payment)
}

/// Makes a payment on a registered payable with the following steps:
/// - Verifies that the oracle has approved for the payable.
/// - Verifies that the payable has been registered with the contract.
/// - Verifies that all funds provided are in the denomination required by the payable.
/// - Verifies that the funds provided are <= payable total owed, but > 0.
/// - Subtracts the payment amount from the total amount owed on the scope attribute.
/// - Sends the amount of funds provided to the value owner of the payable's scope.
pub fn make_payment_with_util<T: ProvenanceUtil>(
    deps: DepsMut<ProvenanceQuery>,
    provenance_util: &T,
    info: MessageInfo,
    make_payment: MakePaymentV1,
) -> Result<Response<ProvenanceMsg>, ContractError> {
    let mut scope_attribute =
        match query_payable_attribute_by_uuid(&deps.as_ref(), &make_payment.payable_uuid) {
            Ok(attr) => {
                if !attr.oracle_approved {
                    return Err(ContractError::NotReadyForPayment {
                        payable_uuid: attr.payable_uuid,
                        not_ready_reason: "Payable missing oracle approval".into(),
                    });
                }
                attr
            }
            Err(_) => {
                return Err(ContractError::PayableNotFound {
                    payable_uuid: make_payment.payable_uuid,
                });
            }
        };
    let invalid_funds = info
        .funds
        .iter()
        .filter_map(|coin| {
            if coin.denom != scope_attribute.payable_denom {
                Some(coin.denom.clone())
            } else {
                None
            }
        })
        .collect::<Vec<String>>();
    if !invalid_funds.is_empty() {
        return Err(ContractError::InvalidFundsProvided {
            valid_denom: scope_attribute.payable_denom,
            invalid_denoms: invalid_funds,
        });
    }
    // Now that all funds are verified equivalent to our payment denomination, sum all amounts to
    // derive the total provided
    let payment_amount = info
        .funds
        .into_iter()
        .fold(0u128, |acc, coin| acc + coin.amount.u128());
    // u128 values can never be negative.  Invalid coin in funds would be rejected outright before the
    // function executes.
    if payment_amount == 0 {
        return Err(ContractError::NoFundsProvided {
            valid_denom: scope_attribute.payable_denom,
        });
    }
    if payment_amount > scope_attribute.payable_remaining_owed.u128() {
        return Err(ContractError::PaymentTooLarge {
            total_owed: scope_attribute.payable_remaining_owed.u128(),
            amount_provided: payment_amount,
        });
    }
    let scope = provenance_util.get_scope_by_id(&deps.querier, &scope_attribute.scope_id)?;
    let payee = scope.value_owner_address;
    let payment_message = CosmosMsg::Bank(BankMsg::Send {
        to_address: payee.to_string(),
        amount: vec![coin(payment_amount, &scope_attribute.payable_denom)],
    });
    // Subtract payment amount from tracked total
    scope_attribute.payable_remaining_owed =
        (scope_attribute.payable_remaining_owed.u128() - payment_amount).into();
    // Load state to derive payable type and contract name
    let state = config_read_v2(deps.storage).load()?;
    let upsert_attribute_msgs =
        provenance_util.upsert_attribute_to_scope(&scope_attribute, &state.contract_name)?;
    Ok(Response::new()
        .add_message(payment_message)
        .add_messages(upsert_attribute_msgs.to_vec())
        .add_attribute(PAYMENT_MADE_KEY, &scope_attribute.payable_uuid)
        .add_attribute(PAYABLE_TYPE_KEY, &scope_attribute.payable_type)
        .add_attribute(PAYABLE_UUID_KEY, &scope_attribute.payable_uuid)
        .add_attribute(ORACLE_ADDRESS_KEY, &scope_attribute.oracle_address)
        .add_attribute(PAYMENT_AMOUNT_KEY, payment_amount.to_string())
        .add_attribute(TOTAL_REMAINING_KEY, scope_attribute.payable_remaining_owed)
        .add_attribute(PAYER_KEY, &info.sender.to_string())
        .add_attribute(PAYEE_KEY, payee.as_str()))
}

#[cfg(test)]
mod tests {
    use crate::contract::query;
    use crate::core::error::ContractError;
    use crate::core::msg::QueryMsg;
    use crate::core::state::PayableScopeAttribute;
    use crate::testutil::make_payment_helpers::{test_make_payment, TestMakePayment};
    use crate::testutil::oracle_approval_helpers::{test_oracle_approval, TestOracleApproval};
    use crate::testutil::register_payable_helpers::{test_register_payable, TestRegisterPayable};
    use crate::testutil::test_utilities::{
        setup_test_suite, single_attribute_for_key, InstArgs, DEFAULT_CONTRACT_NAME,
        DEFAULT_INFO_NAME, DEFAULT_ORACLE_ADDRESS, DEFAULT_PAYABLE_DENOM, DEFAULT_PAYABLE_TOTAL,
        DEFAULT_PAYABLE_TYPE, DEFAULT_PAYABLE_UUID, DEFAULT_SCOPE_ID,
    };
    use crate::util::constants::{
        ORACLE_ADDRESS_KEY, PAYABLE_TYPE_KEY, PAYABLE_UUID_KEY, PAYEE_KEY, PAYER_KEY,
        PAYMENT_AMOUNT_KEY, PAYMENT_MADE_KEY, TOTAL_REMAINING_KEY,
    };
    use cosmwasm_std::testing::{mock_env, mock_info};
    use cosmwasm_std::{from_binary, BankMsg, CosmosMsg};
    use provwasm_mocks::mock_dependencies;
    use provwasm_std::{
        AttributeMsgParams, AttributeValueType, ProvenanceMsg, ProvenanceMsgParams,
    };

    #[test]
    fn test_execute_make_payment_paid_in_full() {
        let mut deps = mock_dependencies(&[]);
        let provenance_util = setup_test_suite(&mut deps, InstArgs::default());
        // Register the default payable for payment
        test_register_payable(&mut deps, &provenance_util, TestRegisterPayable::default()).unwrap();
        // Mark the oracle as approved
        test_oracle_approval(&mut deps, &provenance_util, TestOracleApproval::default()).unwrap();
        provenance_util.bind_captured_attribute(&mut deps);
        let payment_response = test_make_payment(
            &mut deps,
            &provenance_util,
            TestMakePayment::default_with_sender("payer-guy"),
        )
        .unwrap();
        provenance_util.bind_captured_attribute(&mut deps);
        assert_eq!(
            8,
            payment_response.attributes.len(),
            "expected all attributes to be added to the response"
        );
        assert_eq!(
            DEFAULT_PAYABLE_UUID,
            single_attribute_for_key(&payment_response, PAYMENT_MADE_KEY),
            "expected the payment made key to be added to the response",
        );
        assert_eq!(
            DEFAULT_PAYABLE_TYPE,
            single_attribute_for_key(&payment_response, PAYABLE_TYPE_KEY),
            "expected the payable type key to be added to the response",
        );
        assert_eq!(
            DEFAULT_PAYABLE_UUID,
            single_attribute_for_key(&payment_response, PAYABLE_UUID_KEY),
            "expected the payable uuid key to be added to the response",
        );
        assert_eq!(
            DEFAULT_ORACLE_ADDRESS,
            single_attribute_for_key(&payment_response, ORACLE_ADDRESS_KEY),
            "expected the oracle address key to be added to the response",
        );
        assert_eq!(
            DEFAULT_PAYABLE_TOTAL.to_string(),
            single_attribute_for_key(&payment_response, PAYMENT_AMOUNT_KEY),
            "expected the payment amount key to be added to the response and equate to the total owed",
        );
        assert_eq!(
            "0",
            single_attribute_for_key(&payment_response, TOTAL_REMAINING_KEY),
            "expected the total remaining key to be added to the response and equate to zero because the payable was paid off",
        );
        assert_eq!(
            "payer-guy",
            single_attribute_for_key(&payment_response, PAYER_KEY),
            "expected the payer to be the value input as the sender",
        );
        assert_eq!(
            DEFAULT_INFO_NAME,
            single_attribute_for_key(&payment_response, PAYEE_KEY),
            "expected the payee to the be the default info name, as that was used to create the scope",
        );
        assert_eq!(
            3,
            payment_response.messages.len(),
            "three messages should be added: the payment to the owner of the payable, a delete attributes message, and an add attribute message",
        );
        payment_response
            .messages
            .into_iter()
            .for_each(|msg| match msg.msg {
                CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                    assert_eq!(
                        DEFAULT_INFO_NAME, to_address,
                        "the payment should be sent to the original invoice creator"
                    );
                    assert_eq!(
                        1,
                        amount.len(),
                        "only one coin should be sent in the payment"
                    );
                    let payment_coin = amount.first().unwrap();
                    assert_eq!(
                        DEFAULT_PAYABLE_TOTAL,
                        payment_coin.amount.u128(),
                        "the payment amount should be the total on the payable"
                    );
                    assert_eq!(
                        DEFAULT_PAYABLE_DENOM,
                        payment_coin.denom.as_str(),
                        "the denom of the payment should match the payable"
                    );
                }
                CosmosMsg::Custom(ProvenanceMsg { params, .. }) => match params {
                    ProvenanceMsgParams::Attribute(AttributeMsgParams::AddAttribute {
                        name,
                        value,
                        value_type,
                        ..
                    }) => {
                        assert_eq!(
                            DEFAULT_CONTRACT_NAME, name,
                            "the contract name should be the name of the added attribute",
                        );
                        assert_eq!(
                            AttributeValueType::Json,
                            value_type,
                            "the attribute type added should be of the type Json",
                        );
                        let attribute = from_binary::<PayableScopeAttribute>(&value).unwrap();
                        provenance_util.assert_attribute_matches_latest(&attribute);
                    }
                    ProvenanceMsgParams::Attribute(AttributeMsgParams::DeleteAttribute {
                        address,
                        name,
                    }) => {
                        assert_eq!(
                            DEFAULT_SCOPE_ID,
                            address.as_str(),
                            "the delete attribute should target the scope",
                        );
                        assert_eq!(
                            DEFAULT_CONTRACT_NAME, name,
                            "the delete attribute should target the contract's name",
                        );
                    }
                    _ => panic!("unexpected custom message encountered during make payment"),
                },
                _ => panic!("unexpected message sent during payment"),
            });
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
            DEFAULT_PAYABLE_TOTAL,
            scope_attribute.payable_total_owed.u128(),
            "the total owed should remain unchanged by the payment"
        );
        assert_eq!(
            0u128,
            scope_attribute.payable_remaining_owed.u128(),
            "the remaining owed should be reduced to zero after the successful payment"
        );
    }

    #[test]
    fn test_execute_make_payment_pay_less_than_all() {
        let mut deps = mock_dependencies(&[]);
        let provenance_util = setup_test_suite(&mut deps, InstArgs::default());
        // Register the default payable for payment
        test_register_payable(&mut deps, &provenance_util, TestRegisterPayable::default()).unwrap();
        // Mark the oracle as approved
        test_oracle_approval(&mut deps, &provenance_util, TestOracleApproval::default()).unwrap();
        let payment_response = test_make_payment(
            &mut deps,
            &provenance_util,
            TestMakePayment::default_full_sender(
                "payer-guy",
                DEFAULT_PAYABLE_TOTAL - 100,
                DEFAULT_PAYABLE_DENOM,
            ),
        )
        .unwrap();
        assert_eq!(
            8,
            payment_response.attributes.len(),
            "expected all attributes to be added to the response"
        );
        assert_eq!(
            DEFAULT_PAYABLE_UUID,
            single_attribute_for_key(&payment_response, PAYMENT_MADE_KEY),
            "expected the payment made key to be added to the response",
        );
        assert_eq!(
            DEFAULT_PAYABLE_TYPE,
            single_attribute_for_key(&payment_response, PAYABLE_TYPE_KEY),
            "expected the payable type key to be added to the response",
        );
        assert_eq!(
            DEFAULT_PAYABLE_UUID,
            single_attribute_for_key(&payment_response, PAYABLE_UUID_KEY),
            "expected the payable uuid key to be added to the response",
        );
        assert_eq!(
            DEFAULT_ORACLE_ADDRESS,
            single_attribute_for_key(&payment_response, ORACLE_ADDRESS_KEY),
            "expected the oracle address key to be added to the response",
        );
        assert_eq!(
            (DEFAULT_PAYABLE_TOTAL - 100).to_string(),
            single_attribute_for_key(&payment_response, PAYMENT_AMOUNT_KEY),
            "expected the payment amount key to be added to the response and equate to the total owed - 100",
        );
        assert_eq!(
            "100",
            single_attribute_for_key(&payment_response, TOTAL_REMAINING_KEY),
            "expected the total remaining key to be added to the response and equate to 100 because that was the amount unpaid",
        );
        assert_eq!(
            "payer-guy",
            single_attribute_for_key(&payment_response, PAYER_KEY),
            "expected the payer to be the value input as the sender",
        );
        assert_eq!(
            DEFAULT_INFO_NAME,
            single_attribute_for_key(&payment_response, PAYEE_KEY),
            "expected the payee to the be the default info name, as that was used to create the scope",
        );
        assert_eq!(
            3,
            payment_response.messages.len(),
            "three messages should be added: the payment to the owner of the payable, a delete attributes message, and an add attribute message"
        );
        payment_response
            .messages
            .into_iter()
            .for_each(|msg| match msg.msg {
                CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                    assert_eq!(
                        DEFAULT_INFO_NAME, to_address,
                        "the payment should be sent to the original invoice creator"
                    );
                    assert_eq!(
                        1,
                        amount.len(),
                        "only one coin should be sent in the payment"
                    );
                    let payment_coin = amount.first().unwrap();
                    assert_eq!(
                        DEFAULT_PAYABLE_TOTAL - 100,
                        payment_coin.amount.u128(),
                        "the payment amount should be 100 less than the total owed on the payable"
                    );
                    assert_eq!(
                        DEFAULT_PAYABLE_DENOM,
                        payment_coin.denom.as_str(),
                        "the denom of the payment should match the payable"
                    );
                }
                CosmosMsg::Custom(ProvenanceMsg { params, .. }) => match params {
                    ProvenanceMsgParams::Attribute(AttributeMsgParams::AddAttribute {
                        name,
                        value,
                        value_type,
                        ..
                    }) => {
                        assert_eq!(
                            DEFAULT_CONTRACT_NAME, name,
                            "the contract name should be the name of the added attribute",
                        );
                        assert_eq!(
                            AttributeValueType::Json,
                            value_type,
                            "the attribute type added should be of the type Json",
                        );
                        let attribute = from_binary::<PayableScopeAttribute>(&value).unwrap();
                        provenance_util.assert_attribute_matches_latest(&attribute);
                    }
                    ProvenanceMsgParams::Attribute(AttributeMsgParams::DeleteAttribute {
                        address,
                        name,
                    }) => {
                        assert_eq!(
                            DEFAULT_SCOPE_ID,
                            address.as_str(),
                            "the delete attribute should target the scope",
                        );
                        assert_eq!(
                            DEFAULT_CONTRACT_NAME, name,
                            "the delete attribute should target the contract's name",
                        );
                    }
                    _ => panic!("unexpected custom message encountered during make payment"),
                },
                _ => panic!("unexpected message sent during payment"),
            });
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
            DEFAULT_PAYABLE_TOTAL,
            scope_attribute.payable_total_owed.u128(),
            "the total owed should remain unchanged by the payment"
        );
        assert_eq!(
            100u128,
            scope_attribute.payable_remaining_owed.u128(),
            "the remaining owed should be reduced to 100 after the successful payment"
        );
        // Pay subsequently to watch the values be reduced
        test_make_payment(
            &mut deps,
            &provenance_util,
            TestMakePayment::default_full_sender("payer-guy", 100, DEFAULT_PAYABLE_DENOM),
        )
        .unwrap();
        let subsequent_payable_binary = query(
            deps.as_ref(),
            mock_env(),
            QueryMsg::QueryPayableByUuid {
                payable_uuid: DEFAULT_PAYABLE_UUID.to_string(),
            },
        )
        .unwrap();
        let paid_scope_attribute =
            from_binary::<PayableScopeAttribute>(&subsequent_payable_binary).unwrap();
        assert_eq!(
            DEFAULT_PAYABLE_TOTAL,
            paid_scope_attribute.payable_total_owed.u128(),
            "the total owed should remain unchanged by the subsequent payment"
        );
        assert_eq!(
            0u128,
            paid_scope_attribute.payable_remaining_owed.u128(),
            "the remaining owed should now be reduced to zero after the subsequent payment"
        );
    }

    #[test]
    fn test_execute_make_payment_missing_payable_uuid() {
        let mut deps = mock_dependencies(&[]);
        let provenance_util = setup_test_suite(&mut deps, InstArgs::default());
        // No need to do anything upfront because we're going to target a non-existent payable
        let failure =
            test_make_payment(&mut deps, &provenance_util, TestMakePayment::default()).unwrap_err();
        match failure {
            ContractError::PayableNotFound { payable_uuid } => {
                assert_eq!(
                    DEFAULT_PAYABLE_UUID,
                    payable_uuid.as_str(),
                    "the output message should reflect the invalid input uuid"
                );
            }
            _ => panic!("unexpected error when invalid payable uuid provided"),
        };
    }

    #[test]
    fn test_execute_make_payment_invalid_coin_provided() {
        let mut deps = mock_dependencies(&[]);
        let provenance_util = setup_test_suite(&mut deps, InstArgs::default());
        // Register the default payable
        test_register_payable(&mut deps, &provenance_util, TestRegisterPayable::default()).unwrap();
        // Mark the oracle as approved
        test_oracle_approval(&mut deps, &provenance_util, TestOracleApproval::default()).unwrap();
        // Wrong coin
        let failure = test_make_payment(
            &mut deps,
            &provenance_util,
            TestMakePayment::default_with_denom("fakecoin"),
        )
        .unwrap_err();
        match failure {
            ContractError::InvalidFundsProvided {
                valid_denom,
                invalid_denoms,
            } => {
                assert_eq!(
                    DEFAULT_PAYABLE_DENOM,
                    valid_denom.as_str(),
                    "the default payable denom should be returned as the valid type"
                );
                assert_eq!(
                    1,
                    invalid_denoms.len(),
                    "one invalid denomination was provided, and it should be returned"
                );
                let invalid_denom = invalid_denoms.first().unwrap();
                assert_eq!(
                    "fakecoin",
                    invalid_denom.as_str(),
                    "the invalid denomination provided should be returned in the error"
                );
            }
            _ => panic!("unexpected error occurred when invalid coin was provided"),
        }
    }

    #[test]
    fn test_execute_make_payment_no_funds_provided() {
        let mut deps = mock_dependencies(&[]);
        let provenance_util = setup_test_suite(&mut deps, InstArgs::default());
        // Register the default payable for payment
        test_register_payable(&mut deps, &provenance_util, TestRegisterPayable::default()).unwrap();
        // Mark the oracle as approved
        test_oracle_approval(&mut deps, &provenance_util, TestOracleApproval::default()).unwrap();
        let failure = test_make_payment(
            &mut deps,
            &provenance_util,
            TestMakePayment {
                info: mock_info("payer-guy", &[]),
                ..Default::default()
            },
        )
        .unwrap_err();
        match failure {
            ContractError::NoFundsProvided { valid_denom } => {
                assert_eq!(
                    DEFAULT_PAYABLE_DENOM,
                    valid_denom.as_str(),
                    "the correct denomination should be reflected when no funds are provided"
                );
            }
            _ => panic!("unexpected error received when no funds provided"),
        };
    }

    #[test]
    fn test_execute_make_payment_zero_coin_provided() {
        let mut deps = mock_dependencies(&[]);
        let provenance_util = setup_test_suite(&mut deps, InstArgs::default());
        // Register the default payable for payment
        test_register_payable(&mut deps, &provenance_util, TestRegisterPayable::default()).unwrap();
        // Mark the oracle as approved
        test_oracle_approval(&mut deps, &provenance_util, TestOracleApproval::default()).unwrap();
        let failure = test_make_payment(
            &mut deps,
            &provenance_util,
            TestMakePayment::default_with_amount(0),
        )
        .unwrap_err();
        match failure {
            ContractError::NoFundsProvided { valid_denom } => {
                assert_eq!(
                    DEFAULT_PAYABLE_DENOM,
                    valid_denom.as_str(),
                    "the correct denomination should be reflected when no funds are provided"
                );
            }
            _ => panic!("unexpected error received when zero funds provided"),
        };
    }

    #[test]
    fn test_execute_make_payment_too_many_funds_provided() {
        let mut deps = mock_dependencies(&[]);
        let provenance_util = setup_test_suite(&mut deps, InstArgs::default());
        // Register the default payable for payment
        test_register_payable(&mut deps, &provenance_util, TestRegisterPayable::default()).unwrap();
        // Mark the oracle as approved
        test_oracle_approval(&mut deps, &provenance_util, TestOracleApproval::default()).unwrap();
        let failure = test_make_payment(
            &mut deps,
            &provenance_util,
            TestMakePayment::default_with_amount(DEFAULT_PAYABLE_TOTAL + 1),
        )
        .unwrap_err();
        match failure {
            ContractError::PaymentTooLarge {
                total_owed,
                amount_provided,
            } => {
                assert_eq!(
                    DEFAULT_PAYABLE_TOTAL, total_owed,
                    "the actual total owed should be included in the error"
                );
                assert_eq!(
                    DEFAULT_PAYABLE_TOTAL + 1,
                    amount_provided,
                    "the too-large amount provided should be included in the error"
                );
            }
            _ => panic!("unexpected error received when too many funds provided"),
        };
    }
}
