use crate::core::error::ContractError;
use crate::core::state::{
    config, config_v2, payable_meta_storage_read, payable_meta_storage_v2, PayableMeta,
    PayableMetaV2, PayableScopeAttribute, StateV2,
};
use crate::migrate::version_info::{
    get_version_info, migrate_version_info, CONTRACT_NAME, CONTRACT_VERSION,
};
use crate::util::constants::{
    MIGRATION_CONTRACT_NAME, MIGRATION_CONTRACT_VERSION, MIGRATION_STATE_CHANGE_PREFIX,
};
use crate::util::provenance_util::get_add_attribute_to_scope_msg;
use cosmwasm_std::{
    Addr, Attribute, CosmosMsg, Decimal, DepsMut, Order, Record, Response, StdResult, Storage,
    Uint128,
};
use provwasm_std::{ProvenanceMsg, ProvenanceQuery};
use schemars::JsonSchema;
use semver::Version;
use serde::{Deserialize, Serialize};

/// This struct contains all optional values required for migrating the contract.  Its values are
/// derived via the MigrateMsg's helper functions (found in core/msg.rs).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct MigrateContractV2 {
    pub onboarding_cost: Option<Uint128>,
    pub onboarding_denom: Option<String>,
    pub fee_collection_address: Option<Addr>,
    pub fee_percent: Option<Decimal>,
}
impl MigrateContractV2 {
    /// Helper to derive an empty message for testing purposes.
    pub fn empty() -> MigrateContractV2 {
        MigrateContractV2 {
            onboarding_cost: None,
            onboarding_denom: None,
            fee_collection_address: None,
            fee_percent: None,
        }
    }

    /// Helper function to make checks for whether or not any optional fields are provided more
    /// concise.  Useful in testing and to keep the migration code cleaner.
    pub fn has_state_changes(&self) -> bool {
        self.onboarding_cost.is_some()
            || self.onboarding_denom.is_some()
            || self.fee_collection_address.is_some()
            || self.fee_percent.is_some()
    }
}

/// Migrates the contract to a new version, utilizing the values within the msg param to determine
/// which fields in the app state to change.
pub fn migrate_contract(
    deps: DepsMut<ProvenanceQuery>,
    migrate: MigrateContractV2,
) -> Result<Response<ProvenanceMsg>, ContractError> {
    // Ensure the provided version info stored in the contract is valid for the migration before
    // attempting any contract modifications
    check_valid_migration_versioning(deps.storage)?;
    let mut attributes: Vec<Attribute> = vec![];
    // Only load and modify the state if any optional values were provided during the migration
    if migrate.has_state_changes() {
        let mut contract_config = config_v2(deps.storage);
        let mut state = contract_config.load()?;
        // Conditionally modify each portion of the state that has a requested change
        if let Some(cost) = migrate.onboarding_cost {
            attributes.push(state_change_attribute("onboarding_cost", &cost.to_string()));
            state.onboarding_cost = cost;
        }
        if let Some(denom) = migrate.onboarding_denom {
            attributes.push(state_change_attribute("onboarding_denom", &denom));
            state.onboarding_denom = denom;
        }
        if let Some(fee_addr) = migrate.fee_collection_address {
            attributes.push(state_change_attribute(
                "fee_collection_address",
                &fee_addr.to_string(),
            ));
            state.fee_collection_address = fee_addr;
        }
        if let Some(fee_percent) = migrate.fee_percent {
            attributes.push(state_change_attribute(
                "fee_percent",
                &fee_percent.to_string(),
            ));
            state.fee_percent = fee_percent;
        }
        // Persist all changes to the state after modifying them within this block
        contract_config.save(&state)?;
    }
    // Ensure that the new contract version is stored for future migrations to reference
    let new_version_info = migrate_version_info(deps.storage)?;
    // Append attributes that indicate the contract name and version to which the migration brings the contract
    attributes.push(Attribute::new(
        MIGRATION_CONTRACT_NAME,
        &new_version_info.contract,
    ));
    attributes.push(Attribute::new(
        MIGRATION_CONTRACT_VERSION,
        &new_version_info.version,
    ));
    Ok(Response::new().add_attributes(attributes))
}

fn state_change_attribute(field_name: impl Into<String>, value: impl Into<String>) -> Attribute {
    Attribute::new(state_change_attr_name(field_name), value)
}

fn state_change_attr_name(field_name: impl Into<String>) -> String {
    format!("{}{}", MIGRATION_STATE_CHANGE_PREFIX, field_name.into())
}

