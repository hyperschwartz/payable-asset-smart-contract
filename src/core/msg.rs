use crate::core::error::ContractError;
use cosmwasm_std::{CustomQuery, Decimal, Deps, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::state::{StateV2};
use crate::execute::make_payment::MakePaymentV1;
use crate::execute::oracle_approval::OracleApprovalV1;
use crate::execute::register_payable::RegisterPayableV2;
use crate::migrate::migrate_contract::{MigrateContractV1, MigrateContractV2};
use crate::util::conversions::to_uint128;
use crate::util::traits::ValidatedMsg;

/// A message sent to initialize the contract state.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InitMsg {
    // Name of the contract that is tagged on various things
    pub contract_name: String,
    // Cost to onboard each payable
    pub onboarding_cost: String,
    // Coin type for onboarding charge
    pub onboarding_denom: String,
    // The address that will collect onboarding fees
    pub fee_collection_address: String,
    // Percentage of each transaction that is taken as fee
    pub fee_percent: Decimal,
    // Whether or not this contract should have assistance for local environments
    pub is_local: Option<bool>,
}
impl ValidatedMsg for InitMsg {
    fn validate(&self) -> Result<(), ContractError> {
        let mut invalid_fields: Vec<&str> = vec![];
        if self.contract_name.is_empty() {
            invalid_fields.push("contract_name");
        }
        if self.onboarding_cost.parse::<u128>().is_err() {
            invalid_fields.push("onboarding_cost");
        }
        if self.onboarding_denom.is_empty() {
            invalid_fields.push("onboarding_denom");
        }
        if self.fee_collection_address.is_empty() {
            invalid_fields.push("fee_collection_address");
        }
        if self.fee_percent > Decimal::one() {
            invalid_fields.push("fee_percent");
        }
        if !invalid_fields.is_empty() {
            ContractError::invalid_fields(invalid_fields).to_result()
        } else {
            Ok(())
        }
    }
}

/// A message sent to register a name with the name service
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    RegisterPayable {
        payable_type: String,
        payable_uuid: String,
        scope_id: String,
        oracle_address: String,
        payable_denom: String,
        payable_total: Uint128,
    },
    OracleApproval {
        payable_uuid: String,
    },
    MakePayment {
        payable_uuid: String,
    },
}
impl ExecuteMsg {
    pub fn to_register_payable(self) -> Result<RegisterPayableV2, ContractError> {
        match self {
            ExecuteMsg::RegisterPayable {
                payable_type,
                payable_uuid,
                scope_id,
                oracle_address,
                payable_denom,
                payable_total,
            } => Ok(RegisterPayableV2 {
                payable_type,
                payable_uuid,
                scope_id,
                oracle_address,
                payable_denom,
                payable_total,
            }),
            _ => ContractError::std_err("expected RegisterPayable message type"),
        }
    }
    pub fn to_oracle_approval(self) -> Result<OracleApprovalV1, ContractError> {
        match self {
            ExecuteMsg::OracleApproval { payable_uuid } => Ok(OracleApprovalV1 { payable_uuid }),
            _ => ContractError::std_err("expected OracleApproval message type"),
        }
    }
    pub fn to_make_payment(self) -> Result<MakePaymentV1, ContractError> {
        match self {
            ExecuteMsg::MakePayment { payable_uuid } => Ok(MakePaymentV1 { payable_uuid }),
            _ => ContractError::std_err("expected MakePayment message type"),
        }
    }
}
impl ValidatedMsg for ExecuteMsg {
    fn validate(&self) -> Result<(), ContractError> {
        let mut invalid_fields: Vec<&str> = vec![];
        match self {
            ExecuteMsg::RegisterPayable {
                payable_type,
                payable_uuid,
                scope_id,
                oracle_address,
                payable_denom,
                payable_total,
            } => {
                if payable_type.is_empty() {
                    invalid_fields.push("payable_type");
                }
                if payable_uuid.is_empty() {
                    invalid_fields.push("payable_uuid");
                }
                if scope_id.is_empty() {
                    invalid_fields.push("scope_id");
                }
                if oracle_address.is_empty() {
                    invalid_fields.push("oracle_address");
                }
                if payable_denom.is_empty() {
                    invalid_fields.push("payable_denom");
                }
                if payable_total.u128() == 0 {
                    invalid_fields.push("payable_total");
                }
            }
            ExecuteMsg::OracleApproval { payable_uuid } => {
                if payable_uuid.is_empty() {
                    invalid_fields.push("payable_uuid");
                }
            }
            ExecuteMsg::MakePayment { payable_uuid } => {
                if payable_uuid.is_empty() {
                    invalid_fields.push("payable_uuid");
                }
            }
        };
        if !invalid_fields.is_empty() {
            ContractError::invalid_fields(invalid_fields).to_result()
        } else {
            Ok(())
        }
    }
}

