use std::cell::{RefCell};
use cosmwasm_std::{CosmosMsg, Deps, QuerierWrapper, StdResult};
use provwasm_std::{ProvenanceMsg, ProvenanceQuery, Scope};
use crate::core::error::ContractError;
use crate::core::state::PayableScopeAttribute;
use crate::testutil::test_utilities::{mock_default_scope_attribute, mock_scope_attribute, MockOwnedDeps};
use crate::util::provenance_util::{ProvenanceUtil, ProvenanceUtilImpl, WriteAttributeMessages};

pub struct MockProvenanceUtil {
    captured_attributes: RefCell<Vec<PayableScopeAttribute>>,
}
impl MockProvenanceUtil {
    pub fn new() -> MockProvenanceUtil {
        MockProvenanceUtil { captured_attributes: RefCell::new(vec![]), }
    }

    fn add_attribute(&self, attribute: PayableScopeAttribute) {
        self.captured_attributes.borrow_mut().push(attribute);
    }
}
impl ProvenanceUtil for MockProvenanceUtil {
    fn get_scope_by_id(
        &self,
        querier: &QuerierWrapper<ProvenanceQuery>,
        scope_id: impl Into<String>,
    ) -> StdResult<Scope> {
        ProvenanceUtilImpl.get_scope_by_id(querier, scope_id)
    }

    fn get_add_initial_attribute_to_scope_msg(
        &self,
        deps: &Deps<ProvenanceQuery>,
        attribute: &PayableScopeAttribute,
        contract_name: impl Into<String>,
    ) -> Result<CosmosMsg<ProvenanceMsg>, ContractError> {
        self.add_attribute(attribute.clone());
        ProvenanceUtilImpl.get_add_initial_attribute_to_scope_msg(deps, attribute, contract_name)
    }

    fn upsert_attribute_to_scope(
        &self,
        attribute: &PayableScopeAttribute,
        contract_name: impl Into<String>,
    ) -> Result<WriteAttributeMessages, ContractError> {
        self.add_attribute(attribute.clone());
        ProvenanceUtilImpl.upsert_attribute_to_scope(attribute, contract_name)
    }
}
impl MockProvenanceUtil {
    pub fn bind_captured_attribute(&self, deps: &mut MockOwnedDeps) {
        if let Some(attr) = self.captured_attributes.borrow().last() {
            mock_default_scope_attribute(deps, attr);
        }
    }

    pub fn bind_captured_attribute_named(&self, deps: &mut MockOwnedDeps, contract_name: impl Into<String>) {
        if let Some(attr) = self.captured_attributes.borrow().last() {
            mock_scope_attribute(deps, contract_name, attr);
        }
    }

    pub fn assert_attribute_matches_latest(&self, attribute: &PayableScopeAttribute) {
        if let Some(attr) = self.captured_attributes.borrow().last() {
            assert_eq!(
                attribute,
                attr,
                "the latest attribute captured via MockProvenanceUtil is not equivalent to the checked value",
            );
        } else {
            panic!("no attributes have ever been captured by MockProvenanceUtil");
        }
    }
}