fn check_valid_migration_versioning(storage: &mut dyn Storage) -> Result<(), ContractError> {
    let stored_version_info = get_version_info(storage)?;
    // If the contract name has changed or another contract attempts to overwrite this one, this
    // check will reject the change
    if CONTRACT_NAME != stored_version_info.contract {
        return ContractError::InvalidContractName {
            current_contract: stored_version_info.contract,
            migration_contract: CONTRACT_NAME.to_string(),
        }
        .to_result();
    }
    let contract_version = CONTRACT_VERSION.parse::<Version>()?;
    // If the stored version in the contract is greater than the derived version from the package,
    // then this migration is effectively a downgrade and should not be committed
    if stored_version_info.parse_sem_ver()? > contract_version {
        return ContractError::InvalidContractVersion {
            current_version: stored_version_info.version,
            migration_version: CONTRACT_VERSION.to_string(),
        }
        .to_result();
    }
    Ok(())
}

/// This migration assumes that there is no state v1
pub fn migrate_to_scope_attributes(
    deps: DepsMut<ProvenanceQuery>,
) -> Result<Response<ProvenanceMsg>, ContractError> {
    let state_v1 = config(deps.storage).load()?;
    let payable_type = state_v1.payable_type.clone();
    let oracle_address = state_v1.oracle_address.clone();
    let mut config_v2 = config_v2(deps.storage);
    if config_v2.load().is_ok() {
        return ContractError::InvalidMigration("state v2 found - no need to migrate".to_string())
            .to_result();
    }
    let state_v2 = StateV2 {
        contract_name: state_v1.contract_name,
        onboarding_cost: state_v1.onboarding_cost,
        onboarding_denom: state_v1.onboarding_denom,
        fee_collection_address: state_v1.fee_collection_address,
        fee_percent: state_v1.fee_percent,
        is_local: state_v1.is_local,
    };
    // Store the new v2 state
    config_v2.save(&state_v2)?;
    let mut messages: Vec<CosmosMsg<ProvenanceMsg>> = vec![];
    let mut meta_v2s: Vec<PayableMetaV2> = vec![];
    // Migrate all values in local storage to their appropriate scope attributes
    let attribute_storage_ids: Vec<Vec<u8>> = payable_meta_storage_read(deps.storage)
        .range(None, None, Order::Ascending)
        .map(|kv: StdResult<Record<PayableMeta>>| kv.unwrap().0)
        .collect();
    for storage_id in attribute_storage_ids {
        let payable_meta = payable_meta_storage_read(deps.storage).load(&storage_id)?;
        // Create a meta v2 that will store the uuid -> scope link for querying
        let meta_v2 = PayableMetaV2 {
            payable_uuid: payable_meta.payable_uuid.clone(),
            scope_id: payable_meta.scope_id.clone(),
        };
        // Store the v2 in a vector for saving all at once. Don't want to partially migrate if an error occurs
        meta_v2s.push(meta_v2);
        let scope_attribute = PayableScopeAttribute {
            payable_type: payable_type.clone(),
            payable_uuid: payable_meta.payable_uuid,
            scope_id: payable_meta.scope_id,
            oracle_address: oracle_address.clone(),
            payable_denom: payable_meta.payable_denom,
            payable_total_owed: payable_meta.payable_total_owed,
            payable_remaining_owed: payable_meta.payable_remaining_owed,
            oracle_approved: payable_meta.oracle_approved,
        };
        // Create and push a message that will add a json attribute to the scope
        messages.push(get_add_attribute_to_scope_msg(
            &scope_attribute,
            &state_v2.contract_name,
        )?);
    }
    let save_count = meta_v2s.len().to_string();
    for meta_v2 in meta_v2s {
        payable_meta_storage_v2(deps.storage).save(meta_v2.payable_uuid.as_bytes(), &meta_v2)?;
    }
    let message_count = messages.len().to_string();
    Ok(Response::new()
        .add_messages(messages)
        .add_attribute("payables_scope_attributes_added", message_count)
        .add_attribute("payables_metadata_saved", save_count))
}

#[cfg(test)]
mod tests {
    use crate::core::error::ContractError;
    use crate::core::state::config_read_v2;
    use crate::migrate::migrate_contract::{
        migrate_contract, state_change_attr_name, state_change_attribute, MigrateContractV2,
    };
    use crate::migrate::version_info::{
        get_version_info, set_version_info, VersionInfoV1, CONTRACT_NAME, CONTRACT_VERSION,
    };
    use crate::testutil::test_utilities::{single_attribute_for_key, test_instantiate, InstArgs};
    use crate::util::constants::{MIGRATION_CONTRACT_NAME, MIGRATION_CONTRACT_VERSION};
    use cosmwasm_std::{Addr, Decimal, Uint128};
    use provwasm_mocks::mock_dependencies;

