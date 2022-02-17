use crate::core::error::ContractError;
use crate::core::msg::MigrateMsg;
use crate::migrate::version_info::{
    get_version_info, migrate_version_info, CONTRACT_NAME, CONTRACT_VERSION,
};
use crate::util::constants::{MIGRATION_CONTRACT_NAME, MIGRATION_CONTRACT_VERSION};
use cosmwasm_std::{DepsMut, Response, Storage};
use provwasm_std::ProvenanceQuery;
use semver::Version;

pub fn migrate_contract(
    deps: DepsMut<ProvenanceQuery>,
    _msg: MigrateMsg,
) -> Result<Response, ContractError> {
    check_valid_migration_versioning(deps.storage)?;
    // Ensure that the new contract version is stored for future migrations to reference
    let new_version_info = migrate_version_info(deps.storage)?;
    Ok(Response::new()
        .add_attribute(MIGRATION_CONTRACT_NAME, &new_version_info.contract)
        .add_attribute(MIGRATION_CONTRACT_VERSION, &new_version_info.version))
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

#[cfg(test)]
mod tests {
    use crate::core::error::ContractError;
    use crate::core::msg::MigrateMsg;
    use crate::migrate::migrate_contract::migrate_contract;
    use crate::migrate::version_info::{
        get_version_info, set_version_info, VersionInfoV1, CONTRACT_NAME, CONTRACT_VERSION,
    };
    use crate::testutil::test_utilities::{single_attribute_for_key, test_instantiate, InstArgs};
    use crate::util::constants::{MIGRATION_CONTRACT_NAME, MIGRATION_CONTRACT_VERSION};
    use provwasm_mocks::mock_dependencies;

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
        let response = migrate_contract(deps.as_mut(), MigrateMsg {}).unwrap();
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
        let response = migrate_contract(deps.as_mut(), MigrateMsg {}).unwrap();
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
        match migrate_contract(deps.as_mut(), MigrateMsg {}).unwrap_err() {
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
        match migrate_contract(deps.as_mut(), MigrateMsg {}).unwrap_err() {
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
