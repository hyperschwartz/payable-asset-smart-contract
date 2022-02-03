use cosmwasm_std::{
    coin, to_binary, Attribute, BankMsg, Binary, CosmosMsg, Decimal, Deps, DepsMut, Env,
    MessageInfo, QuerierWrapper, Response, StdError, StdResult, Uint128,
};
use provwasm_std::{
    bind_name, NameBinding, Party, PartyType, ProvenanceMsg, ProvenanceQuerier, Scope,
};
use std::ops::Mul;

use crate::error::ContractError;
use crate::helper::{
    to_percent, ORACLE_APPROVED_KEY, ORACLE_FUNDS_KEPT, PAYABLE_REGISTERED_KEY, PAYABLE_TYPE_KEY,
    PAYABLE_UUID_KEY, PAYMENT_AMOUNT_KEY, PAYMENT_MADE_KEY, REFUND_AMOUNT_KEY,
    REGISTERED_DENOM_KEY, SCOPE_ID_KEY, TOTAL_OWED_KEY, TOTAL_REMAINING_KEY,
};
use crate::make_payment::MakePaymentV1;
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
        QueryMsg::QueryPayable { payable_uuid } => {
            let meta_storage = payable_meta_storage_read(deps.storage);
            let payable_meta = meta_storage.load(payable_uuid.as_bytes())?;
            let json = to_binary(&payable_meta)?;
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
        ExecuteMsg::RegisterPayable {
            payable_type,
            payable_uuid,
            scope_id,
            payable_denom,
            payable_total,
        } => register_payable(
            deps,
            info,
            RegisterPayableMarkerV1 {
                payable_type,
                payable_uuid,
                scope_id,
                payable_denom,
                payable_total,
            },
        ),
        ExecuteMsg::OracleApproval { payable_uuid } => {
            oracle_approval(deps, info, OracleApprovalV1 { payable_uuid })
        }
        ExecuteMsg::MakePayment { payable_uuid } => {
            make_payment(deps, info, MakePaymentV1 { payable_uuid })
        }
    }
}

fn register_payable(
    deps: DepsMut,
    info: MessageInfo,
    register: RegisterPayableMarkerV1,
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
    attributes.push(Attribute::new(PAYABLE_REGISTERED_KEY, ""));
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

fn get_scope_by_id(querier: &QuerierWrapper, scope_id: &str) -> StdResult<Scope> {
    ProvenanceQuerier::new(querier).get_scope(scope_id)
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
        .add_attribute(ORACLE_APPROVED_KEY, "")
        .add_attribute(PAYABLE_TYPE_KEY, state.payable_type)
        .add_attribute(PAYABLE_UUID_KEY, target_payable.payable_uuid))
}

fn make_payment(
    deps: DepsMut,
    info: MessageInfo,
    make_payment: MakePaymentV1,
) -> Result<Response<ProvenanceMsg>, ContractError> {
    let mut payables_bucket = payable_meta_storage(deps.storage);
    let mut target_payable = match payables_bucket.load(make_payment.payable_uuid.as_bytes()) {
        Ok(meta) => {
            if !meta.oracle_approved {
                return Err(ContractError::NotReadyForPayment {
                    payable_uuid: meta.payable_uuid,
                    not_ready_reason: "Payable missing oracle approval".into(),
                });
            }
            meta
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
            if coin.denom != target_payable.payable_denom {
                Some(coin.denom.clone())
            } else {
                None
            }
        })
        .collect::<Vec<String>>();
    if !invalid_funds.is_empty() {
        return Err(ContractError::InvalidFundsProvided {
            valid_denom: target_payable.payable_denom,
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
            valid_denom: target_payable.payable_denom,
        });
    }
    if payment_amount > target_payable.payable_remaining_owed.u128() {
        return Err(ContractError::PaymentTooLarge {
            total_owed: target_payable.payable_remaining_owed.u128(),
            amount_provided: payment_amount,
        });
    }
    let scope_owners: Vec<Party> =
        get_scope_by_id(&deps.querier, target_payable.scope_id.as_str())?
            .owners
            .into_iter()
            .filter(|owner| owner.role == PartyType::Owner)
            .collect();
    // TODO: Handle multiple owners if needed, or at least confirm this strategy's sane usage
    if scope_owners.len() != 1 {
        return Err(ContractError::InvalidPayable {
            payable_uuid: target_payable.payable_uuid,
            invalid_reason: "Payable has multiple owners. Only single ownership is supported"
                .into(),
        });
    }
    let payment_message = CosmosMsg::Bank(BankMsg::Send {
        to_address: scope_owners.first().unwrap().address.clone(),
        amount: vec![coin(payment_amount, &target_payable.payable_denom)],
    });
    // Subtract payment amount from tracked total
    target_payable.payable_remaining_owed =
        (target_payable.payable_remaining_owed.u128() - payment_amount).into();
    payables_bucket.save(target_payable.payable_uuid.as_bytes(), &target_payable)?;
    // Load state to derive payable type
    let state = config(deps.storage).load()?;
    Ok(Response::new()
        .add_message(payment_message)
        .add_attribute(PAYMENT_MADE_KEY, "")
        .add_attribute(PAYABLE_TYPE_KEY, state.payable_type)
        .add_attribute(PAYABLE_UUID_KEY, target_payable.payable_uuid)
        .add_attribute(PAYMENT_AMOUNT_KEY, payment_amount.to_string())
        .add_attribute(TOTAL_REMAINING_KEY, target_payable.payable_remaining_owed))
}

