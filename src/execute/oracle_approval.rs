use crate::core::error::ContractError;
use crate::core::state::{config, payable_meta_storage};
use crate::util::constants::{ORACLE_APPROVED_KEY, PAYABLE_TYPE_KEY, PAYABLE_UUID_KEY};
use cosmwasm_std::{coin, BankMsg, CosmosMsg, DepsMut, MessageInfo, Response};
use provwasm_std::{ProvenanceMsg, ProvenanceQuery};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::ops::Mul;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct OracleApprovalV1 {
    pub payable_uuid: String,
}

pub fn oracle_approval(
    deps: DepsMut<ProvenanceQuery>,
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
    let mut target_payable = match payables_bucket.load(oracle_approval.payable_uuid.as_bytes()) {
        Ok(meta) => {
            if meta.oracle_approved {
                return Err(ContractError::DuplicateApproval {
                    payable_uuid: oracle_approval.payable_uuid,
                });
            }
            meta
        }
        Err(_) => {
            return Err(ContractError::PayableNotFound {
                payable_uuid: oracle_approval.payable_uuid,
            });
        }
    };
    // TODO: Tag an attribute on the scope once functionality is available
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
    payables_bucket.save(oracle_approval.payable_uuid.as_bytes(), &target_payable)?;
    Ok(Response::new()
        .add_messages(messages)
        .add_attribute(ORACLE_APPROVED_KEY, &target_payable.payable_uuid)
        .add_attribute(PAYABLE_TYPE_KEY, state.payable_type)
        .add_attribute(PAYABLE_UUID_KEY, target_payable.payable_uuid))
}

#[cfg(test)]
mod tests {
    use crate::contract::{execute, query};
    use crate::core::error::ContractError;
    use crate::core::msg::{ExecuteMsg, QueryMsg};
    use crate::core::state::PayableMeta;
    use crate::testutil::test_utilities::{
        default_register_payable, get_duped_scope, single_attribute_for_key, test_instantiate,
        InstArgs, DEFAULT_FEE_COLLECTION_ADDRESS, DEFAULT_INFO_NAME, DEFAULT_ONBOARDING_DENOM,
        DEFAULT_ORACLE_ADDRESS, DEFAULT_PAYABLE_TYPE, DEFAULT_PAYABLE_UUID, DEFAULT_SCOPE_ID,
    };
    use crate::util::constants::{ORACLE_APPROVED_KEY, PAYABLE_TYPE_KEY, PAYABLE_UUID_KEY};
    use cosmwasm_std::testing::{mock_env, mock_info};
    use cosmwasm_std::{coin, from_binary, BankMsg, CosmosMsg, Decimal};
    use provwasm_mocks::mock_dependencies;

    #[test]
    fn test_execute_oracle_approval_success() {
        let mut deps = mock_dependencies(&[]);
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
        deps.querier
            .with_scope(get_duped_scope(DEFAULT_SCOPE_ID, DEFAULT_INFO_NAME));
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
                payable_uuid: DEFAULT_PAYABLE_UUID.into(),
            },
        )
        .unwrap();
        assert_eq!(
            3,
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
            1,
            approval_response.messages.len(),
            "expected a message for the oracle fee withdrawal"
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
            _ => panic!("unexpected message occurred during oracle approval"),
        });
        let payable_binary = query(
            deps.as_ref(),
            mock_env(),
            QueryMsg::QueryPayable {
                payable_uuid: DEFAULT_PAYABLE_UUID.to_string(),
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
        deps.querier
            .with_scope(get_duped_scope(DEFAULT_SCOPE_ID, DEFAULT_INFO_NAME));
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
                payable_uuid: DEFAULT_PAYABLE_UUID.into(),
            },
        )
        .unwrap();
        assert!(
            approval_response.messages.is_empty(),
            "expected no messages, because the oracle should not have funds to withdraw"
        );
        assert_eq!(
            3,
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
        let payable_binary = query(
            deps.as_ref(),
            mock_env(),
            QueryMsg::QueryPayable {
                payable_uuid: DEFAULT_PAYABLE_UUID.to_string(),
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
        deps.querier
            .with_scope(get_duped_scope(DEFAULT_SCOPE_ID, DEFAULT_INFO_NAME));
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
                payable_uuid: DEFAULT_PAYABLE_UUID.into(),
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
        deps.querier
            .with_scope(get_duped_scope(DEFAULT_SCOPE_ID, DEFAULT_INFO_NAME));
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
                payable_uuid: DEFAULT_PAYABLE_UUID.into(),
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
        deps.querier
            .with_scope(get_duped_scope(DEFAULT_SCOPE_ID, DEFAULT_INFO_NAME));
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
                    payable_uuid: DEFAULT_PAYABLE_UUID.into(),
                },
            )
        };
        // Execute once with good args, should be a success
        execute_as_oracle().unwrap();
        // Execute a second time, should be rejected because the oracle stamp has already been added
        let error = execute_as_oracle().unwrap_err();
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
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
        deps.querier
            .with_scope(get_duped_scope(DEFAULT_SCOPE_ID, DEFAULT_INFO_NAME));
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
                payable_uuid: "09798cd6-83ad-11ec-b485-eff659cf8387".into(),
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
