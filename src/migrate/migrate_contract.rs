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
    use crate::core::msg::MigrateMsg;
    use crate::migrate::migrate_contract::migrate_contract;
    use crate::migrate::version_info::{
        get_version_info, set_version_info, VersionInfoV1, CONTRACT_NAME, CONTRACT_VERSION,
    };
    use crate::util::constants::{MIGRATION_CONTRACT_NAME, MIGRATION_CONTRACT_VERSION};
    use provwasm_mocks::mock_dependencies;

    #[test]
    fn test_successful_migration() {
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
            response
                .attributes
                .iter()
                .find(|attr| attr.key == MIGRATION_CONTRACT_NAME.to_string())
                .unwrap()
                .value
                .as_str(),
            "the contract name should be stored in the migration attribute output",
        );
        assert_eq!(
            CONTRACT_VERSION,
            response
                .attributes
                .iter()
                .find(|attr| attr.key == MIGRATION_CONTRACT_VERSION.to_string())
                .unwrap()
                .value
                .as_str(),
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
}