/// Called when migrating a contract instance to a new code ID.
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    Ok(Response::default())
}

#[cfg(test)]
mod tests {
    use crate::msg::QueryResponse;

    use super::*;
    use crate::error::ContractError::Std;
    use cosmwasm_std::testing::{mock_env, mock_info};
    use cosmwasm_std::StdError::GenericErr;
    use cosmwasm_std::{from_binary, CosmosMsg, Decimal};
    use provwasm_mocks::mock_dependencies;
    use provwasm_std::{NameMsgParams, Party, PartyType, ProvenanceMsgParams};

    const DEFAULT_INFO_NAME: &str = "admin";
    const DEFAULT_PAYABLE_TYPE: &str = "invoice";
    const DEFAULT_CONTRACT_NAME: &str = "payables.asset";
    const DEFAULT_ONBOARDING_COST: &str = "100";
    const DEFAULT_ONBOARDING_DENOM: &str = "nhash";
    const DEFAULT_FEE_COLLECTION_ADDRESS: &str = "feebucket";
    const DEFAULT_FEE_PERCENT: u64 = 75;
    const DEFAULT_ORACLE_ADDRESS: &str = "matt";
    const DEFAULT_PAYABLE_UUID: &str = "200425c6-83ab-11ec-a486-eb4f069082c5";
    const DEFAULT_SCOPE_ID: &str = "scope1qpyq6g6j0tuprmyglw0hn2czfzsq6fcyl8";
    const DEFAULT_PAYABLE_TOTAL: u128 = 1000;
    const DEFAULT_PAYABLE_DENOM: &str = "nhash";

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
            "",
            response
                .attributes
                .iter()
                .find(|attr| attr.key.as_str() == PAYABLE_REGISTERED_KEY)
                .unwrap()
                .value
                .as_str(),
            "the PAYABLE_REGISTERED_KEY should be present with no value",
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
            "",
            response
                .attributes
                .iter()
                .find(|attr| attr.key.as_str() == PAYABLE_REGISTERED_KEY)
                .unwrap()
                .value
                .as_str(),
            "the PAYABLE_REGISTERED_KEY should be present with no value",
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

