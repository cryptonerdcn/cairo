use cairo_lang_sierra::extensions::builtin_cost::CostTokenType;
use cairo_lang_sierra::extensions::core::CoreConcreteLibfunc;
use cairo_lang_sierra::ids::ConcreteTypeId;
use cairo_lang_sierra::program::StatementIdx;
use cairo_lang_sierra_ap_change::core_libfunc_ap_change::InvocationApChangeInfoProvider;
use cairo_lang_utils::collection_arithmetics::{add_maps, sub_maps};
use cairo_lang_utils::ordered_hash_map::OrderedHashMap;

use crate::core_libfunc_cost_base::{
    core_libfunc_postcost, core_libfunc_precost, CostOperations, InvocationCostInfoProvider,
};
use crate::cost_expr::{CostExpr, Var};
use crate::gas_info::GasInfo;
use crate::generate_equations::StatementFutureCost;

pub type CostExprMap = OrderedHashMap<CostTokenType, CostExpr>;

/// Cost operations for getting `CostExpr` costs values.
struct Ops<'a> {
    statement_future_cost: &'a mut dyn StatementFutureCost,
    idx: StatementIdx,
}
impl CostOperations for Ops<'_> {
    type CostType = CostExprMap;

    fn const_cost(&self, value: i32) -> Self::CostType {
        self.const_cost_token(value, CostTokenType::Step)
    }

    fn const_cost_token(&self, value: i32, token_type: CostTokenType) -> Self::CostType {
        Self::CostType::from_iter([(token_type, CostExpr::from_const(value))])
    }

    fn function_token_cost(
        &mut self,
        function: &cairo_lang_sierra::program::Function,
        token_type: CostTokenType,
    ) -> Self::CostType {
        Self::CostType::from_iter([(
            token_type,
            self.statement_future_cost.get_future_cost(&function.entry_point)[token_type].clone(),
        )])
    }

    fn statement_var_cost(&self, token_type: CostTokenType) -> Self::CostType {
        Self::CostType::from_iter([(
            token_type,
            CostExpr::from_var(Var::LibfuncImplicitGasVariable(self.idx, token_type)),
        )])
    }

    fn add(&self, lhs: Self::CostType, rhs: Self::CostType) -> Self::CostType {
        add_maps(lhs, rhs)
    }

    fn sub(&self, lhs: Self::CostType, rhs: Self::CostType) -> Self::CostType {
        sub_maps(lhs, rhs)
    }
}

/// Returns an expression for the gas cost for core libfuncs.
pub fn core_libfunc_precost_expr<InfoProvider: InvocationCostInfoProvider>(
    statement_future_cost: &mut dyn StatementFutureCost,
    idx: &StatementIdx,
    libfunc: &CoreConcreteLibfunc,
    info_provider: &InfoProvider,
) -> Vec<CostExprMap> {
    core_libfunc_precost(&mut Ops { statement_future_cost, idx: *idx }, libfunc, info_provider)
}

pub struct GasApInfoProvider<'a, InfoProvider: InvocationCostInfoProvider> {
    pub info_provider: &'a InfoProvider,
    pub gas_info: &'a GasInfo,
    pub idx: StatementIdx,
}

impl<'a, InfoProvider: InvocationCostInfoProvider> InvocationApChangeInfoProvider
    for GasApInfoProvider<'a, InfoProvider>
{
    fn type_size(&self, ty: &ConcreteTypeId) -> usize {
        self.info_provider.type_size(ty)
    }

    fn token_usages(&self, token_type: CostTokenType) -> usize {
        self.gas_info.variable_values.get(&(self.idx, token_type)).copied().unwrap_or(0) as usize
    }
}

/// Returns an expression for the gas cost for core libfuncs.
pub fn core_libfunc_postcost_expr<InfoProvider: InvocationCostInfoProvider>(
    statement_future_cost: &mut dyn StatementFutureCost,
    idx: &StatementIdx,
    libfunc: &CoreConcreteLibfunc,
    info_provider: &InfoProvider,
    precost_gas_info: &GasInfo,
) -> Vec<CostExprMap> {
    core_libfunc_postcost(
        &mut Ops { statement_future_cost, idx: *idx },
        libfunc,
        info_provider,
        &GasApInfoProvider { info_provider, gas_info: precost_gas_info, idx: *idx },
    )
}