/// A message sent to query contract config state.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    QueryState {},
    QueryPayableByScopeId { scope_id: String },
    QueryPayableByUuid { payable_uuid: String },
}
impl ValidatedMsg for QueryMsg {
    fn validate(&self) -> Result<(), ContractError> {
        let mut invalid_fields: Vec<&str> = vec![];
        match self {
            QueryMsg::QueryState {} => (),
            QueryMsg::QueryPayableByScopeId { scope_id } => {
                if scope_id.is_empty() {
                    invalid_fields.push("scope_id");
                }
            },
            QueryMsg::QueryPayableByUuid { payable_uuid } => {
                if payable_uuid.is_empty() {
                    invalid_fields.push("payable_uuid");
                }
            },
        };
        if !invalid_fields.is_empty() {
            ContractError::invalid_fields(invalid_fields).to_result()
        } else {
            Ok(())
        }
    }
}

/// A type alias for contract state.
pub type QueryResponse = StateV2;

/// Migrate the contract
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct MigrateMsg {
    pub onboarding_cost: Option<String>,
    pub onboarding_denom: Option<String>,
    pub fee_collection_address: Option<String>,
    pub fee_percent: Option<Decimal>,
    pub oracle_address: Option<String>,
}
impl ValidatedMsg for MigrateMsg {
    fn validate(&self) -> Result<(), ContractError> {
        let mut invalid_fields: Vec<&str> = vec![];
        if let Some(cost) = &self.onboarding_cost {
            if cost.parse::<u128>().is_err() {
                invalid_fields.push("onboarding_cost");
            }
        }
        if let Some(denom) = &self.onboarding_denom {
            if denom.is_empty() {
                invalid_fields.push("onboarding_denom");
            }
        }
        if let Some(fee_addr) = &self.fee_collection_address {
            if fee_addr.is_empty() {
                invalid_fields.push("fee_collection_address");
            }
        }
        if let Some(fee_percent) = &self.fee_percent {
            if fee_percent > &Decimal::one() {
                invalid_fields.push("fee_percent");
            }
        }
        if let Some(addr) = &self.oracle_address {
            if addr.is_empty() {
                invalid_fields.push("oracle_address");
            }
        }
        if !invalid_fields.is_empty() {
            ContractError::invalid_fields(invalid_fields).to_result()
        } else {
            Ok(())
        }
    }
}
impl MigrateMsg {
    /// Converts the message from a contract message into a parsed and converted message for
    /// processing in downstream migration code.
    pub fn to_migrate_contract_v1<T: CustomQuery>(
        self,
        deps: &Deps<T>,
    ) -> Result<MigrateContractV1, ContractError> {
        // Unbox mapped fields, convert and validate, and re-box if necessary. Otherwise,
        // pass-through to None
        let onboarding_cost = if let Some(cost) = self.onboarding_cost {
            Some(to_uint128(cost)?)
        } else {
            None
        };
        let fee_collection_address = if let Some(fee_addr) = self.fee_collection_address {
            Some(deps.api.addr_validate(fee_addr.as_str())?)
        } else {
            None
        };
        let oracle_address = if let Some(oracle_addr) = self.oracle_address {
            Some(deps.api.addr_validate(oracle_addr.as_str())?)
        } else {
            None
        };
        Ok(MigrateContractV1 {
            onboarding_cost,
            onboarding_denom: self.onboarding_denom,
            fee_collection_address,
            fee_percent: self.fee_percent,
            oracle_address,
        })
    }