    #[test]
    fn test_query_payable_after_register() {
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
            DEFAULT_PAYABLE_UUID,
            payable_meta.payable_uuid.as_str(),
            "expected the default marker address to be returned"
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
        assert_eq!(
            3,
            approval_response.attributes.len(),
            "expected all attributes to be added"
        );
        assert_eq!(
            "",
            approval_response
                .attributes
                .iter()
                .find(|attr| attr.key.as_str() == ORACLE_APPROVED_KEY)
                .unwrap()
                .value
                .as_str(),
            "expected the oracle approved key to be added as an attribute",
        );
        assert_eq!(
            DEFAULT_PAYABLE_TYPE,
            approval_response
                .attributes
                .iter()
                .find(|attr| attr.key.as_str() == PAYABLE_TYPE_KEY)
                .unwrap()
                .value
                .as_str(),
            "expected the payable type key to be added as an attribute",
        );
        assert_eq!(
            DEFAULT_PAYABLE_UUID,
            approval_response
                .attributes
                .iter()
                .find(|attr| attr.key.as_str() == PAYABLE_UUID_KEY)
                .unwrap()
                .value
                .as_str(),
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
            "",
            approval_response
                .attributes
                .iter()
                .find(|attr| attr.key.as_str() == ORACLE_APPROVED_KEY)
                .unwrap()
                .value
                .as_str(),
            "expected the oracle approved key to be added as an attribute",
        );
        assert_eq!(
            DEFAULT_PAYABLE_TYPE,
            approval_response
                .attributes
                .iter()
                .find(|attr| attr.key.as_str() == PAYABLE_TYPE_KEY)
                .unwrap()
                .value
                .as_str(),
            "expected the payable type key to be added as an attribute",
        );
        assert_eq!(
            DEFAULT_PAYABLE_UUID,
            approval_response
                .attributes
                .iter()
                .find(|attr| attr.key.as_str() == PAYABLE_UUID_KEY)
                .unwrap()
                .value
                .as_str(),
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

    #[test]
    fn test_execute_make_payment_paid_in_full() {
        let mut deps = mock_dependencies(&[]);
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
        deps.querier
            .with_scope(get_duped_scope(DEFAULT_SCOPE_ID, DEFAULT_INFO_NAME));
        // Register the default payable for payment
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
        // Mark the oracle as approved
        execute(
            deps.as_mut(),
            mock_env(),
            mock_info(DEFAULT_ORACLE_ADDRESS, &[]),
            ExecuteMsg::OracleApproval {
                payable_uuid: DEFAULT_PAYABLE_UUID.into(),
            },
        )
        .unwrap();
        let payment_response = execute(
            deps.as_mut(),
            mock_env(),
            mock_info(
                "payer-guy",
                &[coin(DEFAULT_PAYABLE_TOTAL, DEFAULT_PAYABLE_DENOM)],
            ),
            ExecuteMsg::MakePayment {
                payable_uuid: DEFAULT_PAYABLE_UUID.into(),
            },
        )
        .unwrap();
        assert_eq!(
            1,
            payment_response.messages.len(),
            "one message should be added: the payment to the owner of the payable"
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
                _ => panic!("unexpected message sent during payment"),
            });
        assert_eq!(
            5,
            payment_response.attributes.len(),
            "expected all attributes to be added to the response"
        );
        assert_eq!(
            "",
            payment_response
                .attributes
                .iter()
                .find(|attr| attr.key.as_str() == PAYMENT_MADE_KEY)
                .unwrap()
                .value
                .as_str(),
            "expected the payment made key to be added to the response",
        );
        assert_eq!(
            DEFAULT_PAYABLE_TYPE,
            payment_response
                .attributes
                .iter()
                .find(|attr| attr.key.as_str() == PAYABLE_TYPE_KEY)
                .unwrap()
                .value
                .as_str(),
            "expected the payable type key to be added to the response",
        );
        assert_eq!(
            DEFAULT_PAYABLE_UUID,
            payment_response
                .attributes
                .iter()
                .find(|attr| attr.key.as_str() == PAYABLE_UUID_KEY)
                .unwrap()
                .value
                .as_str(),
            "expected the payable uuid key to be added to the response",
        );
        assert_eq!(
            DEFAULT_PAYABLE_TOTAL.to_string(),
            payment_response.attributes.iter().find(|attr| attr.key.as_str() == PAYMENT_AMOUNT_KEY).unwrap().value,
            "expected the payment amount key to be added to the response and equate to the total owed",
        );
        assert_eq!(
            "0",
            payment_response.attributes.iter().find(|attr| attr.key.as_str() == TOTAL_REMAINING_KEY).unwrap().value,
            "expected the total remaining key to be added to the response and equate to zero because the payable was paid off",
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
            DEFAULT_PAYABLE_TOTAL,
            payable_meta.payable_total_owed.u128(),
            "the total owed should remain unchanged by the payment"
        );
        assert_eq!(
            0u128,
            payable_meta.payable_remaining_owed.u128(),
            "the remaining owed should be reduced to zero after the successful payment"
        );
    }

    #[test]
    fn test_execute_make_payment_pay_less_than_all() {
        let mut deps = mock_dependencies(&[]);
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
        deps.querier
            .with_scope(get_duped_scope(DEFAULT_SCOPE_ID, DEFAULT_INFO_NAME));
        // Register the default payable for payment
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
        // Mark the oracle as approved
        execute(
            deps.as_mut(),
            mock_env(),
            mock_info(DEFAULT_ORACLE_ADDRESS, &[]),
            ExecuteMsg::OracleApproval {
                payable_uuid: DEFAULT_PAYABLE_UUID.into(),
            },
        )
        .unwrap();
        let payment_response = execute(
            deps.as_mut(),
            mock_env(),
            // Pay 100 less than the total required for pay off
            mock_info(
                "payer-guy",
                &[coin(DEFAULT_PAYABLE_TOTAL - 100, DEFAULT_PAYABLE_DENOM)],
            ),
            ExecuteMsg::MakePayment {
                payable_uuid: DEFAULT_PAYABLE_UUID.into(),
            },
        )
        .unwrap();
        assert_eq!(
            1,
            payment_response.messages.len(),
            "one message should be added: the payment to the owner of the payable"
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
                _ => panic!("unexpected message sent during payment"),
            });
        assert_eq!(
            5,
            payment_response.attributes.len(),
            "expected all attributes to be added to the response"
        );
        assert_eq!(
            "",
            payment_response
                .attributes
                .iter()
                .find(|attr| attr.key.as_str() == PAYMENT_MADE_KEY)
                .unwrap()
                .value
                .as_str(),
            "expected the payment made key to be added to the response",
        );
        assert_eq!(
            DEFAULT_PAYABLE_TYPE,
            payment_response
                .attributes
                .iter()
                .find(|attr| attr.key.as_str() == PAYABLE_TYPE_KEY)
                .unwrap()
                .value
                .as_str(),
            "expected the payable type key to be added to the response",
        );
        assert_eq!(
            DEFAULT_PAYABLE_UUID,
            payment_response
                .attributes
                .iter()
                .find(|attr| attr.key.as_str() == PAYABLE_UUID_KEY)
                .unwrap()
                .value
                .as_str(),
            "expected the payable uuid key to be added to the response",
        );
        assert_eq!(
            (DEFAULT_PAYABLE_TOTAL - 100).to_string(),
            payment_response.attributes.iter().find(|attr| attr.key.as_str() == PAYMENT_AMOUNT_KEY).unwrap().value,
            "expected the payment amount key to be added to the response and equate to the total owed - 100",
        );
        assert_eq!(
            "100",
            payment_response.attributes.iter().find(|attr| attr.key.as_str() == TOTAL_REMAINING_KEY).unwrap().value,
            "expected the total remaining key to be added to the response and equate to 100 because that was the amount unpaid",
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
            DEFAULT_PAYABLE_TOTAL,
            payable_meta.payable_total_owed.u128(),
            "the total owed should remain unchanged by the payment"
        );
        assert_eq!(
            100u128,
            payable_meta.payable_remaining_owed.u128(),
            "the remaining owed should be reduced to 100 after the successful payment"
        );
        // Pay subsequently to watch the values be reduced
        execute(
            deps.as_mut(),
            mock_env(),
            mock_info("payer-guy", &[coin(100, DEFAULT_PAYABLE_DENOM)]),
            ExecuteMsg::MakePayment {
                payable_uuid: DEFAULT_PAYABLE_UUID.into(),
            },
        )
        .unwrap();
        let subsequent_payable_binary = query(
            deps.as_ref(),
            mock_env(),
            QueryMsg::QueryPayable {
                payable_uuid: DEFAULT_PAYABLE_UUID.to_string(),
            },
        )
        .unwrap();
        let subsequent_payable_meta =
            from_binary::<PayableMeta>(&subsequent_payable_binary).unwrap();
        assert_eq!(
            DEFAULT_PAYABLE_TOTAL,
            subsequent_payable_meta.payable_total_owed.u128(),
            "the total owed should remain unchanged by the subsequent payment"
        );
        assert_eq!(
            0u128,
            subsequent_payable_meta.payable_remaining_owed.u128(),
            "the remaining owed should now be reduced to zero after the subsequent payment"
        );
    }

