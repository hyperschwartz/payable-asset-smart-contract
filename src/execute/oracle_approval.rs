use crate::core::error::ContractError;
use crate::core::state::config_read_v2;
use crate::query::query_payable_by_uuid::query_payable_attribute_by_uuid;
use crate::util::constants::{
    ORACLE_ADDRESS_KEY, ORACLE_APPROVED_KEY, PAYABLE_TYPE_KEY, PAYABLE_UUID_KEY,
};
use crate::util::provenance_util::{ProvenanceUtil, ProvenanceUtilImpl};
use cosmwasm_std::{coin, BankMsg, CosmosMsg, DepsMut, MessageInfo, Response};
use provwasm_std::{ProvenanceMsg, ProvenanceQuery};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::ops::Mul;

/// Contains all relevant fields required in order for an oracle address to mark a payable as
/// approved and ready for payment.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct OracleApprovalV1 {
    pub payable_uuid: String,
}

/// Parent function path for the contract to mark an oracle approval.  Ensures that the
/// ProvenanceUtilImpl is the implementation used for this functionality outside of tests.
pub fn oracle_approval(
    deps: DepsMut<ProvenanceQuery>,
    info: MessageInfo,
    oracle_approval: OracleApprovalV1,
) -> Result<Response<ProvenanceMsg>, ContractError> {
    oracle_approval_with_util(deps, &ProvenanceUtilImpl, info, oracle_approval)
}

/// Stamps an oracle approval on the target payable with the following steps:
/// - Verifies that no funds were send (oracle approvals are free).
/// - Ensures that the oracle has not yet approved of this transaction.
/// - Ensures that the payable targeted has been registered.
/// - Ensures that the sender address is the oracle listed on the payable's scope attribute.
/// - Sends the oracle fee to the oracle for performing its stamp.
/// - Updates the attribute on the scope to indicate that the oracle approved successfully.
pub fn oracle_approval_with_util<T: ProvenanceUtil>(
    deps: DepsMut<ProvenanceQuery>,
    provenance_util: &T,
    info: MessageInfo,
    oracle_approval: OracleApprovalV1,
) -> Result<Response<ProvenanceMsg>, ContractError> {
    let mut messages: Vec<CosmosMsg<ProvenanceMsg>> = vec![];
    // Oracle approval should not require any funds
    if !info.funds.is_empty() {
        return Err(ContractError::FundsPresent);
    }
    let mut scope_attribute =
        match query_payable_attribute_by_uuid(&deps.as_ref(), &oracle_approval.payable_uuid) {
            Ok(attr) => {
                if attr.oracle_approved {
                    return ContractError::DuplicateApproval {
                        payable_uuid: oracle_approval.payable_uuid,
                    }
                    .to_result();
                }
                attr
            }
            Err(_) => {
                return ContractError::PayableNotFound {
                    payable_uuid: oracle_approval.payable_uuid,
                }
                .to_result();
            }
        };
    // Only the designated oracle can mark an approval on a denomination
    if info.sender != scope_attribute.oracle_address {
        return Err(ContractError::Unauthorized);
    }
    let state = config_read_v2(deps.storage).load()?;
    // The oracle is paid X on each approval, where X is the remaining amount after the fee is taken
    // from the onboarding funds.
    let oracle_withdraw_amount =
        state.onboarding_cost - state.onboarding_cost.mul(state.fee_percent);
    // Only create a payment to the oracle if there were funds stored in the first place
    if oracle_withdraw_amount.u128() > 0 {
        messages.push(CosmosMsg::Bank(BankMsg::Send {
            to_address: scope_attribute.oracle_address.clone().into(),
            amount: vec![coin(oracle_withdraw_amount.u128(), state.onboarding_denom)],
        }));
    }
    scope_attribute.oracle_approved = true;
    // Add messages that will remove the current attribute and replace it with the attribute with an
    // oracle approval on it
    messages.append(
        &mut provenance_util
            .upsert_attribute_to_scope(&scope_attribute, &state.contract_name)?
            .to_vec(),
    );
    Ok(Response::new()
        .add_messages(messages)
        .add_attribute(ORACLE_APPROVED_KEY, &scope_attribute.payable_uuid)
        .add_attribute(PAYABLE_TYPE_KEY, &scope_attribute.payable_type)
        .add_attribute(PAYABLE_UUID_KEY, &scope_attribute.payable_uuid)
        .add_attribute(ORACLE_ADDRESS_KEY, scope_attribute.oracle_address.as_str()))
}