    #[test]
    fn test_state_change_attr_name() {
        assert_eq!(
            "payable_migration_state_field_test_field",
            state_change_attr_name("test_field").as_str(),
            "the field name should be populated correctly",
        );
    }

    #[test]
    fn test_state_change_attribute() {
        let attribute = state_change_attribute("some_field", "120");
        assert_eq!(
            "payable_migration_state_field_some_field",
            attribute.key.as_str(),
            "the key should be formatted correctly",
        );
        assert_eq!(
            "120",
            attribute.value.as_str(),
            "the value should directly reflect the value passed into the function",
        );
    }

    #[test]
    fn test_migrate_msg_has_state_changes() {
        let mut msg = MigrateContractV2::empty();
        assert!(
            !msg.has_state_changes(),
            "an empty migrate contract v1 should not have state changes",
        );
        msg.onboarding_cost = Some(Uint128::new(100));
        assert!(
            msg.has_state_changes(),
            "onboarding cost including a value should cause state changes",
        );
        msg.onboarding_cost = None;
        msg.onboarding_denom = Some("nhash".to_string());
        assert!(
            msg.has_state_changes(),
            "onboarding denom including a value should cause state changes",
        );
        msg.onboarding_denom = None;
        msg.fee_collection_address = Some(Addr::unchecked("address"));
        assert!(
            msg.has_state_changes(),
            "fee collection address including a value should cause state changes",
        );
        msg.fee_collection_address = None;
        msg.fee_percent = Some(Decimal::percent(60));
        assert!(
            msg.has_state_changes(),
            "fee percent including a value should cause state changes",
        );
    }

    #[test]
    fn test_successful_upgrade_migration() {
        let mut deps = mock_dependencies(&[]);
        set_version_info(
            deps.as_mut().storage,
            &VersionInfoV1 {
                contract: CONTRACT_NAME.to_string(),
                version: "0.0.1".to_string(),
            },
        )
        .unwrap();
        let response = migrate_contract(deps.as_mut(), MigrateContractV2::empty()).unwrap();
        assert!(
            response.messages.is_empty(),
            "no messages should be sent on migrate"
        );
        assert_eq!(
            2,
            response.attributes.len(),
            "both migration attributes should be added"
        );
        assert_eq!(
            CONTRACT_NAME,
            single_attribute_for_key(&response, MIGRATION_CONTRACT_NAME),
            "the contract name should be stored in the migration attribute output",
        );
        assert_eq!(
            CONTRACT_VERSION,
            single_attribute_for_key(&response, MIGRATION_CONTRACT_VERSION),
            "the contract version should be stored in the migration attribute output",
        );
        let version_info = get_version_info(deps.as_ref().storage).unwrap();
        assert_eq!(
            CONTRACT_NAME,
            version_info.contract.as_str(),
            "the contract name should be updated to the correct value after the migration",
        );
        assert_eq!(
            CONTRACT_VERSION,
            version_info.version.as_str(),
            "the contract version should be updated to the correct value after the migration",
        );
    }

    #[test]
    fn test_successful_same_version_migration() {
        let mut deps = mock_dependencies(&[]);
        // Instantiate the contract, automatically setting the version and contract name.
        // This can be seen working correctly in init_contract.rs > test_valid_init test
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
        let response = migrate_contract(deps.as_mut(), MigrateContractV2::empty()).unwrap();
        assert!(
            response.messages.is_empty(),
            "no messages should be sent on migrate"
        );
        assert_eq!(
            2,
            response.attributes.len(),
            "both migration attributes should be added"
        );
        assert_eq!(
            CONTRACT_NAME,
            single_attribute_for_key(&response, MIGRATION_CONTRACT_NAME),
            "the contract name should be stored in the migration attribute output",
        );
        assert_eq!(
            CONTRACT_VERSION,
            single_attribute_for_key(&response, MIGRATION_CONTRACT_VERSION),
            "the contract version should be stored in the migration attribute output",
        );
        let version_info = get_version_info(deps.as_ref().storage).unwrap();
        assert_eq!(
            CONTRACT_NAME,
            version_info.contract.as_str(),
            "the contract name should be updated to the correct value after the migration",
        );
        assert_eq!(
            CONTRACT_VERSION,
            version_info.version.as_str(),
            "the contract version should be updated to the correct value after the migration",
        );
    }

