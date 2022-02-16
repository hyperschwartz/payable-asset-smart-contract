use crate::core::error::ContractError;
use crate::core::msg::MigrateMsg;
use crate::migrate::version_info::{get_version_info, CONTRACT_NAME, CONTRACT_VERSION};
use cosmwasm_std::{DepsMut, Response, Storage};
use provwasm_std::ProvenanceQuery;
use semver::Version;

pub fn migrate_contract(
    deps: DepsMut<ProvenanceQuery>,
    _msg: MigrateMsg,
) -> Result<Response, ContractError> {
    check_valid_migration_versioning(deps.storage)?;
    Ok(Response::new())
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
