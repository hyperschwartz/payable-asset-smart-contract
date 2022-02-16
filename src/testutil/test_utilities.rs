use crate::contract::instantiate;
use crate::core::error::ContractError;
use crate::core::msg::{ExecuteMsg, InitMsg};
use cosmwasm_std::testing::{mock_env, mock_info};
use cosmwasm_std::{Addr, Decimal, DepsMut, Env, MessageInfo, Response, Uint128};
use provwasm_std::{Party, PartyType, ProvenanceMsg, ProvenanceQuery, Scope};

pub const DEFAULT_INFO_NAME: &str = "admin";
pub const DEFAULT_PAYABLE_TYPE: &str = "invoice";
pub const DEFAULT_CONTRACT_NAME: &str = "payables.asset";
pub const DEFAULT_ONBOARDING_COST: &str = "100";
pub const DEFAULT_ONBOARDING_DENOM: &str = "nhash";
pub const DEFAULT_FEE_COLLECTION_ADDRESS: &str = "feebucket";
pub const DEFAULT_FEE_PERCENT: u64 = 75;
pub const DEFAULT_ORACLE_ADDRESS: &str = "matt";
pub const DEFAULT_PAYABLE_UUID: &str = "200425c6-83ab-11ec-a486-eb4f069082c5";
pub const DEFAULT_SCOPE_ID: &str = "scope1qpyq6g6j0tuprmyglw0hn2czfzsq6fcyl8";
pub const DEFAULT_PAYABLE_TOTAL: u128 = 1000;
pub const DEFAULT_PAYABLE_DENOM: &str = "nhash";

pub struct InstArgs {
    pub env: Env,
    pub info: MessageInfo,
    pub payable_type: String,
    pub contract_name: String,
    pub onboarding_cost: String,
    pub onboarding_denom: String,
    pub fee_collection_address: String,
    pub fee_percent: Decimal,
    pub oracle_address: String,
    pub is_local: bool,
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

pub fn test_instantiate(
    deps: DepsMut<ProvenanceQuery>,
    args: InstArgs,
) -> Result<Response<ProvenanceMsg>, ContractError> {
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

pub fn default_register_payable() -> ExecuteMsg {
    ExecuteMsg::RegisterPayable {
        payable_type: DEFAULT_PAYABLE_TYPE.into(),
        payable_uuid: DEFAULT_PAYABLE_UUID.into(),
        scope_id: DEFAULT_SCOPE_ID.into(),
        payable_denom: DEFAULT_PAYABLE_DENOM.into(),
        payable_total: Uint128::new(DEFAULT_PAYABLE_TOTAL),
    }
}

pub fn get_duped_scope(scope_id: &str, owner_address: &str) -> Scope {
    Scope {
        scope_id: scope_id.into(),
        specification_id: "duped_spec_id".into(),
        owners: vec![Party {
            address: Addr::unchecked(owner_address),
            role: PartyType::Owner,
        }],
        data_access: vec![],
        value_owner_address: Addr::unchecked(owner_address),
    }
}
