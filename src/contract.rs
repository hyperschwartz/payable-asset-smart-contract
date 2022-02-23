use crate::core::error::ContractError;
use crate::core::msg::{ExecuteMsg, InitMsg, MigrateMsg, QueryMsg};
use crate::execute::make_payment::make_payment;
use crate::execute::oracle_approval::oracle_approval;
use crate::execute::register_payable::register_payable;
use crate::instantiate::init_contract::init_contract;
use crate::migrate::migrate_contract::migrate_contract;
use crate::query::query_payable_by_scope_id::query_payable_binary_by_scope_id;
use crate::query::query_payable_by_uuid::query_payable_binary_by_uuid;
use crate::query::query_state::query_state;
use crate::util::traits::ValidatedMsg;
use cosmwasm_std::{entry_point, Binary, Deps, DepsMut, Env, MessageInfo, Response};
use provwasm_std::{ProvenanceMsg, ProvenanceQuery};

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
        QueryMsg::QueryPayableByScopeId { scope_id } => {
            query_payable_binary_by_scope_id(&deps, scope_id)
        }
        QueryMsg::QueryPayableByUuid { payable_uuid } => {
            query_payable_binary_by_uuid(&deps, payable_uuid)
        }
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
        ExecuteMsg::RegisterPayable { .. } => {
            register_payable(deps, info, msg.to_register_payable()?)
        }
        ExecuteMsg::OracleApproval { .. } => oracle_approval(deps, info, msg.to_oracle_approval()?),
        ExecuteMsg::MakePayment { .. } => make_payment(deps, info, msg.to_make_payment()?),
    }
}

/// Called when migrating a contract instance to a new code ID.
#[entry_point]
pub fn migrate(
    deps: DepsMut<ProvenanceQuery>,
    _env: Env,
    msg: MigrateMsg,
) -> Result<Response<ProvenanceMsg>, ContractError> {
    // Ensure that the message is valid before processing the request
    msg.validate()?;
    let migrate_msg = msg.to_migrate_contract_v2(&deps.as_ref())?;
    migrate_contract(deps, migrate_msg)
}
