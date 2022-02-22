use crate::core::error::ContractError;
use crate::core::state::PayableScopeAttribute;
use crate::query::query_payable_by_scope_id::query_payable_attribute_by_scope_id;
use cosmwasm_std::{Addr, CosmosMsg, Deps, QuerierWrapper, StdResult};
use provwasm_std::{
    add_json_attribute, delete_attributes, ProvenanceMsg, ProvenanceQuerier, ProvenanceQuery, Scope,
};

pub trait ProvenanceUtil {
    fn get_scope_by_id(
        &self,
        querier: &QuerierWrapper<ProvenanceQuery>,
        scope_id: impl Into<String>,
    ) -> StdResult<Scope>;

    fn get_add_initial_attribute_to_scope_msg(
        &self,
        deps: &Deps<ProvenanceQuery>,
        attribute: &PayableScopeAttribute,
        contract_name: impl Into<String>,
    ) -> Result<CosmosMsg<ProvenanceMsg>, ContractError>;

    fn upsert_attribute_to_scope(
        &self,
        attribute: &PayableScopeAttribute,
        contract_name: impl Into<String>,
    ) -> Result<WriteAttributeMessages, ContractError>;
}

pub struct ProvenanceUtilImpl;

impl ProvenanceUtil for ProvenanceUtilImpl {
    fn get_scope_by_id(
        &self,
        querier: &QuerierWrapper<ProvenanceQuery>,
        scope_id: impl Into<String>,
    ) -> StdResult<Scope> {
        ProvenanceQuerier::new(querier).get_scope(scope_id)
    }

    fn get_add_initial_attribute_to_scope_msg(
        &self,
        deps: &Deps<ProvenanceQuery>,
        attribute: &PayableScopeAttribute,
        contract_name: impl Into<String>,
    ) -> Result<CosmosMsg<ProvenanceMsg>, ContractError> {
        if query_payable_attribute_by_scope_id(deps, &attribute.scope_id).is_ok() {
            return ContractError::DuplicateRegistration {
                scope_id: attribute.scope_id.clone(),
            }
            .to_result();
        }
        super::provenance_util::get_add_attribute_to_scope_msg(attribute, contract_name)
    }

    fn upsert_attribute_to_scope(
        &self,
        attribute: &PayableScopeAttribute,
        contract_name: impl Into<String>,
    ) -> Result<WriteAttributeMessages, ContractError> {
        let contract_name = contract_name.into();
        let delete_attributes_msg =
            delete_attributes(Addr::unchecked(&attribute.scope_id), &contract_name)
                .map_err(ContractError::Std)?;
        let add_attribute_msg =
            super::provenance_util::get_add_attribute_to_scope_msg(attribute, &contract_name)?;
        Ok(WriteAttributeMessages {
            delete_attributes_msg,
            add_attribute_msg,
        })
    }
}

fn get_add_attribute_to_scope_msg(
    attribute: &PayableScopeAttribute,
    contract_name: impl Into<String>,
) -> Result<CosmosMsg<ProvenanceMsg>, ContractError> {
    add_json_attribute(
        // Until there's a way to parse a scope address as an Addr, we must use Addr::unchecked.
        // It's not the best policy, but contract execution will fail if it's an incorrect address,
        // so it'll just fail later down the line with a less sane error message than if it was
        // being properly checked.
        Addr::unchecked(&attribute.scope_id),
        contract_name,
        attribute,
    )
    .map_err(ContractError::Std)
}

pub struct WriteAttributeMessages {
    delete_attributes_msg: CosmosMsg<ProvenanceMsg>,
    add_attribute_msg: CosmosMsg<ProvenanceMsg>,
}
impl WriteAttributeMessages {
    pub fn to_vec(self) -> Vec<CosmosMsg<ProvenanceMsg>> {
        vec![self.delete_attributes_msg, self.add_attribute_msg]
    }
}