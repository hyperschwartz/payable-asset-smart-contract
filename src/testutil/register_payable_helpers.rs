use crate::core::error::ContractError;
use crate::execute::register_payable::{register_payable_with_util, RegisterPayableV2};
use crate::testutil::mock_provenance_util::MockProvenanceUtil;
use crate::testutil::test_utilities::{
    MockOwnedDeps, DEFAULT_CONTRACT_NAME, DEFAULT_INFO_NAME, DEFAULT_ONBOARDING_DENOM,
    DEFAULT_ORACLE_ADDRESS, DEFAULT_PAYABLE_DENOM, DEFAULT_PAYABLE_TOTAL, DEFAULT_PAYABLE_TYPE,
    DEFAULT_PAYABLE_UUID, DEFAULT_SCOPE_ID,
};
use cosmwasm_std::testing::mock_info;
use cosmwasm_std::{coin, MessageInfo, Response, Uint128};
use provwasm_std::ProvenanceMsg;

pub struct TestRegisterPayable {
    pub info: MessageInfo,
    pub contract_name: String,
    pub register_payable: RegisterPayableV2,
}
impl TestRegisterPayable {
    pub fn default_register_payable() -> RegisterPayableV2 {
        RegisterPayableV2 {
            payable_type: DEFAULT_PAYABLE_TYPE.to_string(),
            payable_uuid: DEFAULT_PAYABLE_UUID.to_string(),
            scope_id: DEFAULT_SCOPE_ID.to_string(),
            oracle_address: DEFAULT_ORACLE_ADDRESS.to_string(),
            payable_denom: DEFAULT_PAYABLE_DENOM.to_string(),
            payable_total: Uint128::new(DEFAULT_PAYABLE_TOTAL),
        }
    }

    pub fn default_full_sender(sender: &str, amount: u128, denom: &str) -> Self {
        TestRegisterPayable {
            info: mock_info(sender, &[coin(amount, denom)]),
            ..Default::default()
        }
    }

    pub fn default_with_coin(amount: u128, denom: &str) -> Self {
        Self::default_full_sender(DEFAULT_INFO_NAME, amount, denom)
    }

    pub fn default_with_sender(sender: &str) -> Self {
        Self::default_full_sender(sender, 100, DEFAULT_ONBOARDING_DENOM)
    }

    pub fn default_with_amount(amount: u128) -> Self {
        Self::default_full_sender(DEFAULT_INFO_NAME, amount, DEFAULT_ONBOARDING_DENOM)
    }

    pub fn default_with_denom(denom: &str) -> Self {
        Self::default_full_sender(DEFAULT_INFO_NAME, 100, denom)
    }
}
impl Default for TestRegisterPayable {
    fn default() -> Self {
        TestRegisterPayable {
            info: mock_info(
                DEFAULT_INFO_NAME,
                &[coin(100, DEFAULT_ONBOARDING_DENOM.to_string())],
            ),
            contract_name: DEFAULT_CONTRACT_NAME.to_string(),
            register_payable: TestRegisterPayable::default_register_payable(),
        }
    }
}

pub fn test_register_payable(
    deps: &mut MockOwnedDeps,
    provenance_util: &MockProvenanceUtil,
    msg: TestRegisterPayable,
) -> Result<Response<ProvenanceMsg>, ContractError> {
    let response = register_payable_with_util(
        deps.as_mut(),
        provenance_util,
        msg.info,
        msg.register_payable,
    );
    provenance_util.bind_captured_attribute_named(deps, msg.contract_name);
    response
}
