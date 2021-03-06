use crate::contract::instantiate;
use crate::core::error::ContractError;
use crate::core::msg::{ExecuteMsg, InitMsg};
use crate::core::state::PayableScopeAttribute;
use crate::testutil::mock_provenance_util::MockProvenanceUtil;
use cosmwasm_std::testing::{mock_env, mock_info, MockApi, MockStorage};
use cosmwasm_std::{Addr, Decimal, DepsMut, Env, MessageInfo, OwnedDeps, Response, Uint128};
use provwasm_mocks::ProvenanceMockQuerier;
use provwasm_std::{Party, PartyType, ProvenanceMsg, ProvenanceQuery, Scope};
use serde_json_wasm::to_string;

pub type MockOwnedDeps = OwnedDeps<MockStorage, MockApi, ProvenanceMockQuerier, ProvenanceQuery>;

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
    pub contract_name: String,
    pub onboarding_cost: String,
    pub onboarding_denom: String,
    pub fee_collection_address: String,
    pub fee_percent: Decimal,
    pub is_local: bool,
}
impl Default for InstArgs {
    fn default() -> Self {
        InstArgs {
            env: mock_env(),
            info: mock_info(DEFAULT_INFO_NAME, &[]),
            contract_name: DEFAULT_CONTRACT_NAME.into(),
            onboarding_cost: DEFAULT_ONBOARDING_COST.into(),
            onboarding_denom: DEFAULT_ONBOARDING_DENOM.into(),
            fee_collection_address: DEFAULT_FEE_COLLECTION_ADDRESS.into(),
            fee_percent: Decimal::percent(DEFAULT_FEE_PERCENT),
            is_local: false,
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
            contract_name: args.contract_name,
            onboarding_cost: args.onboarding_cost,
            onboarding_denom: args.onboarding_denom,
            fee_collection_address: args.fee_collection_address,
            fee_percent: args.fee_percent,
            is_local: Some(args.is_local),
        },
    )
}

pub fn setup_test_suite(deps: &mut MockOwnedDeps, args: InstArgs) -> MockProvenanceUtil {
    test_instantiate(deps.as_mut(), args).expect("instantiation should succeed");
    mock_default_scope(deps);
    MockProvenanceUtil::new()
}

pub fn default_register_payable() -> ExecuteMsg {
    ExecuteMsg::RegisterPayable {
        payable_type: DEFAULT_PAYABLE_TYPE.into(),
        payable_uuid: DEFAULT_PAYABLE_UUID.into(),
        scope_id: DEFAULT_SCOPE_ID.into(),
        oracle_address: DEFAULT_ORACLE_ADDRESS.into(),
        payable_denom: DEFAULT_PAYABLE_DENOM.into(),
        payable_total: Uint128::new(DEFAULT_PAYABLE_TOTAL),
    }
}

pub fn get_duped_scope(scope_id: impl Into<String>, owner_address: impl Into<String>) -> Scope {
    let owner_address = owner_address.into();
    Scope {
        scope_id: scope_id.into(),
        specification_id: "duped_spec_id".into(),
        owners: vec![Party {
            address: Addr::unchecked(&owner_address),
            role: PartyType::Owner,
        }],
        data_access: vec![],
        value_owner_address: Addr::unchecked(owner_address),
    }
}

pub fn mock_scope(
    deps: &mut MockOwnedDeps,
    scope_id: impl Into<String>,
    owner_address: impl Into<String>,
) {
    deps.querier
        .with_scope(get_duped_scope(scope_id, owner_address))
}

pub fn mock_default_scope(deps: &mut MockOwnedDeps) {
    mock_scope(deps, DEFAULT_SCOPE_ID, DEFAULT_INFO_NAME)
}

pub fn mock_scope_attribute(
    deps: &mut MockOwnedDeps,
    contract_name: impl Into<String>,
    attribute: &PayableScopeAttribute,
) {
    deps.querier.with_attributes(
        attribute.scope_id.clone().as_str(),
        &[(
            contract_name.into().as_str(),
            to_string(attribute).unwrap().as_str(),
            "json",
        )],
    );
}

pub fn mock_default_scope_attribute(deps: &mut MockOwnedDeps, attribute: &PayableScopeAttribute) {
    mock_scope_attribute(deps, DEFAULT_CONTRACT_NAME, attribute);
}

pub fn single_attribute_for_key<'a, T>(response: &'a Response<T>, key: &'a str) -> &'a str {
    response
        .attributes
        .iter()
        .find(|attr| attr.key.as_str() == key)
        .unwrap()
        .value
        .as_str()
}
