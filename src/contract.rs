use crate::core::error::ContractError;
use crate::core::msg::{ExecuteMsg, InitMsg, MigrateMsg, QueryMsg};
use crate::execute::make_payment::{make_payment, MakePaymentV1};
use crate::execute::oracle_approval::{oracle_approval, OracleApprovalV1};
use crate::execute::register_payable::{register_payable, RegisterPayableV1};
use crate::instantiate::init_contract::init_contract;
use crate::query::query_payable::query_payable;
use crate::query::query_state::query_state;
use cosmwasm_std::{entry_point, Binary, Deps, DepsMut, Env, MessageInfo, Response};
use provwasm_std::{ProvenanceMsg, ProvenanceQuery};
use crate::migrate::migrate_contract::migrate_contract;
use crate::util::traits::ValidatedMsg;

/// Initialize the contract
#[entry_point]
pub fn instantiate(
    deps: DepsMut<ProvenanceQuery>,
    env: Env,
    info: MessageInfo,
    msg: InitMsg,
) -> Result<Response<ProvenanceMsg>, ContractError> {
    // Ensure that the message is valid before processing the request
    msg.validate()?;
    init_contract(deps, env, info, msg)
}

/// Query contract state.
#[entry_point]
pub fn query(
    deps: Deps<ProvenanceQuery>,
    _env: Env,
    msg: QueryMsg,
) -> Result<Binary, ContractError> {
    // Ensure that the message is valid before processing the request
    msg.validate()?;
    match msg {
        QueryMsg::QueryState {} => query_state(deps),
        QueryMsg::QueryPayable { payable_uuid } => query_payable(deps, payable_uuid),
    }
}

/// Handle execution strategies - register payable, oracle approval, make payments
#[entry_point]
pub fn execute(
    deps: DepsMut<ProvenanceQuery>,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response<ProvenanceMsg>, ContractError> {
    // Ensure that the message is valid before processing the request
    msg.validate()?;
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
            RegisterPayableV1 {
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

/// Called when migrating a contract instance to a new code ID.
#[entry_point]
pub fn migrate(
    deps: DepsMut<ProvenanceQuery>,
    _env: Env,
    msg: MigrateMsg,
) -> Result<Response, ContractError> {
    // Ensure that the message is valid before processing the request
    msg.validate()?;
    migrate_contract(deps, msg)
}
