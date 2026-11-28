//! Engine-independent in-memory [`BudgetStore`] for `L3` conformance tests (`TASK-53`).

use std::collections::BTreeMap;
use std::sync::Mutex;

use org_proto::budgets::{BudgetPeriodKind, BudgetScopeKind};
use org_proto::ApiError;

use crate::{BudgetAggregate, BudgetError, BudgetStore};

type BudgetMap = BTreeMap<(String, String), BudgetAggregate>;

/// In-memory [`BudgetStore`] mirroring `SqliteBudgetStore` for engine-independent tests.
#[derive(Default)]
pub struct MemoryBudgetStore {
    inner: Mutex<BudgetMap>,
}

impl MemoryBudgetStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, BudgetMap>, BudgetError> {
        self.inner
            .lock()
            .map_err(|_| BudgetError::Invalid("memory budget store poisoned"))
    }
}

impl BudgetStore for MemoryBudgetStore {
    fn save_aggregate(
        &self,
        agg: &BudgetAggregate,
        _employee_kinds: &[(String, &'static str)],
    ) -> Result<Result<(), ApiError>, BudgetError> {
        let mut map = self.lock()?;
        map.insert(
            (agg.budget.organization_id.clone(), agg.budget.id.clone()),
            agg.clone(),
        );
        Ok(Ok(()))
    }

    fn get_by_id(
        &self,
        org_id: &str,
        budget_id: &str,
    ) -> Result<Option<BudgetAggregate>, BudgetError> {
        let map = self.lock()?;
        Ok(map.get(&(org_id.to_owned(), budget_id.to_owned())).cloned())
    }

    fn find_by_target(
        &self,
        org_id: &str,
        scope_kind: BudgetScopeKind,
        task_id: Option<&str>,
        agent_id: Option<&str>,
        release_id: Option<&str>,
        period_kind: BudgetPeriodKind,
    ) -> Result<Option<BudgetAggregate>, BudgetError> {
        let map = self.lock()?;
        Ok(map
            .values()
            .find(|a| {
                a.budget.organization_id == org_id
                    && a.budget.scope_kind == scope_kind
                    && a.budget.task_id.as_deref() == task_id
                    && a.budget.agent_id.as_deref() == agent_id
                    && a.budget.release_id.as_deref() == release_id
                    && a.budget.period_kind == period_kind
            })
            .cloned())
    }

    fn list_all(&self, org_id: &str) -> Result<Vec<BudgetAggregate>, BudgetError> {
        let map = self.lock()?;
        Ok(map
            .iter()
            .filter(|((o, _), _)| o == org_id)
            .map(|(_, a)| a.clone())
            .collect())
    }
}