#[cfg(test)]
mod tests {
    use crate::contract::query;
    use crate::core::error::ContractError;
    use crate::core::msg::QueryMsg;
    use crate::core::state::PayableScopeAttribute;
    use crate::execute::oracle_approval::OracleApprovalV1;
    use crate::testutil::oracle_approval_helpers::{test_oracle_approval, TestOracleApproval};
    use crate::testutil::register_payable_helpers::{test_register_payable, TestRegisterPayable};
    use crate::testutil::test_utilities::{
        setup_test_suite, single_attribute_for_key, InstArgs, DEFAULT_CONTRACT_NAME,
        DEFAULT_FEE_COLLECTION_ADDRESS, DEFAULT_ONBOARDING_DENOM, DEFAULT_ORACLE_ADDRESS,
        DEFAULT_PAYABLE_TYPE, DEFAULT_PAYABLE_UUID, DEFAULT_SCOPE_ID,
    };
    use crate::util::constants::{
        ORACLE_ADDRESS_KEY, ORACLE_APPROVED_KEY, PAYABLE_TYPE_KEY, PAYABLE_UUID_KEY,
    };
    use cosmwasm_std::testing::{mock_env, mock_info};
    use cosmwasm_std::{coin, from_binary, BankMsg, CosmosMsg, Decimal};
    use provwasm_mocks::mock_dependencies;
    use provwasm_std::{
        AttributeMsgParams, AttributeValueType, ProvenanceMsg, ProvenanceMsgParams,
    };

    #[test]
    fn test_execute_oracle_approval_success() {
        let mut deps = mock_dependencies(&[]);
        let provenance_util = setup_test_suite(&mut deps, InstArgs::default());
        test_register_payable(&mut deps, &provenance_util, TestRegisterPayable::default()).unwrap();
        let approval_response =
            test_oracle_approval(&mut deps, &provenance_util, TestOracleApproval::default())
                .unwrap();
        assert_eq!(
            4,
            approval_response.attributes.len(),
            "expected all attributes to be added"
        );
        assert_eq!(
            DEFAULT_PAYABLE_UUID,
            single_attribute_for_key(&approval_response, ORACLE_APPROVED_KEY),
            "expected the oracle approved key to be added as an attribute",
        );
        assert_eq!(
            DEFAULT_PAYABLE_TYPE,
            single_attribute_for_key(&approval_response, PAYABLE_TYPE_KEY),
            "expected the payable type key to be added as an attribute",
        );
        assert_eq!(
            DEFAULT_PAYABLE_UUID,
            single_attribute_for_key(&approval_response, PAYABLE_UUID_KEY),
            "expected the payable uuid key to be added as an attribute",
        );
        assert_eq!(
            DEFAULT_ORACLE_ADDRESS,
            single_attribute_for_key(&approval_response, ORACLE_ADDRESS_KEY),
            "expected the oracle address key to be added as an attribute",
        );
        assert_eq!(
            3,
            approval_response.messages.len(),
            "expected a message for the oracle fee withdrawal and the attribute swaps"
        );
        approval_response.messages.into_iter().for_each(|msg| match msg.msg {
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
            CosmosMsg::Custom(ProvenanceMsg { params, .. }) => {
                match params {
                    ProvenanceMsgParams::Attribute(AttributeMsgParams::AddAttribute {
                                                       name,
                                                       value,
                                                       value_type,
                                                       ..
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
                            DEFAULT_CONTRACT_NAME,
                            name,
                            "the delete attribute should target the contract's name",
                        );
                    },
                    _ => panic!("unexpected custom message encountered during make payment"),
                }
            },
            _ => panic!("unexpected message occurred during oracle approval"),
        });
        let payable_scope_binary = query(
            deps.as_ref(),
            mock_env(),
            QueryMsg::QueryPayableByUuid {
                payable_uuid: DEFAULT_PAYABLE_UUID.to_string(),
            },
        )
        .unwrap();
        let scope_attribute = from_binary::<PayableScopeAttribute>(&payable_scope_binary).unwrap();
        assert_eq!(
            true, scope_attribute.oracle_approved,
            "the payable should be marked as oracle approved after the function executes"
        );
    }