    #[test]
    fn test_execute_make_payment_missing_payable_uuid() {
        let mut deps = mock_dependencies(&[]);
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
        // No need to do anything upfront because we're going to target a non-existent payable
        let failure = execute(
            deps.as_mut(),
            mock_env(),
            mock_info(
                "payer-guy",
                &[coin(DEFAULT_PAYABLE_TOTAL, DEFAULT_PAYABLE_DENOM)],
            ),
            ExecuteMsg::MakePayment {
                payable_uuid: DEFAULT_PAYABLE_UUID.into(),
            },
        )
        .unwrap_err();
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
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
        deps.querier
            .with_scope(get_duped_scope(DEFAULT_SCOPE_ID, DEFAULT_INFO_NAME));
        // Register the default payable for payment
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
        // Mark the oracle as approved
        execute(
            deps.as_mut(),
            mock_env(),
            mock_info(DEFAULT_ORACLE_ADDRESS, &[]),
            ExecuteMsg::OracleApproval {
                payable_uuid: DEFAULT_PAYABLE_UUID.into(),
            },
        )
        .unwrap();
        let failure = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("payer-guy", &[coin(DEFAULT_PAYABLE_TOTAL, "fakecoin")]),
            ExecuteMsg::MakePayment {
                payable_uuid: DEFAULT_PAYABLE_UUID.into(),
            },
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
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
        deps.querier
            .with_scope(get_duped_scope(DEFAULT_SCOPE_ID, DEFAULT_INFO_NAME));
        // Register the default payable for payment
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
        // Mark the oracle as approved
        execute(
            deps.as_mut(),
            mock_env(),
            mock_info(DEFAULT_ORACLE_ADDRESS, &[]),
            ExecuteMsg::OracleApproval {
                payable_uuid: DEFAULT_PAYABLE_UUID.into(),
            },
        )
        .unwrap();
        let failure = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("payer-guy", &[]),
            ExecuteMsg::MakePayment {
                payable_uuid: DEFAULT_PAYABLE_UUID.into(),
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
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
        deps.querier
            .with_scope(get_duped_scope(DEFAULT_SCOPE_ID, DEFAULT_INFO_NAME));
        // Register the default payable for payment
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
        // Mark the oracle as approved
        execute(
            deps.as_mut(),
            mock_env(),
            mock_info(DEFAULT_ORACLE_ADDRESS, &[]),
            ExecuteMsg::OracleApproval {
                payable_uuid: DEFAULT_PAYABLE_UUID.into(),
            },
        )
        .unwrap();
        let failure = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("payer-guy", &[coin(0, DEFAULT_PAYABLE_DENOM)]),
            ExecuteMsg::MakePayment {
                payable_uuid: DEFAULT_PAYABLE_UUID.into(),
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
            _ => panic!("unexpected error received when zero funds provided"),
        };
    }

    #[test]
    fn test_execute_make_payment_too_many_funds_provided() {
        let mut deps = mock_dependencies(&[]);
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
        deps.querier
            .with_scope(get_duped_scope(DEFAULT_SCOPE_ID, DEFAULT_INFO_NAME));
        // Register the default payable for payment
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
        // Mark the oracle as approved
        execute(
            deps.as_mut(),
            mock_env(),
            mock_info(DEFAULT_ORACLE_ADDRESS, &[]),
            ExecuteMsg::OracleApproval {
                payable_uuid: DEFAULT_PAYABLE_UUID.into(),
            },
        )
        .unwrap();
        let failure = execute(
            deps.as_mut(),
            mock_env(),
            mock_info(
                "payer-guy",
                &[coin(DEFAULT_PAYABLE_TOTAL + 1, DEFAULT_PAYABLE_DENOM)],
            ),
            ExecuteMsg::MakePayment {
                payable_uuid: DEFAULT_PAYABLE_UUID.into(),
            },
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

    struct InstArgs {
        env: Env,
        info: MessageInfo,
        payable_type: String,
        contract_name: String,
        onboarding_cost: String,
        onboarding_denom: String,
        fee_collection_address: String,
        fee_percent: Decimal,
        oracle_address: String,
        is_local: bool,
    }

    impl Default for InstArgs {
        fn default() -> Self {
            InstArgs {
                env: mock_env(),
                info: mock_info(DEFAULT_INFO_NAME, &[]),
                payable_type: DEFAULT_PAYABLE_TYPE.into(),
                contract_name: DEFAULT_CONTRACT_NAME.into(),
                onboarding_cost: DEFAULT_ONBOARDING_COST.into(),
                onboarding_denom: DEFAULT_ONBOARDING_DENOM.into(),
                fee_collection_address: DEFAULT_FEE_COLLECTION_ADDRESS.into(),
                fee_percent: Decimal::percent(DEFAULT_FEE_PERCENT),
                oracle_address: DEFAULT_ORACLE_ADDRESS.into(),
                is_local: false,
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
                payable_type: args.payable_type,
                contract_name: args.contract_name,
                onboarding_cost: args.onboarding_cost,
                onboarding_denom: args.onboarding_denom,
                fee_collection_address: args.fee_collection_address,
                fee_percent: args.fee_percent,
                oracle_address: args.oracle_address,
                is_local: Some(args.is_local),
            },
        )
    }

    fn default_register_payable() -> ExecuteMsg {
        ExecuteMsg::RegisterPayable {
            payable_type: DEFAULT_PAYABLE_TYPE.into(),
            payable_uuid: DEFAULT_PAYABLE_UUID.into(),
            scope_id: DEFAULT_SCOPE_ID.into(),
            payable_denom: DEFAULT_PAYABLE_DENOM.into(),
            payable_total: Uint128::new(DEFAULT_PAYABLE_TOTAL),
        }
    }

    fn get_duped_scope(scope_id: &str, owner_address: &str) -> Scope {
        Scope {
            scope_id: scope_id.into(),
            specification_id: "duped_spec_id".into(),
            owners: vec![Party {
                address: owner_address.into(),
                role: PartyType::Owner,
            }],
            data_access: vec![],
            value_owner_address: owner_address.into(),
        }
    }
}
