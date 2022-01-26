use cosmwasm_std::{
    coin, to_binary, BankMsg, Binary, CosmosMsg, Deps, DepsMut, Env, MessageInfo, Response,
    StdError, StdResult, Uint128,
};
use provwasm_std::{
    add_attribute, bind_name, AttributeValueType, NameBinding, PartyType, ProvenanceMsg,
    ProvenanceQuerier,
};
use std::ops::Mul;

use crate::error::ContractError;
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
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response<ProvenanceMsg>, ContractError> {
    match msg {
        ExecuteMsg::RegisterScope { scope_id } => register_scope(deps, env, info, scope_id),
    }
}

fn register_scope(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    scope_id: String,
) -> Result<Response<ProvenanceMsg>, ContractError> {
    let state = config(deps.storage).load()?;
    let querier = ProvenanceQuerier::new(&deps.querier);
    let scope = querier.get_scope(scope_id.clone())?;
    // Verify that the address in the request is an owner of this scope
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
        // TODO: Maybe don't use the scope id as the value of the attribute. Something more useufl will likely present its as development continues
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
        Some(coin) => coin.amount,
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
    if onboarding_cost < funds_sent {
        return Err(ContractError::InsufficientFundsProvided {
            amount_needed: onboarding_cost.u128(),
            amount_provided: funds_sent.u128(),
        });
    }
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
    use provwasm_std::{NameMsgParams, ProvenanceMsgParams};

    #[test]
    fn valid_init() {
        // Create mocks
        let mut deps = mock_dependencies(&[]);

        // Create valid config state
        let res = test_instantiate(
            deps.as_mut(),
            InstArgs {
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
    }

    #[test]
    fn query_test() {
        // Create mocks
        let mut deps = mock_dependencies(&[]);

        test_instantiate(
            deps.as_mut(),
            InstArgs {
                ..Default::default()
            },
        )
        .unwrap();

        // Call the smart contract query function to get stored state.
        let bin = query(deps.as_ref(), mock_env(), QueryMsg::QueryRequest {}).unwrap();
        let resp: QueryResponse = from_binary(&bin).unwrap();

        // Ensure the expected init fields were properly stored.
        assert_eq!(resp.contract_name, "payables.asset");
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
                onboarding_cost: "150".into(),
                onboarding_denom: "nhash".into(),
                fee_collection_address: "feebucket".into(),
                fee_percent: Decimal::percent(75),
                oracle_address: "matt".into(),
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
}