    #[test]
    fn test_execute_oracle_approval_success_with_no_oracle_fee() {
        let mut deps = mock_dependencies(&[]);
        // Set the fee percent to 100%, ensuring that all funds are taken as a fee to the fee
        // collector, with none remaining for the oracle to withdraw
        let provenance_util = setup_test_suite(
            &mut deps,
            InstArgs {
                fee_percent: Decimal::percent(100),
                ..Default::default()
            },
        );
        test_register_payable(&mut deps, &provenance_util, TestRegisterPayable::default()).unwrap();
        let approval_response =
            test_oracle_approval(&mut deps, &provenance_util, TestOracleApproval::default())
                .unwrap();
        assert_eq!(
            4,
            approval_response.attributes.len(),
            "expected all attributes to be added"
        );
        assert_eq!(
            DEFAULT_PAYABLE_UUID,
            single_attribute_for_key(&approval_response, ORACLE_APPROVED_KEY),
            "expected the oracle approved key to be added as an attribute",
        );
        assert_eq!(
            DEFAULT_PAYABLE_TYPE,
            single_attribute_for_key(&approval_response, PAYABLE_TYPE_KEY),
            "expected the payable type key to be added as an attribute",
        );
        assert_eq!(
            DEFAULT_PAYABLE_UUID,
            single_attribute_for_key(&approval_response, PAYABLE_UUID_KEY),
            "expected the payable uuid key to be added as an attribute",
        );
        assert_eq!(
            DEFAULT_ORACLE_ADDRESS,
            single_attribute_for_key(&approval_response, ORACLE_ADDRESS_KEY),
            "expected the oracle address key to be added as an attribute",
        );
        assert_eq!(
            2,
            approval_response.messages.len(),
            "expected only attribute swap messages",
        );
        approval_response
            .messages
            .into_iter()
            .for_each(|msg| match msg.msg {
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
                _ => panic!("unexpected message occurred during oracle approval"),
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
            true, scope_attribute.oracle_approved,
            "the payable should be marked as oracle approved after the function executes"
        );
    }

    #[test]
    fn test_execute_oracle_approval_fails_for_included_funds() {
        let mut deps = mock_dependencies(&[]);
        // Set the fee percent to 100%, ensuring that all funds are taken as a fee to the fee
        // collector, with none remaining for the oracle to withdraw
        let provenance_util = setup_test_suite(
            &mut deps,
            InstArgs {
                fee_percent: Decimal::percent(100),
                ..Default::default()
            },
        );
        test_register_payable(&mut deps, &provenance_util, TestRegisterPayable::default()).unwrap();
        let error = test_oracle_approval(
            &mut deps,
            &provenance_util,
            TestOracleApproval {
                info: mock_info(
                    DEFAULT_ORACLE_ADDRESS,
                    // Include some funds in the request.  Oracle approvals should not charge the
                    // oracle
                    &[coin(400, DEFAULT_ONBOARDING_DENOM.to_string())],
                ),
                ..Default::default()
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
        let provenance_util = setup_test_suite(&mut deps, InstArgs::default());
        test_register_payable(&mut deps, &provenance_util, TestRegisterPayable::default()).unwrap();
        let error = test_oracle_approval(
            &mut deps,
            &provenance_util,
            // Try to call into the oracle approval as the fee collector.  Only the oracle is
            // allowed to make this call, so the execution should fail.
            TestOracleApproval {
                info: mock_info(DEFAULT_FEE_COLLECTION_ADDRESS, &[]),
                ..Default::default()
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
        let provenance_util = setup_test_suite(&mut deps, InstArgs::default());
        test_register_payable(&mut deps, &provenance_util, TestRegisterPayable::default()).unwrap();
        // Execute once with good args, should be a success
        test_oracle_approval(&mut deps, &provenance_util, TestOracleApproval::default()).unwrap();
        // Execute a second time, should be rejected because the oracle stamp has already been added
        let error =
            test_oracle_approval(&mut deps, &provenance_util, TestOracleApproval::default())
                .unwrap_err();
        match error {
            ContractError::DuplicateApproval { payable_uuid } => {
                assert_eq!(
                    DEFAULT_PAYABLE_UUID,
                    payable_uuid.as_str(),
                    "the error message should include the payable's uuid"
                );
            }
            _ => panic!("unexpected error occurred during execution"),
        };
    }

    #[test]
    fn test_execute_oracle_approval_fails_for_wrong_target_payable() {
        let mut deps = mock_dependencies(&[]);
        let provenance_util = setup_test_suite(&mut deps, InstArgs::default());
        test_register_payable(&mut deps, &provenance_util, TestRegisterPayable::default()).unwrap();
        // Closure to keep code more concise than some of these other tests I wrote...
        let error = test_oracle_approval(
            &mut deps,
            &provenance_util,
            TestOracleApproval {
                oracle_approval: OracleApprovalV1 {
                    payable_uuid: "09798cd6-83ad-11ec-b485-eff659cf8387".to_string(),
                },
                ..Default::default()
            },
        )
        .unwrap_err();
        match error {
            ContractError::PayableNotFound { payable_uuid } => {
                assert_eq!(
                    "09798cd6-83ad-11ec-b485-eff659cf8387",
                    payable_uuid.as_str(),
                    "the incorrect uuid should be included in the error message"
                );
            }
            _ => panic!("unexpected error occurred during execution"),
        }
    }
}
