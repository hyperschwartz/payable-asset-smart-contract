use crate::core::error::ContractError;
use crate::execute::make_payment::{make_payment_with_util, MakePaymentV1};
use crate::testutil::mock_provenance_util::MockProvenanceUtil;
use crate::testutil::test_utilities::{
    MockOwnedDeps, DEFAULT_CONTRACT_NAME, DEFAULT_INFO_NAME, DEFAULT_PAYABLE_DENOM,
    DEFAULT_PAYABLE_TOTAL, DEFAULT_PAYABLE_UUID,
};
use cosmwasm_std::testing::mock_info;
use cosmwasm_std::{coin, MessageInfo, Response};
use provwasm_std::ProvenanceMsg;

pub struct TestMakePayment {
    pub info: MessageInfo,
    pub contract_name: String,
    pub make_payment: MakePaymentV1,
}
impl TestMakePayment {
    pub fn default_make_payment() -> MakePaymentV1 {
        MakePaymentV1 {
            payable_uuid: DEFAULT_PAYABLE_UUID.to_string(),
        }
    }
    pub fn default_full_sender(sender: &str, amount: u128, denom: &str) -> Self {
        TestMakePayment {
            info: mock_info(sender, &[coin(amount, denom)]),
            ..Default::default()
        }
    }
    pub fn default_with_coin(amount: u128, denom: &str) -> Self {
        Self::default_full_sender(DEFAULT_INFO_NAME, amount, denom)
    }
    pub fn default_with_sender(sender: &str) -> Self {
        Self::default_full_sender(sender, DEFAULT_PAYABLE_TOTAL, DEFAULT_PAYABLE_DENOM)
    }
    pub fn default_with_amount(amount: u128) -> Self {
        Self::default_full_sender(DEFAULT_INFO_NAME, amount, DEFAULT_PAYABLE_DENOM)
    }
    pub fn default_with_denom(denom: &str) -> Self {
        Self::default_full_sender(DEFAULT_INFO_NAME, DEFAULT_PAYABLE_TOTAL, denom)
    }
}
impl Default for TestMakePayment {
    fn default() -> Self {
        TestMakePayment {
            info: mock_info(
                DEFAULT_INFO_NAME,
                &[coin(DEFAULT_PAYABLE_TOTAL, DEFAULT_PAYABLE_DENOM)],
            ),
            contract_name: DEFAULT_CONTRACT_NAME.to_string(),
            make_payment: TestMakePayment::default_make_payment(),
        }
    }
}

pub fn test_make_payment(
    deps: &mut MockOwnedDeps,
    provenance_util: &MockProvenanceUtil,
    msg: TestMakePayment,
) -> Result<Response<ProvenanceMsg>, ContractError> {
    let response =
        make_payment_with_util(deps.as_mut(), provenance_util, msg.info, msg.make_payment);
    provenance_util.bind_captured_attribute_named(deps, msg.contract_name);
    response
}
