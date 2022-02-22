use cosmwasm_std::{MessageInfo, Response};
use cosmwasm_std::testing::mock_info;
use provwasm_std::ProvenanceMsg;
use crate::core::error::ContractError;
use crate::execute::oracle_approval::{oracle_approval_with_util, OracleApprovalV1};
use crate::testutil::mock_provenance_util::MockProvenanceUtil;
use crate::testutil::test_utilities::{DEFAULT_CONTRACT_NAME, DEFAULT_ORACLE_ADDRESS, DEFAULT_PAYABLE_UUID, MockOwnedDeps};

pub struct TestOracleApproval {
    pub info: MessageInfo,
    pub contract_name: String,
    pub oracle_approval: OracleApprovalV1,
}
impl TestOracleApproval {
    pub fn default_oracle_approval() -> OracleApprovalV1 {
        OracleApprovalV1 {
            payable_uuid: DEFAULT_PAYABLE_UUID.to_string(),
        }
    }
}
impl Default for TestOracleApproval {
    fn default() -> Self {
        TestOracleApproval {
            // Bind the default oracle address as the sender - it should match the oracle address
            // that was bound to the scope attribute
            info: mock_info(DEFAULT_ORACLE_ADDRESS, &[]),
            contract_name: DEFAULT_CONTRACT_NAME.to_string(),
            oracle_approval: TestOracleApproval::default_oracle_approval(),
        }
    }
}

pub fn test_oracle_approval(
    deps: &mut MockOwnedDeps,
    provenance_util: &MockProvenanceUtil,
    msg: TestOracleApproval,
) -> Result<Response<ProvenanceMsg>, ContractError> {
    let response = oracle_approval_with_util(deps.as_mut(), provenance_util, msg.info, msg.oracle_approval);
    provenance_util.bind_captured_attribute_named(deps, msg.contract_name);
    response
}