    #[test]
    fn test_successful_state_change_migration() {
        let mut deps = mock_dependencies(&[]);
        test_instantiate(deps.as_mut(), InstArgs::default()).unwrap();
        let response = migrate_contract(
            deps.as_mut(),
            MigrateContractV2 {
                onboarding_cost: Some(Uint128::new(134)),
                onboarding_denom: Some("dogecoin".to_string()),
                fee_collection_address: Some(Addr::unchecked("new-fee-addr")),
                fee_percent: Some(Decimal::percent(12)),
            },
        )
        .unwrap();
        assert!(
            response.messages.is_empty(),
            "no messages should be sent on migrate"
        );
        assert_eq!(
            6,
            response.attributes.len(),
            "all migration attributes should be added because all fields were changed",
        );
        assert_eq!(
            "134",
            single_attribute_for_key(
                &response,
                state_change_attr_name("onboarding_cost").as_str()
            ),
            "the onboarding cost attribute should be added correctly",
        );
        assert_eq!(
            "dogecoin",
            single_attribute_for_key(
                &response,
                state_change_attr_name("onboarding_denom").as_str()
            ),
            "the onboarding denom attribute should be added correctly",
        );
        assert_eq!(
            "new-fee-addr",
            single_attribute_for_key(
                &response,
                state_change_attr_name("fee_collection_address").as_str()
            ),
            "the fee collection address attribute should be added correctly",
        );
        assert_eq!(
            "0.12",
            single_attribute_for_key(&response, state_change_attr_name("fee_percent").as_str()),
            "the fee percent attribute should be added correctly",
        );
        assert_eq!(
            CONTRACT_NAME,
            single_attribute_for_key(&response, MIGRATION_CONTRACT_NAME),
            "the contract name attribute should be added correctly",
        );
        assert_eq!(
            CONTRACT_VERSION,
            single_attribute_for_key(&response, MIGRATION_CONTRACT_VERSION),
            "the contract version attribute should be added correctly",
        );
        let state = config_read_v2(deps.as_ref().storage)
            .load()
            .expect("state should load properly");
        assert_eq!(
            Uint128::new(134),
            state.onboarding_cost,
            "onboarding cost should be properly updated in the state",
        );
        assert_eq!(
            "dogecoin",
            state.onboarding_denom.as_str(),
            "onboarding denom should be properly updated in the state",
        );
        assert_eq!(
            Addr::unchecked("new-fee-addr"),
            state.fee_collection_address,
            "fee collection address should be properly updated in the state",
        );
        assert_eq!(
            Decimal::percent(12),
            state.fee_percent,
            "fee percent should be properly updated in the state",
        );
    }

    #[test]
    fn test_failed_migration_for_incorrect_name() {
        let mut deps = mock_dependencies(&[]);
        set_version_info(
            deps.as_mut().storage,
            &VersionInfoV1 {
                contract: "Wrong name".to_string(),
                version: CONTRACT_VERSION.to_string(),
            },
        )
        .unwrap();
        match migrate_contract(deps.as_mut(), MigrateContractV2::empty()).unwrap_err() {
            ContractError::InvalidContractName {
                current_contract,
                migration_contract,
            } => {
                assert_eq!(
                    "Wrong name",
                    current_contract.as_str(),
                    "the current contract name should equate to the value stored in contract storage",
                );
                assert_eq!(
                    CONTRACT_NAME,
                    migration_contract.as_str(),
                    "the migration contract should be the env contract name",
                );
            }
            _ => panic!("unexpected error encountered"),
        };
    }

    #[test]
    fn test_failed_migration_for_too_low_version() {
        let mut deps = mock_dependencies(&[]);
        set_version_info(
            deps.as_mut().storage,
            &VersionInfoV1 {
                contract: CONTRACT_NAME.to_string(),
                version: "99.9.9".to_string(),
            },
        )
        .unwrap();
        match migrate_contract(deps.as_mut(), MigrateContractV2::empty()).unwrap_err() {
            ContractError::InvalidContractVersion {
                current_version,
                migration_version,
            } => {
                assert_eq!(
                    "99.9.9",
                    current_version.as_str(),
                    "the current contract version should equate to the value stored in contract storage",
                );
                assert_eq!(
                    CONTRACT_VERSION,
                    migration_version.as_str(),
                    "the migration contract version should equate to the env value",
                );
            }
            _ => panic!("unexpected error encountered"),
        };
    }
}