    pub fn to_migrate_contract_v2<T: CustomQuery>(
        self,
        deps: &Deps<T>,
    ) -> Result<MigrateContractV2, ContractError> {
        // Unbox mapped fields, convert and validate, and re-box if necessary. Otherwise,
        // pass-through to None
        let onboarding_cost = if let Some(cost) = self.onboarding_cost {
            Some(to_uint128(cost)?)
        } else {
            None
        };
        let fee_collection_address = if let Some(fee_addr) = self.fee_collection_address {
            Some(deps.api.addr_validate(fee_addr.as_str())?)
        } else {
            None
        };
        Ok(MigrateContractV2 {
            onboarding_cost,
            onboarding_denom: self.onboarding_denom,
            fee_collection_address,
            fee_percent: self.fee_percent,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::core::error::ContractError;
    use crate::core::msg::ExecuteMsg::{MakePayment, OracleApproval};
    use crate::core::msg::QueryMsg::{QueryPayableByUuid, QueryState};
    use crate::core::msg::{ExecuteMsg, InitMsg, MigrateMsg};
    use crate::util::traits::ValidatedMsg;
    use cosmwasm_std::{Decimal, Uint128};
    use provwasm_mocks::mock_dependencies;

    #[test]
    fn test_valid_init_msg() {
        get_valid_init_msg()
            .validate()
            .expect("a populated init msg should pass validation");
    }

    #[test]
    fn test_invalid_init_msg_contract_name() {
        let mut msg = get_valid_init_msg();
        // Empty string bad
        msg.contract_name = String::new();
        test_invalid_msg(&msg, "contract_name");
    }

    #[test]
    fn test_invalid_init_msg_onboarding_cost() {
        let mut msg = get_valid_init_msg();
        // Non-numbers bad
        msg.onboarding_cost = "word".to_string();
        test_invalid_msg(&msg, "onboarding_cost");
        // Negative numbers bad
        msg.onboarding_cost = "-1".to_string();
        test_invalid_msg(&msg, "onboarding_cost");
    }

    #[test]
    fn test_invalid_init_msg_fee_collection_address() {
        let mut msg = get_valid_init_msg();
        // Empty string bad
        msg.fee_collection_address = String::new();
        test_invalid_msg(&msg, "fee_collection_address");
    }

    #[test]
    fn test_invalid_init_msg_fee_percent() {
        let mut msg = get_valid_init_msg();
        // Over 100% bad
        msg.fee_percent = Decimal::percent(101);
        test_invalid_msg(&msg, "fee_percent");
    }

    #[test]
    fn test_valid_execute_register_payable() {
        get_valid_register_payable()
            .to_enum()
            .validate()
            .expect("a populated register payable should pass validation");
    }

    #[test]
    fn test_invalid_execute_register_payable_payable_type() {
        let mut msg = get_valid_register_payable();
        // Empty string bad
        msg.payable_type = String::new();
        test_invalid_msg(&msg.to_enum(), "payable_type");
    }

    #[test]
    fn test_invalid_execute_register_payable_payable_uuid() {
        let mut msg = get_valid_register_payable();
        // Empty string bad
        msg.payable_uuid = String::new();
        test_invalid_msg(&msg.to_enum(), "payable_uuid");
    }

    #[test]
    fn test_invalid_execute_register_payable_scope_id() {
        let mut msg = get_valid_register_payable();
        // Empty string bad
        msg.scope_id = String::new();
        test_invalid_msg(&msg.to_enum(), "scope_id");
    }

    #[test]
    fn test_invalid_execute_register_payable_payable_denom() {
        let mut msg = get_valid_register_payable();
        // Empty string bad
        msg.payable_denom = String::new();
        test_invalid_msg(&msg.to_enum(), "payable_denom");
    }

    #[test]
    fn test_invalid_execute_register_payable_payable_total() {
        let mut msg = get_valid_register_payable();
        // Zero bad
        msg.payable_total = Uint128::zero();
        test_invalid_msg(&msg.to_enum(), "payable_total");
    }

    #[test]
    fn test_valid_execute_oracle_approval() {
        OracleApproval {
            payable_uuid: "d6219342-8f82-11ec-a7cf-1fe3b2eb3267".to_string(),
        }
        .validate()
        .expect("a populated oracle approval should pass validation");
    }

    #[test]
    fn test_invalid_execute_oracle_approval_payable_uuid() {
        test_invalid_msg(
            &OracleApproval {
                payable_uuid: String::new(),
            },
            "payable_uuid",
        );
    }

    #[test]
    fn test_valid_execute_make_payment() {
        MakePayment {
            payable_uuid: "07933e94-8f83-11ec-a3e4-dbff515bf8c5".to_string(),
        }
        .validate()
        .expect("a populated make payment msg should pass validation");
    }

    #[test]
    fn test_invalid_execute_make_payment_payable_uuid() {
        test_invalid_msg(
            &MakePayment {
                payable_uuid: String::new(),
            },
            "payable_uuid",
        );
    }

    #[test]
    fn test_valid_query_query_state() {
        QueryState {}
            .validate()
            .expect("a populated query state msg should pass validation");
    }

    #[test]
    fn test_valid_query_query_payable_by_uuid() {
        QueryPayableByUuid {
            payable_uuid: "3ee3a636-8f83-11ec-8c26-6b8cbb24f4aa".to_string(),
        }
        .validate()
        .expect("a populated query payable should pass validation");
    }

    #[test]
    fn test_invalid_query_query_by_uuid_payable_uuid() {
        test_invalid_msg(
            &QueryPayableByUuid {
                payable_uuid: String::new(),
            },
            "payable_uuid",
        );
    }

    #[test]
    fn test_valid_migrate() {
        MigrateMsg {
            onboarding_cost: None,
            onboarding_denom: None,
            fee_collection_address: None,
            fee_percent: None,
            oracle_address: None,
        }
        .validate()
        .expect("a migrate msg with no fields populated should pass validation");
        get_valid_migrate_msg()
            .validate()
            .expect("a migrate msg with all fields populated should pass validation");
    }

    #[test]
    fn test_invalid_migrate_onboarding_cost() {
        let mut msg = get_valid_migrate_msg();
        // NaN bad
        msg.onboarding_cost = Some("not a number".to_string());
        test_invalid_msg(&msg, "onboarding_cost");
        // Below zero bad
        msg.onboarding_cost = Some("-1".to_string());
        test_invalid_msg(&msg, "onboarding_cost");
    }

    #[test]
    fn test_invalid_migrate_onboarding_denom() {
        let mut msg = get_valid_migrate_msg();
        // Empty string bad
        msg.onboarding_denom = Some(String::new());
        test_invalid_msg(&msg, "onboarding_denom");
    }

    #[test]
    fn test_invalid_migrate_fee_collection_address() {
        let mut msg = get_valid_migrate_msg();
        // Empty string bad
        msg.fee_collection_address = Some(String::new());
        test_invalid_msg(&msg, "fee_collection_address");
    }

    #[test]
    fn test_invalid_migrate_fee_percent() {
        let mut msg = get_valid_migrate_msg();
        // Above 100 percent bad
        msg.fee_percent = Some(Decimal::percent(101));
        test_invalid_msg(&msg, "fee_percent");
    }

    #[test]
    fn test_invalid_migrate_oracle_address() {
        let mut msg = get_valid_migrate_msg();
        // Empty string bad
        msg.oracle_address = Some(String::new());
        test_invalid_msg(&msg, "oracle_address");
    }

    #[test]
    fn test_invalid_migration_to_v1_conversion_onboarding_cost() {
        let deps = mock_dependencies(&[]);
        let mut msg = get_valid_migrate_msg();
        msg.onboarding_cost = Some("not a number".to_string());
        assert!(msg.to_migrate_contract_v1(&deps.as_ref()).is_err());
    }

    #[test]
    fn test_invalid_migration_to_v1_conversion_fee_collection_address() {
        let deps = mock_dependencies(&[]);
        let mut msg = get_valid_migrate_msg();
        msg.fee_collection_address = Some(String::new());
        assert!(msg.to_migrate_contract_v1(&deps.as_ref()).is_err());
    }

    #[test]
    fn test_invalid_migration_to_v1_conversion_oracle_address() {
        let deps = mock_dependencies(&[]);
        let mut msg = get_valid_migrate_msg();
        msg.oracle_address = Some(String::new());
        assert!(msg.to_migrate_contract_v1(&deps.as_ref()).is_err());
    }

    fn get_valid_init_msg() -> InitMsg {
        InitMsg {
            contract_name: "test".to_string(),
            onboarding_cost: "100".to_string(),
            onboarding_denom: "nhash".to_string(),
            fee_collection_address: "addr".to_string(),
            fee_percent: Decimal::percent(50),
            is_local: Some(true),
        }
    }

    struct RegisterPayableBuilder {
        payable_type: String,
        payable_uuid: String,
        scope_id: String,
        oracle_address: String,
        payable_denom: String,
        payable_total: Uint128,
    }
    impl RegisterPayableBuilder {
        fn to_enum(self) -> ExecuteMsg {
            ExecuteMsg::RegisterPayable {
                payable_type: self.payable_type,
                payable_uuid: self.payable_uuid,
                scope_id: self.scope_id,
                oracle_address: self.oracle_address,
                payable_denom: self.payable_denom,
                payable_total: self.payable_total,
            }
        }
    }

    fn get_valid_register_payable() -> RegisterPayableBuilder {
        RegisterPayableBuilder {
            payable_type: "test".to_string(),
            payable_uuid: "86c224de-8f81-11ec-9277-0353b82d7772".to_string(),
            scope_id: "scope".to_string(),
            oracle_address: "oracle-addr".to_string(),
            payable_denom: "nhash".to_string(),
            payable_total: Uint128::new(128),
        }
    }

    fn get_valid_migrate_msg() -> MigrateMsg {
        MigrateMsg {
            onboarding_cost: Some("100".to_string()),
            onboarding_denom: Some("nhash".to_string()),
            fee_collection_address: Some("address".to_string()),
            fee_percent: Some(Decimal::percent(50)),
            oracle_address: Some("address".to_string()),
        }
    }

    fn test_invalid_msg(msg: &dyn ValidatedMsg, expected_bad_field: &str) {
        let err = msg.validate().unwrap_err();
        match err {
            ContractError::InvalidFields { fields } => {
                assert!(
                    fields.contains(&expected_bad_field.to_string()),
                    "expected field {} to be contained in errored fields, but found fields {:?}",
                    expected_bad_field,
                    fields,
                )
            }
            _ => panic!("unexpected contract error type for invalid fields"),
        }
    }
}
